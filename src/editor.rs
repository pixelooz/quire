use std::{io::Write, path::Path};

use anyhow::Result;

use crate::{
    buffer::{CursorDir, TextBuffer},
    command::{Action, Command, CommandResult, NoAction, TextSearch},
    highlight::Highlighting,
    renderer::Renderer,
    status_bar::{Position, StatusBar},
    terminal::{Event, Key, KeySeq, Size},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditStep {
    Continue,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditAction {
    Move(CursorDir),
    MovePage(CursorDir),
    MoveToEdge(CursorDir),
    MoveByWord(CursorDir),
    MoveParagraph(CursorDir),
    InsertChar(char),
    InsertTab,
    InsertLine,
    DeleteChar,
    DeleteRightChar,
    DeleteWord,
    DeleteUntilLineEnd,
    DeleteUntilLineHead,
    Undo,
    Redo,
}

#[derive(Debug)]
pub struct Document {
    pub buffer: TextBuffer,
    pub highlight: Highlighting,
}

impl Document {
    pub fn new(buffer: TextBuffer) -> Self {
        let highlight = Highlighting::new(buffer.lang, buffer.rows());
        Self { buffer, highlight }
    }

    pub fn execute<W>(
        &mut self,
        edit_act: EditAction,
        renderer: &Renderer<W>,
    ) -> Option<&'static str>
    where
        W: Write,
    {
        use EditAction::*;
        match edit_act {
            Redo => (!self.buffer.redo()).then_some("Already at newest change"),
            Undo => (!self.buffer.undo()).then_some("No older change"),
            InsertChar(ch) => {
                self.buffer.insert_char(ch);
                None
            }
            InsertTab => {
                self.buffer.insert_tab();
                None
            }
            Move(dir) => {
                self.buffer.step(dir);
                None
            }
            MovePage(dir) => {
                self.buffer
                    .jump_page_up_down(dir, renderer.rowoff, renderer.rows());
                None
            }
            MoveParagraph(dir) => {
                self.buffer.jump_paragraphs(dir);
                None
            }
            MoveByWord(dir) => {
                self.buffer.step_by_word(dir);
                None
            }
            MoveToEdge(dir) => {
                self.buffer.jump_to_edge(dir);
                None
            }
            InsertLine => {
                self.buffer.insert_line();
                None
            }
            DeleteChar => {
                self.buffer.delete_char();
                None
            }
            DeleteRightChar => {
                self.buffer.delete_right_char();
                None
            }
            DeleteWord => {
                self.buffer.delete_word();
                None
            }
            DeleteUntilLineHead => {
                self.buffer.delete_until_line_head();
                None
            }
            DeleteUntilLineEnd => {
                self.buffer.delete_until_line_end();
                None
            }
        }
    }
}

pub struct Editor<I, W>
where
    W: Write,
    I: Iterator<Item = Result<Event>>,
{
    documents: Vec<Document>,
    doc_idx: usize,
    renderer: Renderer<W>,
    input: I,
    quitting: bool,
    status_bar: StatusBar,
}

impl<I, W> Editor<I, W>
where
    W: Write,
    I: Iterator<Item = Result<Event>>,
{
    pub fn new(input: I, output: W, size: Size) -> Result<Self> {
        let renderer = Renderer::new(size, output)?;
        let buffer = TextBuffer::empty();
        let status_bar = StatusBar::from_buffer(&buffer, Position { curr: 1, size: 1 });

        let document = Document::new(buffer);
        Ok(Self {
            documents: vec![document],
            doc_idx: 0,
            renderer,
            input,
            quitting: false,
            status_bar,
        })
    }

    pub fn open<P>(input: I, output: W, size: Size, paths: &[P]) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        if paths.is_empty() {
            return Self::new(input, output, size);
        }
        let renderer = Renderer::new(size, output)?;

        let mut documents = Vec::with_capacity(paths.len());
        for path in paths {
            let buffer = TextBuffer::open(path)?;
            documents.push(Document::new(buffer));
        }
        let status_bar = StatusBar::from_buffer(
            &documents[0].buffer,
            Position {
                curr: 1,
                size: documents.len(),
            },
        );
        Ok(Self {
            documents,
            doc_idx: 0,
            renderer,
            input,
            quitting: false,
            status_bar,
        })
    }

    pub fn doc_mut(&mut self) -> &mut Document {
        &mut self.documents[self.doc_idx]
    }

    pub fn doc(&self) -> &Document {
        &self.documents[self.doc_idx]
    }

    fn canvas_init(&mut self) -> Result<()> {
        self.render_screen()?;
        Ok(())
    }

    pub fn edit(&mut self) -> Result<()> {
        self.canvas_init()?;
        loop {
            if self.step()? != EditStep::Continue {
                break;
            }
        }
        Ok(())
    }

    fn step(&mut self) -> Result<EditStep> {
        let Some(event) = self.input.next().transpose()? else {
            return Ok(EditStep::Quit);
        };
        match event {
            Event::Resize { cols, rows } => {
                self.renderer.resize(Size {
                    width: cols,
                    height: rows,
                })?;
                self.renderer.set_redraw_idx(self.renderer.rowoff);
                self.status_bar.redraw = true;
                self.render_screen()?;
                Ok(EditStep::Continue)
            }
            Event::Key(seq) => {
                let step = self.process_keypress(seq)?;
                if step == EditStep::Continue {
                    self.render_screen()?;
                }
                Ok(step)
            }
        }
    }

    fn render_screen(&mut self) -> Result<()> {
        self.refresh_status_bar();
        let doc = &mut self.documents[self.doc_idx];
        self.renderer
            .render(&doc.buffer, &mut doc.highlight, &self.status_bar)?;
        self.status_bar.redraw = false;
        Ok(())
    }

    fn refresh_status_bar(&mut self) {
        self.status_bar.set_buf_pos(Position {
            curr: self.doc_idx + 1,
            size: self.documents.len(),
        });
        self.status_bar
            .update_from_buf(&self.documents[self.doc_idx].buffer);
    }

    fn prompt<A>(&mut self, prompt_text: &str, cmd_empty: bool) -> Result<CommandResult>
    where
        A: Action,
    {
        let doc = &mut self.documents[self.doc_idx];
        let mut cmd = Command::new(
            &mut self.renderer,
            &mut doc.buffer,
            &mut doc.highlight,
            cmd_empty,
            &mut self.status_bar,
        );
        cmd.run::<A, _, _>(prompt_text, &mut self.input)
    }

    fn save(&mut self) -> Result<()> {
        if self.doc().buffer.filename() == "[No Name]" {
            let result = self.prompt::<NoAction>("save as: ", true)?;
            match result {
                CommandResult::Canceled => {
                    self.renderer.set_info_msg("save canceled");
                    return Ok(());
                }
                CommandResult::Input(name) => {
                    self.doc_mut().buffer.set_file(name);
                }
            }
        }
        let doc = self.doc_mut();
        match doc.buffer.save() {
            Ok(msg) => self.renderer.set_info_msg(msg),
            Err(err) => self
                .renderer
                .set_error_msg(format!("Failed to save: {}", err)),
        }
        Ok(())
    }

    fn search(&mut self) -> Result<()> {
        self.prompt::<TextSearch>("search:", true)?;
        Ok(())
    }

    fn open_buffer(&mut self) -> Result<()> {
        let result = self.prompt::<NoAction>("open:", true)?;

        match result {
            CommandResult::Input(path) => match TextBuffer::open(&path) {
                Ok(buffer) => {
                    let doc = Document::new(buffer);
                    self.documents.push(doc);
                    self.switch_buffer(self.documents.len() - 1);
                }
                Err(err) => {
                    self.renderer
                        .set_error_msg(format!("Failed to open {}: {}", path, err));
                }
            },
            CommandResult::Canceled => self.renderer.set_info_msg("open cancelled"),
        }
        Ok(())
    }

    fn switch_buffer(&mut self, idx: usize) {
        let len = self.documents.len();
        if len <= 1 {
            return self
                .renderer
                .set_info_msg("No other buffer is opened!");
        }
        debug_assert!(idx < len);
        self.doc_idx = idx;

        // TODO: remember scroll position for each open buffer.
        self.renderer.rowoff = 0;
        self.renderer.coloff = 0;
        // A full force redraw because the entire context has been swapped.
        self.renderer.set_redraw_idx(0);
    }

    fn next_buffer(&mut self) {
        let mut next = self.doc_idx + 1;
        if self.doc_idx == self.documents.len() - 1 {
            next = 0; // Wrap around to the first buffer.
        }
        self.switch_buffer(next);
    }

    fn prev_buffer(&mut self) {
        let mut prev = self.doc_idx - 1;
        if self.doc_idx == 0 {
            prev = self.documents.len() - 1;
        }
        self.switch_buffer(prev);
    }

    fn show_help(&mut self) -> Result<()> {
        self.renderer.render_help()?;
        while let Some(event) = self.input.next().transpose()? {
            match event {
                Event::Key(seq) => {
                    if seq.key != Key::Unknown {
                        break;
                    }
                }
                Event::Resize { cols, rows } => {
                    self.renderer.resize(Size {
                        width: cols,
                        height: rows,
                    })?;
                    self.renderer.render_help()?;
                    self.status_bar.redraw = true;
                }
            }
        }
        // The loop has been broken, meaning the terminal is currently showing
        // the help text, but needs to show buffer content.
        self.renderer.set_redraw_idx(self.renderer.rowoff);
        Ok(())
    }

    fn process_keypress(&mut self, seq: KeySeq) -> Result<EditStep> {
        use Key::*;

        let prev_cursor = self.doc().buffer.cursor();
        let mut action: Option<EditAction> = None;

        match seq {
            KeySeq { alt: true, key, .. } => match key {
                Char('w') => action = Some(EditAction::MoveByWord(CursorDir::Right)),
                Char('b') => action = Some(EditAction::MoveByWord(CursorDir::Left)),
                Char('p') => action = Some(EditAction::MovePage(CursorDir::Up)),
                Char(']') => action = Some(EditAction::MoveParagraph(CursorDir::Down)),
                Char('[') => action = Some(EditAction::MoveParagraph(CursorDir::Up)),
                Char('<') | Up => action = Some(EditAction::MoveToEdge(CursorDir::Up)),
                Char('>') | Down => action = Some(EditAction::MoveToEdge(CursorDir::Down)),
                Left => action = Some(EditAction::MoveToEdge(CursorDir::Left)),
                Right => action = Some(EditAction::MoveToEdge(CursorDir::Right)),
                _ => self.handle_unmapped(&seq),
            },
            // Ctrl Commands: Editor Actions & Fast Movement
            KeySeq { ctrl: true, key, .. } => match key {
                // Editor State Commands
                Char('o') => self.open_buffer()?,
                Char('s') => self.save()?,
                Char('b') => self.prev_buffer(),
                Char('f') => self.next_buffer(),
                Char('q') => return Ok(self.handle_quit()),
                Char('7') | Char('?') => self.show_help()?,
                Char('t') => {
                    self.renderer.set_redraw_idx(self.renderer.rowoff);
                    self.renderer.remove_msg();
                    self.status_bar.redraw = true;
                }
                Char('g') => self.search()?,

                // Buffer Edits
                Char('u') => action = Some(EditAction::Undo),
                Char('r') => action = Some(EditAction::Redo),
                Char('<') => action = Some(EditAction::DeleteChar), // Same as Backspace
                Char('>') => action = Some(EditAction::DeleteRightChar),
                Char('w') => action = Some(EditAction::DeleteWord),
                Char('n') => action = Some(EditAction::DeleteUntilLineEnd),
                Char('p') => action = Some(EditAction::DeleteUntilLineHead),
                Char('i') => action = Some(EditAction::InsertTab),

                // Buffer Movement
                Char('k') => action = Some(EditAction::Move(CursorDir::Up)),
                Char('j') => action = Some(EditAction::Move(CursorDir::Down)),
                Char('h') => action = Some(EditAction::Move(CursorDir::Left)),
                Char('l') => action = Some(EditAction::Move(CursorDir::Right)),
                Char('v') | Char(']') => action = Some(EditAction::MovePage(CursorDir::Down)),
                Char('a') => action = Some(EditAction::MoveToEdge(CursorDir::Left)),
                Char('e') => action = Some(EditAction::MoveToEdge(CursorDir::Right)),

                // Ctrl + Arrows (Semantic movement mapping for modern keyboards)
                Left => action = Some(EditAction::MoveByWord(CursorDir::Left)),
                Right => action = Some(EditAction::MoveByWord(CursorDir::Right)),
                Up => action = Some(EditAction::MoveParagraph(CursorDir::Up)),
                Down => action = Some(EditAction::MoveParagraph(CursorDir::Down)),

                _ => self.handle_unmapped(&seq),
            },
            KeySeq { key, .. } => match key {
                Char(ch) => action = Some(EditAction::InsertChar(ch)),
                Enter => action = Some(EditAction::InsertLine),
                Backspace => action = Some(EditAction::DeleteChar),
                Delete => action = Some(EditAction::DeleteRightChar),
                Tab => action = Some(EditAction::InsertTab),

                Right => action = Some(EditAction::Move(CursorDir::Right)),
                Left => action = Some(EditAction::Move(CursorDir::Left)),
                Up => action = Some(EditAction::Move(CursorDir::Up)),
                Down => action = Some(EditAction::Move(CursorDir::Down)),

                Home => action = Some(EditAction::MoveToEdge(CursorDir::Left)),
                End => action = Some(EditAction::MoveToEdge(CursorDir::Right)),
                PageUp => action = Some(EditAction::MovePage(CursorDir::Up)),
                PageDown => action = Some(EditAction::MovePage(CursorDir::Down)),

                Esc | Unknown => {}
            },
        }

        // --- Execution Phase ---
        if let Some(edit_act) = action
            && let Some(msg) = self.documents[self.doc_idx].execute(edit_act, &self.renderer)
        {
            self.renderer.set_info_msg(msg);
        }
        let curr_doc = self.doc_mut();

        if let Some(redraw_row) = curr_doc.buffer.commit_edit() {
            curr_doc.highlight.needs_update = true;
            self.renderer.set_redraw_idx(redraw_row);
        }
        if self.doc().buffer.cursor() != prev_cursor {
            self.renderer.cursor_moved = true;
        }
        self.quitting = false;
        Ok(EditStep::Continue)
    }

    fn handle_quit(&mut self) -> EditStep {
        let has_unsaved = self.documents.iter().any(|d| d.buffer.modified());
        if !has_unsaved || self.quitting {
            return EditStep::Quit;
        }
        self.quitting = true;
        self.renderer
            .set_error_msg("There are unsaved changes! Press ^Q again to quit");
        EditStep::Continue
    }

    fn handle_unmapped(&mut self, seq: &KeySeq) {
        let modifier = if seq.ctrl {
            "^"
        } else if seq.alt {
            "Alt-"
        } else {
            ""
        };
        let key_str = match seq.key {
            Key::Char(ch) => ch.to_string(),
            _ => format!("{:?}", seq.key),
        };
        self.renderer
            .set_error_msg(format!("Key '{}{}' not mapped", modifier, key_str));
    }
}
