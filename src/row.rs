use anyhow::{Result, bail};
use std::{
    mem,
    ops::{Index, Range, RangeFrom, RangeInclusive, RangeTo, RangeToInclusive},
};
use unicode_width::UnicodeWidthChar;

/// Number of spaces a tab takes.
const TAB_STOP: usize = 4;

/// Row is the line representation of what a String would look like. It is a different
/// datatype as we need more than just text for proper handling of data, colors, etc.
#[derive(Debug, Default)]
pub struct Row {
    /// This is the raw text. Everything else derives from it.
    buffer: String,

    /// Stores the rendered information. "Hello\tWorld" -> "Hello    World"
    render: String,

    /// Cache of byte indices of characters in `buf`. This will be empty when `buf`
    /// only contains single byte characters so as to not allocate memory.
    indices: Vec<usize>,
}

impl Row {
    /// Returns an empty `Row`.
    pub fn empty() -> Self {
        Self {
            buffer: String::new(),
            render: String::new(),
            indices: Vec::with_capacity(0),
        }
    }

    /// Returns an initialized `Row` from the provided parameter [line] as long
    /// as [line] can be converted into a string using `into()`.
    pub fn new<T: Into<String>>(line: T) -> Result<Self> {
        let mut row = Self {
            buffer: line.into(),
            render: String::new(),
            indices: Vec::with_capacity(0),
        };
        // processes the given line and update the render and indices fields if
        // necessary.
        row.update_render()?;
        Ok(row)
    }

    /// Returns the length of the `Row` indices unless, there are no multibyte
    /// characters in which case indices will be empty, so buffer length will
    /// be returned.
    pub fn len(&self) -> usize {
        if self.indices.is_empty() {
            self.buffer.len()
        } else {
            self.indices.len()
        }
    }

    /// Returns the byte index of the character
    pub fn char_to_byte_idx(&self, char_idx: usize) -> usize {
        if self.indices.is_empty() {
            char_idx
        } else if char_idx == self.indices.len() {
            self.buffer.len()
        } else {
            self.indices[char_idx]
        }
    }

    /// Converts a UTF-8 byte index to a character index.
    ///
    /// `byte_idx` must be on a valid UTF-8 character boundary.
    pub fn byte_to_char_idx(&self, byte_idx: usize) -> usize {
        if self.indices.is_empty() {
            return byte_idx;
        }
        if self.buffer.len() == byte_idx {
            return self.indices.len(); // pointing after the last character in the vec.
        }
        /* TODO: could be optimized to O(1) by storing the byte indices in cache
        as well but introduces more maintenence for little gain.
        Will do later if needed. */
        self.indices
            .iter()
            .position(|&idx| idx == byte_idx)
            .expect("byte index is not at the correct boundary of UTF-8")
    }

    /// Processes the buffer and renders the ASCII such as '\t' '\n' accordingly
    /// and stores them in `Row.render`.
    /// If there are any multi-byte characters, their indices are stored as
    /// cache so as to avoid lookup time in buffer or render again.
    fn update_render(&mut self) -> Result<()> {
        self.render.clear();
        self.render.reserve(self.buffer.len());

        let mut index = 0;
        let mut num_chars = 0;

        for ch in self.buffer.chars() {
            if let Some(width) = ch.width_cjk() {
                index += width;
                self.render.push(ch);
            } else if ch == '\t' {
                loop {
                    self.render.push(' ');
                    index += 1;
                    if index % TAB_STOP == 0 {
                        break;
                    }
                }
            } else {
                // Control characters are valid UTF-8 but they should not appear
                // in text and we won't be handling them.
                bail!("Control character in text: {}", ch);
            }
            num_chars += 1;
        }
        if num_chars == self.buffer.len() {
            // If number of chars is the same as byte length, this line includes
            // no multi-byte character. Hence, no memory is allocated to the
            // heap.
            self.indices = Vec::with_capacity(0);
        } else {
            self.indices.clear();
            self.indices.reserve(num_chars);
            for (idx, _) in self.buffer.char_indices() {
                self.indices.push(idx);
            }
        }
        Ok(())
    }

    /// Returns the row's source text.
    pub fn buffer(&self) -> &str {
        self.buffer.as_str()
    }

    /// Returns the row's rendered text.
    pub fn render(&self) -> &str {
        self.render.as_str()
    }

    /// Extracts the owned text buffer, leaving an empty string in its place.
    ///
    /// This method is used to steal the underlying heap allocation without
    /// consuming the `Row` itself. It is particularly useful during deletion
    /// or concatenation operations where the `Row` is about to be discarded,
    /// allowing us to reuse the memory and avoid an expensive `.clone()` or
    /// `.to_owned()` call.
    ///
    /// After calling this, the `Row`'s text buffer will be empty.
    pub fn take_buffer(&mut self) -> String {
        mem::take(&mut self.buffer)
    }

    /// Returns the character at the provided index.
    ///
    /// # Panic
    /// If the character is not found at the provided index.
    pub fn char_at(&self, idx: usize) -> char {
        self.chat_at_checked(idx)
            .expect("character should have been present at the provided index")
    }

    /// Returns an Option with the character at the provided index.
    pub fn chat_at_checked(&self, idx: usize) -> Option<char> {
        // if it doesn't work properly check the trait implementations.
        self[idx..].chars().next()
    }

    /// Returns the rendered column corresponding to the raw `col_idx`.
    pub fn rcol_idx_from(&self, col_idx: usize) -> usize {
        self[..col_idx].chars().fold(0, |rcol_idx, ch| {
            if ch == '\t' {
                rcol_idx + TAB_STOP - (rcol_idx % TAB_STOP)
            } else {
                rcol_idx + ch.width_cjk().unwrap()
            }
        })
    }

    /// Inserts the given character at the given index.
    pub fn insert_char(&mut self, idx: usize, ch: char) {
        if self.len() <= idx {
            self.buffer.push(ch);
        } else {
            let b_idx = self.char_to_byte_idx(idx);
            self.buffer.insert(b_idx, ch);
        }
        self.update_render().unwrap();
    }

    /// Inserts the given str at the given index.
    pub fn insert_str<T: AsRef<str>>(&mut self, idx: usize, strs: T) {
        if self.len() <= idx {
            self.buffer.push_str(strs.as_ref());
        } else {
            let b_idx = self.char_to_byte_idx(idx);
            self.buffer.insert_str(b_idx, strs.as_ref());
        }
        self.update_render().unwrap();
    }

    /// Deletes a character at the given index.
    pub fn delete_char(&mut self, idx: usize) {
        if idx < self.len() {
            let b_idx = self.char_to_byte_idx(idx);
            self.buffer.remove(b_idx);
            self.update_render().unwrap();
        }
    }

    /// Appends the given string to the buffer and updates the renderer.
    pub fn append<T: AsRef<str>>(&mut self, strs: T) {
        let strs = strs.as_ref();
        if strs.is_empty() {
            return;
        }
        self.buffer.push_str(strs);
        self.update_render().unwrap();
    }

    /// Truncates the items till the provided index.
    pub fn truncate(&mut self, idx: usize) {
        if idx < self.len() {
            let b_idx = self.char_to_byte_idx(idx);
            self.buffer.truncate(b_idx);
            self.update_render().unwrap();
        }
    }

    /// Removes items from the buffer within the specified indices.
    pub fn remove(&mut self, start: usize, end: usize) {
        if start < end {
            let b_start = self.char_to_byte_idx(start);
            let b_end = self.char_to_byte_idx(end);
            self.buffer.drain(b_start..b_end);
            self.update_render().unwrap();
        }
    }
}

// Implementation of the Index trait with every Range type.

impl Index<Range<usize>> for Row {
    type Output = str;

    fn index(&self, r: Range<usize>) -> &Self::Output {
        let start = self.char_to_byte_idx(r.start);
        let end = self.char_to_byte_idx(r.end);
        &self.buffer[start..end]
    }
}

impl Index<RangeFrom<usize>> for Row {
    type Output = str;

    fn index(&self, r: RangeFrom<usize>) -> &Self::Output {
        let start = self.char_to_byte_idx(r.start);
        &self.buffer[start..]
    }
}

impl Index<RangeTo<usize>> for Row {
    type Output = str;

    fn index(&self, r: RangeTo<usize>) -> &Self::Output {
        let end = self.char_to_byte_idx(r.end);
        &self.buffer[..end]
    }
}

impl Index<RangeInclusive<usize>> for Row {
    type Output = str;

    fn index(&self, r: RangeInclusive<usize>) -> &Self::Output {
        let start = self.char_to_byte_idx(*r.start());
        let end = self.char_to_byte_idx(*r.end());
        &self.buffer[start..=end]
    }
}

impl Index<RangeToInclusive<usize>> for Row {
    type Output = str;

    fn index(&self, r: RangeToInclusive<usize>) -> &Self::Output {
        let end = self.char_to_byte_idx(r.end);
        &self.buffer[..=end]
    }
}
