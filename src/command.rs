use std::{cmp, io::Write};

use anyhow::Result;

use crate::{
    buffer::TextBuffer,
    color::UiElement,
    diff::CursorPosition,
    highlight::{Highlighting, RegionHighlight},
    renderer::Renderer,
    row::Row,
    status_bar::StatusBar,
    terminal::{Event, KeySeq, Size},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandResult {
    Canceled,
    Input(String),
}

pub trait Action: Sized {
    fn new<W: Write>(cmd: &mut Command<'_, W>) -> Self;

    fn on_seq<W>(&mut self, _cmd: &mut Command<'_, W>, _input: &str, _seq: KeySeq) -> Result<bool>
    where
        W: Write,
    {
        Ok(false)
    }

    fn on_end<W>(self, _cmd: &mut Command<'_, W>, result: CommandResult) -> Result<CommandResult>
    where
        W: Write,
    {
        Ok(result)
    }
}

pub struct NoAction;

impl Action for NoAction {
    fn new<W: Write>(_cmd: &mut Command<'_, W>) -> Self {
        Self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchDirection {
    Backward,
    Forward,
}

#[derive(Debug)]
pub struct TextSearch {
    saved_cursor: CursorPosition,
    saved_scroll: CursorPosition,
    direction: SearchDirection,
    matched: bool,
    /// Flattened buffer for fast str::find.
    raw_buf_str: Box<str>,
    /// Current cursor position as a 1D byte offset.
    curr_offset: usize,
    /// Byte index where each line begins.
    row_offsets: Box<[usize]>,
}

impl TextSearch {
    fn cleanup_match_highlight<W: Write>(&self, cmd: &mut Command<'_, W>) {
        if !self.matched {
            return;
        }
        // Clear highlights and force a redraw from the top of the screen.
        cmd.highlight.clear_matches();
        cmd.highlight.needs_update = true;
        cmd.renderer.set_redraw_idx(cmd.renderer.rowoff);
    }

    fn handle_key_seq(&mut self, seq: &KeySeq) {
        use crate::terminal::Key::*;
        match (seq.key, seq.ctrl) {
            (Right, _) | (Down, _) | (Char('f'), true) | (Char('n'), true) => {
                self.direction = SearchDirection::Forward;
            }
            (Left, _) | (Up, _) | (Char('b'), true) | (Char('p'), true) => {
                self.direction = SearchDirection::Backward;
            }
            // Any typing unset the match state so we start from scratch.
            _ => {
                self.matched = false;
            }
        }
    }

    fn reject_match_to_current(&mut self) {
        // If we hit "next match", nudge the offset forward/backward by exactly
        // one character so `str::find` doesn't just re-find the active match.
        self.curr_offset = match self.direction {
            SearchDirection::Backward => self.raw_buf_str[..self.curr_offset]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or_else(|| self.raw_buf_str.len()),
            SearchDirection::Forward => self.raw_buf_str[self.curr_offset..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.curr_offset + i)
                .unwrap_or(0),
        }
    }

    fn nearest_line(&self, byte_offset: usize) -> usize {
        match self.row_offsets.binary_search(&byte_offset) {
            Ok(idx) => idx,
            Err(idx) => idx.saturating_sub(1),
        }
    }

    fn offset_to_pos(&self, byte_offset: usize, rows: &[Row]) -> CursorPosition {
        let row_idx = self.nearest_line(byte_offset);
        let row_start = self.row_offsets[row_idx];
        let col_byte_idx = byte_offset - row_start;
        let col_idx = rows[row_idx].byte_to_char_idx(col_byte_idx);
        CursorPosition { col_idx, row_idx }
    }

    fn pos_to_offset(&self, cursor: CursorPosition, rows: &[Row]) -> usize {
        // Write the calculation in comments.
        let (col_idx, row_idx) = (cursor.col_idx, cursor.row_idx);
        let col_byte_idx = rows[row_idx].char_to_byte_idx(col_idx);
        self.row_offsets[row_idx] + col_byte_idx
    }

    fn find_at(&self, query: &str, offset: usize) -> Option<usize> {
        match self.direction {
            SearchDirection::Backward => self.raw_buf_str[..offset]
                .rfind(query)
                .or_else(|| {
                    self.raw_buf_str[offset..]
                        .rfind(query)
                        .map(|idx| idx + offset)
                }),
            SearchDirection::Forward => self.raw_buf_str[offset..]
                .find(query)
                .map(|idx| idx + offset)
                .or_else(|| self.raw_buf_str.find(query)),
        }
    }

    fn highlight_matches<W: Write>(
        &self,
        query: &str,
        renderer: &Renderer<W>,
        curr_match: RegionHighlight,
        rows: &[Row],
    ) -> Vec<RegionHighlight> {
        let mut matches = vec![];

        let screen_start = renderer.rowoff;
        let screen_end = cmp::min(screen_start + renderer.rows() + 1, rows.len());

        let cursor = CursorPosition::new(0, screen_start);
        let start_offset = self.pos_to_offset(cursor, rows);

        let end_offset = if screen_end == rows.len() {
            self.raw_buf_str.len()
        } else {
            let cursor = CursorPosition::new(0, screen_end);
            self.pos_to_offset(cursor, rows)
        };

        // Find all the other matches in the visible screen area.
        for (idx, _) in self.raw_buf_str[start_offset..end_offset].match_indices(query) {
            let offset = start_offset + idx;
            if offset == self.curr_offset {
                continue;
            }
            matches.push(RegionHighlight {
                ui_element: UiElement::SearchMatch,
                start: self.offset_to_pos(offset, rows),
                end: self.offset_to_pos(offset + query.len(), rows),
            });
        }
        // Push the active match last so it overlays on top.
        matches.push(curr_match);
        matches
    }

    fn search<W: Write>(&mut self, input: &str, cmd: &mut Command<'_, W>) {
        match self.find_at(input, self.curr_offset) {
            Some(offset) => self.curr_offset = offset,
            None => return,
        }
        let curr_match = RegionHighlight {
            ui_element: UiElement::CurrentMatch,
            start: self.offset_to_pos(self.curr_offset, cmd.buffer.rows()),
            end: self.offset_to_pos(self.curr_offset + input.len(), cmd.buffer.rows()),
        };
        let cursor = curr_match.start;
        cmd.buffer.set_cursor(cursor);

        // Force the viewport to follow the screen search result, keeping it
        // somewhat centered.
        cmd.renderer.rowoff = cursor
            .row_idx
            .saturating_sub(cmd.renderer.rows() / 2);
        cmd.renderer.coloff = 0;

        let matches = self.highlight_matches(input, cmd.renderer, curr_match, cmd.buffer.rows());

        cmd.highlight.set_matches(matches);
        cmd.highlight.needs_update = true;

        self.matched = true;
        cmd.renderer.set_redraw_idx(cmd.renderer.rowoff);
    }
}

impl Action for TextSearch {
    fn new<W: Write>(cmd: &mut Command<'_, W>) -> Self {
        let rows = cmd.buffer.rows();
        // Adding an extra 1 for each row because we'll append '\n' for each line
        //  in the flattened text to indicate new lines.
        let capacity = rows
            .iter()
            .fold(0, |acc, row| acc + row.buffer().len() + 1);

        let mut row_offsets = Vec::with_capacity(rows.len());
        let mut raw_buf_str = String::with_capacity(capacity);
        let mut offset = 0;

        for row in rows {
            row_offsets.push(offset);
            raw_buf_str.push_str(row.buffer());
            raw_buf_str.push('\n');
            offset += row.buffer().len() + 1;
        }
        let cursor = CursorPosition::new(cmd.buffer.col_idx(), cmd.buffer.row_idx());
        let scroll = CursorPosition::new(cmd.renderer.coloff, cmd.renderer.rowoff);
        let mut search = Self {
            saved_cursor: cursor,
            saved_scroll: scroll,
            direction: SearchDirection::Forward,
            matched: false,
            raw_buf_str: raw_buf_str.into_boxed_str(),
            curr_offset: 0,
            row_offsets: row_offsets.into_boxed_slice(),
        };
        let col_idx = cmd.buffer.col_idx();
        let row_idx = cmp::min(cmd.buffer.row_idx(), rows.len().saturating_sub(1));

        let cursor = CursorPosition::new(col_idx, row_idx);
        search.curr_offset = search.pos_to_offset(cursor, rows);
        search
    }

    fn on_seq<W>(&mut self, cmd: &mut Command<'_, W>, input: &str, seq: KeySeq) -> Result<bool>
    where
        W: Write,
    {
        self.cleanup_match_highlight(cmd);
        self.handle_key_seq(&seq);
        if input.is_empty() {
            return Ok(false);
        }
        if self.matched {
            self.reject_match_to_current();
        }
        self.search(input, cmd);
        Ok(true)
    }

    fn on_end<W>(self, cmd: &mut Command<'_, W>, result: CommandResult) -> Result<CommandResult>
    where
        W: Write,
    {
        self.cleanup_match_highlight(cmd);
        use CommandResult::*;

        let result = match &result {
            Canceled => Canceled,
            Input(string) if string.is_empty() => Canceled,
            Input(_) if self.matched => {
                cmd.renderer.set_info_msg("Found");
                result
            }
            Input(_) => {
                cmd.renderer.set_info_msg("Not Found");
                result
            }
        };
        if result == Canceled {
            let cursor = self.saved_cursor;
            let scroll = self.saved_scroll;
            cmd.buffer.set_cursor(cursor);
            cmd.renderer.coloff = scroll.col_idx;
            cmd.renderer.rowoff = scroll.row_idx;
            cmd.renderer.set_redraw_idx(cmd.renderer.rowoff);
        }
        Ok(result)
    }
}

pub(crate) struct CommandTemplate<'a> {
    prefix: &'a str,
    suffix: &'a str,
    prefix_chars: usize,
}

impl<'a> CommandTemplate<'a> {
    pub fn new(prefix: &'a str, suffix: &'a str) -> Self {
        Self {
            prefix,
            suffix,
            prefix_chars: prefix.chars().count(),
        }
    }

    pub fn build(&self, input: &str) -> String {
        let capacity = self.prefix.len() + self.suffix.len() + input.len();
        let mut buf = String::with_capacity(capacity);
        buf.push_str(self.prefix);
        buf.push_str(input);
        buf.push_str(self.suffix);
        buf
    }

    pub fn cursor_col(&self, input: &str) -> usize {
        self.prefix_chars + input.chars().count()
    }
}

pub struct Command<'a, W: Write> {
    pub renderer: &'a mut Renderer<W>,
    pub buffer: &'a mut TextBuffer,
    pub highlight: &'a mut Highlighting,
    pub cmd_empty: bool,
    pub status_bar: &'a mut StatusBar,
}

impl<'a, W: Write> Command<'a, W> {
    pub fn new(
        renderer: &'a mut Renderer<W>,
        buffer: &'a mut TextBuffer,
        highlight: &'a mut Highlighting,
        cmd_empty: bool,
        status_bar: &'a mut StatusBar,
    ) -> Self {
        Self {
            renderer,
            buffer,
            highlight,
            cmd_empty,
            status_bar,
        }
    }

    fn render_screen(&mut self, input: &str, cmd_template: &CommandTemplate) -> Result<()> {
        self.renderer
            .set_info_msg(cmd_template.build(input));
        self.status_bar.update_from_buf(self.buffer);
        self.renderer
            .render(self.buffer, self.highlight, self.status_bar)?;

        let prompt_row = self.renderer.rows() + 1;
        let prompt_col = cmd_template.cursor_col(input);
        self.renderer
            .force_set_cursor(prompt_col, prompt_row)?;
        Ok(())
    }

    pub fn run<A, T, I>(&mut self, cmd: T, input: &mut I) -> Result<CommandResult>
    where
        A: Action,
        T: AsRef<str>,
        I: Iterator<Item = Result<Event>>,
    {
        let mut action = A::new(self);
        let mut cmd_buf = String::new();
        let mut canceled = false;

        // Parse the command template (e.g. "Save as: {} (ESC to cancel)").
        let cmd_template = {
            let mut parts = cmd.as_ref().splitn(2, "{}");
            let prefix = parts.next().unwrap_or("");
            let suffix = parts.next().unwrap_or("");
            CommandTemplate::new(prefix, suffix)
        };
        // Initial render before waiting for any input.
        self.render_screen("", &cmd_template)?;

        while let Some(event) = input.next().transpose()? {
            let prev_len = cmd_buf.len();

            match event {
                Event::Resize { cols, rows } => {
                    self.renderer.resize(Size {
                        width: cols,
                        height: rows,
                    })?;
                    self.renderer.set_redraw_idx(self.renderer.rowoff);
                    self.status_bar.redraw = true;
                    self.render_screen(&cmd_buf, &cmd_template)?;
                    continue;
                }
                Event::Key(seq) => {
                    use crate::terminal::Key::*;
                    match (seq.key, seq.ctrl) {
                        (Backspace, _) | (Delete, _) | (Char('h'), true) => {
                            if !cmd_buf.is_empty() {
                                cmd_buf.pop();
                            }
                        }
                        (Char('w'), true) => {
                            let is_sep =
                                |ch: char| ch.is_ascii_whitespace() || ch.is_ascii_punctuation();

                            while let Some(curr) = cmd_buf.pop() {
                                let next_is_sep = cmd_buf.chars().last().is_none_or(is_sep);
                                if !is_sep(curr) && next_is_sep {
                                    break;
                                }
                            }
                        }
                        (Char('j'), true) => cmd_buf.clear(),
                        (Esc, _) | (Char('q'), true) | (Char('g'), true) => {
                            canceled = true;
                            break;
                        }
                        (Unknown, _) => continue,
                        (Enter, _) | (Char('m'), true) => break,
                        (Char(ch), false) => cmd_buf.push(ch),
                        _ => {}
                    }
                    let should_render = action.on_seq(self, &cmd_buf, seq)?;

                    if should_render || prev_len != cmd_buf.len() {
                        self.render_screen(&cmd_buf, &cmd_template)?;
                    }
                }
            }
        }
        let result = if canceled || (self.cmd_empty && cmd_buf.is_empty()) {
            self.renderer.set_info_msg("Canceled");
            CommandResult::Canceled
        } else {
            self.renderer.remove_msg();
            self.status_bar.redraw = true;
            CommandResult::Input(cmd_buf)
        };
        action.on_end(self, result)
    }
}
