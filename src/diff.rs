use crate::row::Row;

/// The cursor's position within the text buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    pub col_idx: usize,
    pub row_idx: usize,
}

impl CursorPosition {
    pub fn new(col: usize, row: usize) -> Self {
        Self {
            col_idx: col,
            row_idx: row,
        }
    }
}

/// A reversible edit operation recorded in the undo history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditDiff {
    InsertChar { at: CursorPosition, ch: char },
    DeleteChar { at: CursorPosition, ch: char },
    Insert { at: CursorPosition, text: String },
    Remove { at: CursorPosition, text: String },
    Append { row: usize, text: String },
    Truncate { row: usize, removed: String },
    InsertLine { row: usize, text: String },
    DeleteLine { row: usize, text: String },
}

impl EditDiff {
    /// Applies the edit and returns the resulting cursor position.
    pub fn apply(&self, rows: &mut Vec<Row>) -> CursorPosition {
        use CursorPosition as cp;
        use EditDiff::*;

        match self {
            InsertChar { at, ch } => {
                rows[at.row_idx].insert_char(at.col_idx, *ch);
                cp::new(at.col_idx + 1, at.row_idx)
            }
            DeleteChar { at, .. } => {
                rows[at.row_idx].delete_char(at.col_idx);
                cp::new(at.col_idx, at.row_idx)
            }
            Insert { at, text } => {
                rows[at.row_idx].insert_str(at.col_idx, text);
                let len = text.chars().count();
                cp::new(at.col_idx + len, at.row_idx)
            }
            /*  NOTE: [Remove] has been changed a few times here and there, so if we see
            any panics due to cursor calculation during undo/redo, check here.*/
            Remove { at, text } => {
                let end_x = at.col_idx + text.chars().count();
                rows[at.row_idx].remove(at.col_idx, end_x);
                cp::new(at.col_idx, at.row_idx)
            }
            Append { row, text } => {
                let len = rows[*row].len();
                rows[*row].append(text);
                cp::new(len, *row)
            }
            Truncate { row, removed } => {
                let count = removed.chars().count();
                let len = rows[*row].len();
                rows[*row].truncate(len - count);
                cp::new(len - count, *row)
            }
            InsertLine { row, text } => {
                rows.insert(
                    *row,
                    Row::new(text).expect("creating a new row should've succeded"),
                );
                cp::new(0, *row)
            }
            DeleteLine { row, .. } => {
                if *row == rows.len() - 1 {
                    rows.pop();
                } else {
                    rows.remove(*row);
                }
                if *row == 0 {
                    cp::new(0, 0)
                } else {
                    cp::new(rows[*row - 1].len(), *row - 1)
                }
            }
        }
    }

    /* FIXME: Remove cloning behaviour of the function; can be optimized by the
    function taking a mutable vec of rows and owning the operation like apply.
    Then we have undo redo functions instead of apply and inverse. */

    /// Returns the inverse of this edit.
    pub fn inverse(&self) -> Self {
        use EditDiff::*;
        match *self {
            InsertChar { at, ch } => DeleteChar { at, ch },
            DeleteChar { at, ch } => InsertChar { at, ch },
            Insert { at, ref text } => Remove {
                at,
                text: text.clone(),
            },
            Remove { at, ref text } => Insert {
                at,
                text: text.clone(),
            },
            Append { row, ref text } => Truncate {
                row,
                removed: text.clone(),
            },
            Truncate { row, ref removed } => Append {
                row,
                text: removed.clone(),
            },
            InsertLine { row, ref text } => DeleteLine {
                row,
                text: text.clone(),
            },
            DeleteLine { row, ref text } => InsertLine {
                row,
                text: text.clone(),
            },
        }
    }
}
