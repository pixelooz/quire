use anyhow::{Result, bail};
use unicode_width::UnicodeWidthChar;

/// Number of spaces a Tab takes.
const TAB_STOP: usize = 4;

/// Row is the pure data representation of a line of text.
#[derive(Debug, Default)]
pub struct Row {
    /// The raw utf-8 text buffer.
    buffer: String,
}

impl Row {
    /// Returns an initialized `Row` from the provided parameter.
    pub fn new<L: Into<String>>(line: L) -> Result<Self> {
        let buffer = line.into();
        for ch in buffer.chars() {
            if ch != '\t' && ch.is_control() {
                bail!("Control character in text: {}", ch);
            }
        }
        Ok(Self { buffer })
    }

    /// Returns the character count of the row.
    pub fn len(&self) -> usize {
        self.buffer.chars().count()
    }

    /// Returns the row's source text.
    pub fn buffer(&self) -> &str {
        self.buffer.as_str()
    }

    /// Translate a logical character index into a raw utf-8 byte index.
    pub fn char_to_byte_idx(&self, char_idx: usize) -> usize {
        self.buffer()
            .char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or_else(|| self.buffer.len())
    }

    /// Calculates the visual column (rendered x coordinate) dynamically.
    pub fn rcol_idx_from(&self, col_idx: usize) -> usize {
        let byte_end = self.char_to_byte_idx(col_idx);
        self.buffer[..byte_end]
            .chars()
            .fold(0, |rcol_idx, ch| {
                if ch == '\t' {
                    rcol_idx + TAB_STOP - (rcol_idx % TAB_STOP)
                } else {
                    rcol_idx + ch.width_cjk().unwrap_or(1)
                }
            })
    }
}
