use std::{
    cmp,
    fs::File,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    slice,
};

use anyhow::{Context, Result};

use crate::{
    diff::{CursorPosition, EditDiff},
    history::History,
    lang::{Indent, Language},
    row::Row,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorDir {
    Left,
    Right,
    Up,
    Down,
}

/// Contains both, actual sequence and display string.
#[derive(Debug)]
pub struct FilePath {
    pub path: PathBuf,
    pub display: String,
}

impl FilePath {
    /// Returns `FilePath` constructed from the given string or a type that implements
    /// `Into<String>`.
    fn from_string<T: Into<String>>(string: T) -> Self {
        let display = string.into();
        Self {
            path: PathBuf::from(&display),
            display,
        }
    }

    /// Returns `FilePath` constructed from the given path or a type that implements
    /// `AsRef<Path>`.
    ///
    /// `[FilePath::display]` is constructed using `path.to_string_lossy()`.
    fn from_path<P: AsRef<Path>>(path: P) -> Self {
        let path = path.as_ref();
        Self {
            path: PathBuf::from(path),
            display: path.to_string_lossy().to_string(),
        }
    }
}

pub struct Lines<'a>(slice::Iter<'a, Row>);

impl<'a> ExactSizeIterator for Lines<'a> {}

impl<'a> Iterator for Lines<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|x| x.buffer())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

#[derive(Debug, Default)]
pub struct TextBuffer {
    /// (col/x) co-ordinate in the internal text buffer of rows - the raw text.
    col_idx: usize,

    /// (row/y) co-ordinate in the internal text buffer of rows - the raw text.
    row_idx: usize,

    /// File the editor is actually opening.
    file: Option<FilePath>,

    /// Lines inside the text buffer.
    rows: Vec<Row>,

    /// Count of how many times undo points were created in the buffer. This
    /// value is set to 0 after just loading the buffer. When saving the
    /// buffer to file, count is reset to 0.
    /// When redo/undo is applied without ongoing changes, this count is just
    /// incremented or decremented.
    undo_count: i32,

    /// True when there are uncommitted edits in the `ongoing` buffer.
    /// This flag is necessary because `undo_count` only tracks modifications
    /// that have been fully sealed into history. This flag catches the
    /// "in-progress" batch before an undo-point is created.
    modified: bool,

    /// Language which current buffer belongs to.
    pub lang: Language,

    /// History per undo point for undo/redo.
    history: History,

    /// Flag to ensure at most one undo point per one key input.
    inserted_undo: bool,

    /// Flag to require screen re-rendering. The value represents the row from
    /// where we need to re-render the editor.
    redraw_from: Option<usize>,
}

impl TextBuffer {
    /// Returns an empty `TextBuffer`.
    pub fn empty() -> Self {
        Self {
            rows: vec![Row::empty()],
            redraw_from: Some(0),
            ..Default::default()
        }
    }

    pub fn with_lines<T, I>(lines: I) -> Result<Self>
    where
        T: AsRef<str>,
        I: Iterator<Item = T>,
    {
        let rows = lines
            .map(|line| Row::new(line.as_ref()))
            .collect::<Result<_>>()?;
        let mut buf = Self::empty();
        buf.rows = rows;
        Ok(buf)
    }

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let file = FilePath::from_path(path);

        if !path.try_exists()? {
            // When the path does not exist, consider it a new file.
            let mut buf = Self::empty();
            buf.file = Some(file);
            buf.lang = Language::detect(path);
            return Ok(buf);
        }
        let rows = io::BufReader::new(File::open(path)?)
            .lines()
            .map(|line_result| {
                let mut line = line_result?;
                if line.ends_with('\r') {
                    line.pop();
                }
                Row::new(line)
            })
            .collect::<Result<_>>()?;

        let mut buf = Self::empty();
        buf.file = Some(file);
        buf.rows = rows;
        buf.lang = Language::detect(path);
        Ok(buf)
    }

    pub fn save(&mut self) -> Result<String> {
        self.insert_undo_point();

        let Some(fp) = &self.file else {
            return Ok(String::new());
        };
        let file = File::create(&fp.path).context("Could not save")?;

        let mut writer = io::BufWriter::new(file);
        let mut bytes = 0;

        for line in self.rows.iter() {
            let text = line.buffer();
            // check to see if using write_all() directly affects performance
            // for large files.
            writeln!(writer, "{}", text).context("Could not write to file")?;
            bytes += text.len() + 1;
        }
        writer
            .flush()
            .context("Could not flush to file")?;
        self.undo_count = 0;
        self.modified = false;
        Ok(format!("{} bytes written to {}", bytes, fp.display))
    }

    pub fn is_scratch(&self) -> bool {
        self.file.is_none() && self.rows.len() == 1 && self.rows[0].len() == 0
    }
}

impl TextBuffer {
    fn set_redraw_idx(&mut self, line: usize) {
        if let Some(row_idx) = self.redraw_from {
            if row_idx <= line {
                return;
            }
        }
        self.redraw_from = Some(line)
    }

    pub fn col_idx(&self) -> usize {
        self.col_idx
    }

    pub fn row_idx(&self) -> usize {
        self.row_idx
    }

    fn curr_row(&self) -> &Row {
        &self.rows[self.row_idx]
    }

    pub fn set_cursor(&mut self, cursor: CursorPosition) {
        self.col_idx = cursor.col_idx;
        self.row_idx = cursor.row_idx;
    }

    pub fn cursor(&self) -> CursorPosition {
        CursorPosition {
            col_idx: self.col_idx,
            row_idx: self.row_idx,
        }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn set_unnamed(&mut self) {
        self.file = None
    }

    pub fn filename(&self) -> &str {
        self.file
            .as_ref()
            .map(|fp| fp.display.as_str())
            .unwrap_or("[No Name]")
    }

    pub fn modified(&self) -> bool {
        self.undo_count != 0 || self.modified
    }

    pub fn lines(&self) -> Lines<'_> {
        Lines(self.rows.iter())
    }

    pub fn set_file<T: Into<String>>(&mut self, path: T) {
        let fp = FilePath::from_string(path);
        self.lang = Language::detect(&fp.path);
        self.file = Some(fp);
    }
}

impl TextBuffer {
    pub fn insert_char(&mut self, ch: char) {
        // we don't add a undo point here to group multiple insert_char changes
        // into one undo point.
        if self.row_idx == self.rows.len() {
            self.new_diff(EditDiff::InsertLine {
                row: self.row_idx,
                text: "".to_string(),
            });
        }
        self.new_diff(EditDiff::InsertChar {
            at: CursorPosition {
                col_idx: self.col_idx,
                row_idx: self.row_idx,
            },
            ch,
        });
    }

    pub fn insert_tab(&mut self) {
        self.insert_undo_point();
        match self.lang.indent() {
            Indent::FourSpaces(indent) => {
                let cursor = CursorPosition {
                    col_idx: self.col_idx,
                    row_idx: self.row_idx,
                };
                self.new_diff(EditDiff::Insert {
                    at: cursor,
                    text: indent.to_owned(),
                });
            }
            Indent::Tab => self.insert_char('\t'),
        }
    }

    pub fn delete_char(&mut self) {
        if self.row_idx == self.rows.len() || self.col_idx == 0 && self.row_idx == 0 {
            return;
        }
        self.insert_undo_point();
        if self.col_idx == 0 {
            return self.squash_to_previous_line();
        }
        let col_idx = self.col_idx - 1;
        let deleted = self.curr_row().char_at(col_idx);
        let cursor = CursorPosition {
            col_idx: self.col_idx,
            row_idx: self.row_idx,
        };
        self.new_diff(EditDiff::DeleteChar {
            at: cursor,
            ch: deleted,
        });
    }

    pub fn delete_right_char(&mut self) {
        if self.row_idx == self.rows.len()
            || self.row_idx == self.rows.len() - 1 && self.col_idx == self.curr_row().len()
        {
            // nothing can be deleted at the end of the buffer and the cursor
            // should not move.
            return;
        }
        self.step(CursorDir::Right);
        self.delete_char();
    }

    pub fn insert_line(&mut self) {
        self.insert_undo_point();

        if self.row_idx >= self.rows.len() {
            self.new_diff(EditDiff::InsertLine {
                row: self.row_idx,
                text: "".to_string(),
            });
        } else if self.col_idx >= self.curr_row().len() {
            self.new_diff(EditDiff::InsertLine {
                row: self.row_idx + 1,
                text: "".to_string(),
            });
        } else if self.col_idx <= self.curr_row().buffer().len() {
            let truncated = self.curr_row()[self.col_idx..].to_owned();
            self.new_diff(EditDiff::Truncate {
                row: self.row_idx,
                removed: truncated.clone(),
            });
            self.new_diff(EditDiff::InsertLine {
                row: self.row_idx + 1,
                text: truncated,
            });
        }
    }

    pub fn delete_word(&mut self) {
        if self.col_idx == 0 || self.row_idx == self.rows.len() {
            return;
        }
        self.insert_undo_point();

        let line = self.curr_row();
        let mut colx = self.col_idx - 1;

        // if we are on a whitespace we'll keep going back until we encounter an
        // alphanumeric character.
        while colx > 0 && line.char_at(colx).is_ascii_whitespace() {
            colx -= 1;
        }

        // we keep going back until we encounter a space before cur word meaning
        // we are pointing at the start of the word.
        while colx > 0 && !line.char_at(colx - 1).is_ascii_whitespace() {
            colx -= 1;
        }
        let removed = line[colx..self.col_idx].to_owned();

        let cursor = CursorPosition {
            col_idx: colx,
            row_idx: self.row_idx,
        };
        self.new_diff(EditDiff::Remove {
            at: cursor,
            text: removed,
        });
    }

    pub fn delete_until_line_end(&mut self) {
        if self.row_idx == self.rows.len() {
            return;
        }
        self.insert_undo_point();

        let row = self.curr_row();
        if self.col_idx == row.len() {
            // do nothing when cursor is at the end of line and at the end of the
            // text buffer; basically the last line and the last character in it.
            if self.row_idx == self.rows.len() - 1 {
                return;
            }
            /* TODO: This can be extended to support vim's `Shift-J` keypress where
            it doesn't matter where the cursor was, the next line gets concatenated
            to the end of current line.*/
            self.concat_next_line();
        } else if self.col_idx < row.buffer().len() {
            let truncated = row[self.col_idx..].to_owned();
            self.new_diff(EditDiff::Truncate {
                row: self.row_idx,
                removed: truncated,
            });
        }
    }

    pub fn delete_until_line_head(&mut self) {
        if self.col_idx == 0 && self.row_idx == 0 || self.row_idx == self.rows.len() {
            return;
        }
        self.insert_undo_point();

        if self.col_idx == 0 {
            self.squash_to_previous_line();
        } else {
            let removed = self.curr_row()[..self.col_idx].to_owned();
            let cursor = CursorPosition {
                col_idx: 0,
                row_idx: self.row_idx,
            };
            self.new_diff(EditDiff::Remove {
                at: cursor,
                text: removed,
            });
        }
    }

    /// At the beginning of a line, backspace concats current line to previous line.
    fn squash_to_previous_line(&mut self) {
        // move cursor to previous line/row.
        self.row_idx -= 1;
        // move cursor to the end of now current row.
        self.col_idx = self.curr_row().len();
        self.concat_next_line();
    }

    fn concat_next_line(&mut self) {
        let removed = self.rows[self.row_idx + 1].take_buffer();
        self.new_diff(EditDiff::DeleteLine {
            row: self.row_idx + 1,
            text: removed.clone(),
        });
        self.new_diff(EditDiff::Append {
            row: self.row_idx,
            text: removed,
        });
    }

    fn new_diff(&mut self, diff: EditDiff) {
        let cursor = diff.apply(&mut self.rows);
        self.set_cursor(cursor);
        self.set_redraw_idx(cursor.row_idx);
        self.modified = true;
        self.history.push_diff(diff);
    }

    pub fn commit_edit(&mut self) -> Option<usize> {
        self.inserted_undo = false;
        let redraw_idx = self.redraw_from;
        self.redraw_from = None;
        redraw_idx
    }

    fn insert_undo_point(&mut self) {
        if !self.inserted_undo {
            if self.history.queue_ongoing_edits() {
                self.undo_count = self.undo_count.saturating_add(1);
            }
            self.modified = false;
            self.inserted_undo = true;
        }
    }

    fn after_undo_redo(&mut self, state: Option<(CursorPosition, usize, bool)>) -> bool {
        match state {
            Some((cursor, redraw_idx, _)) => {
                self.set_cursor(cursor);
                self.set_redraw_idx(redraw_idx);
                true
            }
            None => false,
        }
    }

    pub fn undo(&mut self) -> bool {
        let state = self.history.undo(&mut self.rows);
        if let Some((_, _, edited)) = state {
            // If edited is true, it means that undo target is the ongoing change.
            // In this case, undo point is not consumed and undo count should not
            // be decremented.
            if !edited {
                self.undo_count = self.undo_count.saturating_sub(1);
            }
            self.modified = false;
        };
        self.after_undo_redo(state)
    }

    pub fn redo(&mut self) -> bool {
        let state = self.history.redo(&mut self.rows);
        if let Some((_, _, edited)) = state {
            // If edited is true, it means that redo target is the ongoing change.
            // In this case, undo point is not consumed and undo count should not
            // be incremented.
            if !edited {
                self.undo_count = self.undo_count.saturating_add(1);
            }
            self.modified = false;
        };
        self.after_undo_redo(state)
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum CharKind {
    Punctuation,
    Space,
    Identifier,
}

impl CharKind {
    pub fn from_char(ch: char) -> Self {
        if ch.is_ascii_whitespace() {
            CharKind::Space
        } else if ch == '_' || ch.is_ascii_alphanumeric() {
            CharKind::Identifier
        } else {
            CharKind::Punctuation
        }
    }
}

impl TextBuffer {
    pub fn step(&mut self, direction: CursorDir) {
        match direction {
            CursorDir::Up => self.row_idx = self.row_idx.saturating_sub(1),
            CursorDir::Down => {
                if self.row_idx + 1 < self.rows.len() {
                    self.row_idx += 1;
                }
            }
            CursorDir::Left => {
                if self.col_idx > 0 {
                    self.col_idx -= 1;
                } else if self.row_idx > 0 {
                    self.row_idx -= 1;
                    self.col_idx = self.curr_row().len();
                }
            }
            CursorDir::Right => {
                if self.row_idx < self.rows.len() {
                    let len = self.curr_row().len();

                    if self.col_idx < len {
                        self.col_idx += 1;
                    } else if self.row_idx + 1 < self.rows.len() {
                        self.row_idx += 1;
                        self.col_idx = 0;
                    }
                }
            }
        }
        // snap cursor to the end of line when moving up/down from a longer line.
        let curr_row_len = self
            .rows
            .get(self.row_idx)
            .map(|r| r.len())
            .unwrap_or(0);
        if self.col_idx > curr_row_len {
            self.col_idx = curr_row_len;
        }
    }

    // TODO: document the panic behaviour
    pub fn jump_page_up_down(&mut self, direction: CursorDir, row_off: usize, num_rows: usize) {
        self.row_idx = match direction {
            CursorDir::Up => row_off,
            CursorDir::Down => cmp::min(row_off + num_rows - 1, self.rows.len()),
            _ => unreachable!(),
        };
        for _ in 0..num_rows {
            self.step(direction);
        }
    }

    pub fn jump_to_edge(&mut self, direction: CursorDir) {
        match direction {
            CursorDir::Left => self.col_idx = 0,
            CursorDir::Right => {
                if self.row_idx < self.rows.len() {
                    self.col_idx = self.curr_row().len();
                }
            }
            CursorDir::Up => self.row_idx = 0,
            CursorDir::Down => self.row_idx = self.rows.len(),
        }
    }

    pub fn step_by_word(&mut self, direction: CursorDir) {
        // Helper to safely fetch the CharKind at the current cursor position.
        let get_kind = |buf: &Self| -> CharKind {
            buf.rows
                .get(buf.row_idx)
                .and_then(|r| r.chat_at_checked(buf.col_idx))
                .map(CharKind::from_char)
                .unwrap_or(CharKind::Space) // Treat EOF/EOL as Space.
        };
        let mut prev = get_kind(self);
        loop {
            self.step(direction);

            // did we hit the absolute start or end of the file?
            if (self.row_idx == 0 && self.col_idx == 0) || self.row_idx == self.rows.len() {
                return;
            }
            let curr = get_kind(self);
            match direction {
                CursorDir::Left if Self::is_word_boundary(curr, prev) => {
                    return;
                }
                CursorDir::Right if Self::is_word_boundary(prev, curr) => {
                    return;
                }
                _ => {} // up/down are invalid so do nothing.
            }
            prev = curr
        }
    }

    pub fn jump_paragraphs(&mut self, direction: CursorDir) {
        debug_assert!(direction != CursorDir::Left && direction != CursorDir::Right);
        loop {
            self.step(direction);
            if self.row_idx == 0
                || self.row_idx == self.rows.len()
                || self.rows[self.row_idx - 1].buffer().is_empty()
                    && !self.curr_row().buffer().is_empty()
            {
                break;
            }
        }
    }

    fn is_word_boundary(left: CharKind, right: CharKind) -> bool {
        use CharKind::*;
        matches!(
            (left, right),
            (Space, Identifier)
                | (Space, Punctuation)
                | (Punctuation, Identifier)
                | (Identifier, Punctuation)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emtpy_is_empty() {
        let empty = TextBuffer::empty();
        dbg!(&empty);
        let strings = vec!["thiß german text", "is", "a", "string", "iter"];
        let with_lines = TextBuffer::with_lines(strings.iter());
        dbg!(with_lines.unwrap());
    }
}
