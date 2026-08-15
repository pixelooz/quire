use std::{
    fs::File,
    io::{self, BufRead},
    path::{Path, PathBuf},
    slice,
};

use anyhow::Result;

use crate::{lang::Language, row::Row};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorDir {
    Left,
    Right,
    Up,
    Down,
}

/// Contains both, the actual path sequence and name to display on the status bar.
#[derive(Debug)]
pub struct FilePath {
    pub path: PathBuf,
    pub display_path: String,
}

impl FilePath {
    /// Returns `FilePath` constructed from the given string or a type that implements
    /// `Into<String>`.
    fn from_string<T: Into<String>>(path: T) -> Self {
        let display_path = path.into();
        Self {
            path: PathBuf::from(&display_path),
            display_path,
        }
    }

    /// Returns `FilePath` constructed from the given path or a type that implements
    /// `AsRef<Path>`.
    ///
    /// `[FilePath::display]` is constructed using `path.to_string_lossy()`.
    fn from_path<P: AsRef<Path>>(path: P) -> Self {
        let path = path.as_ref();
        Self {
            path: path.into(),
            display_path: path.to_string_lossy().into(),
        }
    }
}

pub struct Line<'a>(slice::Iter<'a, Row>);

impl<'a> ExactSizeIterator for Line<'a> {}

impl<'a> Iterator for Line<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|x| x.buffer())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

/// The pure data model representing the open file.
#[derive(Debug)]
pub struct TextBuffer {
    /// Logical y-coordinate of the character.
    col_idx: usize,

    /// Logical x-coordinate of the character.
    row_idx: usize,

    /// The file path and the display name.
    file: Option<FilePath>,

    /// The underlying lines of text.
    rows: Vec<Row>,

    /// Language detected from the file extension.
    lang: Language,
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self {
            col_idx: 0,
            row_idx: 0,
            file: None,
            rows: vec![Row::default()],
            lang: Language::PlainText,
        }
    }
}

impl TextBuffer {
    /// Returns an empty `TextBuffer`.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Opens a file, reads it line by line into `Row`s, and detects the language.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let file = FilePath::from_path(path);

        let mut buffer = Self {
            lang: Language::detect(path),
            file: Some(file),
            ..Default::default()
        };
        if !path.try_exists()? {
            return Ok(buffer);
        }
        let reader = io::BufReader::new(File::open(path)?);
        let create_row = |line_result| {
            let mut line: String = line_result?;
            if line.ends_with('\r') {
                line.pop();
            }
            Row::new(line)
        };
        buffer.rows = reader
            .lines()
            .map(create_row)
            .collect::<Result<_>>()?;

        Ok(buffer)
    }

    pub fn col_idx(&self) -> usize {
        self.col_idx
    }

    pub fn row_idx(&self) -> usize {
        self.row_idx
    }

    /// Returns all the rows as a borrowed slice.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Returns either Some(row) or None if there are no rows.
    fn curr_row(&self) -> Option<&Row> {
        self.rows.get(self.row_idx)
    }

    /// Moves the logical cursor, handling bounds and snapping.
    pub fn step(&mut self, dir: CursorDir) {
        match dir {
            CursorDir::Right => {
                if self.row_idx < self.rows().len() {
                    let len = self.curr_row().map(|r| r.len()).unwrap_or(0);
                    if self.col_idx < len {
                        self.col_idx += 1;
                    } else if self.row_idx + 1 < self.rows().len() {
                        self.col_idx = 0;
                        self.row_idx += 1;
                    }
                }
            }
            CursorDir::Left => {
                if self.col_idx > 0 {
                    self.col_idx -= 1;
                } else if self.row_idx > 0 {
                    self.row_idx -= 1;
                    self.col_idx = self.curr_row().map(|r| r.len()).unwrap_or(0)
                }
            }
            CursorDir::Up => self.row_idx = self.row_idx.saturating_sub(1),
            CursorDir::Down => {
                if self.row_idx + 1 < self.rows().len() {
                    self.row_idx += 1;
                }
            }
        }
        self.curr_row()
            .map(|r| r.len())
            .unwrap_or(0)
            .lt(&self.col_idx)
            .then(|| self.col_idx = self.curr_row().unwrap().len());
    }
}
