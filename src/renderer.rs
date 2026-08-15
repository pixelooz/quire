use std::io::Write;

use anyhow::{Context, Result, bail};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    queue,
    style::Print,
    terminal::{Clear, ClearType},
};
use unicode_width::UnicodeWidthChar;

use crate::{buffer::TextBuffer, row::Row, terminal::Size};

/// Returns whether the window is small than 4x4
fn window_too_small(width: usize, height: usize) -> bool {
    width < 4 || height < 4
}

/// Stateless type responsible for painting the screen with content.
#[derive(Debug)]
pub struct Renderer<W: Write> {
    /// Number of columns we have on the terminal.
    num_cols: usize,

    /// Number of rows we have on the terminal.
    num_rows: usize,

    output: W,

    /// The top-left Y coordinate of the viewport.
    pub rowoff: usize,

    /// The top-left X coordinate of the viewport.
    pub coloff: usize,

    /// The expanded visual X coordinate of the cursor.
    pub rcol_idx: usize,
}

impl<W: Write> Renderer<W> {
    /// Initializes the `Renderer` and queue the first cursor Hide command.
    pub fn new(size: Size, mut output: W) -> Result<Self> {
        let height = size.height as usize;
        let width = size.width as usize;

        if window_too_small(width, height) {
            bail!(
                "Terminal window too small: {}x{} (minimum required is 4x4",
                width,
                height
            )
        }
        queue!(output, Hide)?;
        output
            .flush()
            .context("Failed to flush initial cursor hide sequence")?;
        Ok(Self {
            num_rows: height.saturating_sub(2),
            num_cols: width.saturating_sub(3),
            output,
            rowoff: 0,
            coloff: 0,
            rcol_idx: 0,
        })
    }

    /// Sets the `Renderer`'s num_cols and num_rows, given they are valid.
    pub fn resize(&mut self, size: Size) -> Result<()> {
        let height = size.height as usize;
        let width = size.width as usize;

        if window_too_small(width, height) {
            bail!(
                "Terminal window too small: {}x{} (minimum required is 4x4",
                width,
                height
            )
        }
        self.num_cols = width;
        self.num_rows = height.saturating_sub(2); // reserving the bottom 2 lines for status bar;
        Ok(())
    }

    /// Flushes the in-memory canvas to the hardware in a singly syscall.
    fn write_flush(&mut self, bytes: &[u8]) -> Result<()> {
        self.output.write_all(bytes)?;
        self.output.flush()?;
        Ok(())
    }

    /// Adjusts the viewport camera (`rowoff`, `coloff`) to ensure the cursor is visible.
    fn do_scroll(&mut self, buffer: &TextBuffer) {
        let col_idx = buffer.col_idx();
        let row_idx = buffer.row_idx();

        if row_idx < self.rowoff {
            self.rowoff = row_idx;
        }
        if row_idx >= self.rowoff + self.num_rows {
            self.rowoff = row_idx - self.num_rows + 1;
        }
        self.rcol_idx = if row_idx < buffer.rows().len() {
            buffer.rows()[row_idx].rcol_idx_from(col_idx)
        } else {
            0
        };
        if self.rcol_idx < self.coloff {
            self.coloff = self.rcol_idx;
        }
        if self.rcol_idx >= self.coloff + self.num_cols {
            self.coloff = self.rcol_idx - self.num_cols + 1;
        }
    }

    /// Draws the rows after an event has occurred.
    fn draw_rows(&self, writer: &mut Vec<u8>, rows: &[Row]) -> Result<()> {
        for (idx, screen_row) in (0..self.num_rows).enumerate() {
            let buffer_row = self.rowoff + screen_row;
            queue!(writer, MoveTo(0, screen_row as u16))?;

            if buffer_row >= rows.len() {
                queue!(writer, Print("~"), Clear(ClearType::UntilNewLine))?;
                continue;
            }
            let row = &rows[buffer_row];

            let mut screen_start = 0;
            let mut screen_drawn = 0;

            let line_num = self.rowoff + idx + 1;
            let width = line_num.ilog10();
            let spaces = 4 - width as usize;

            queue!(writer, Print(format!("{}{}", line_num, " ".repeat(spaces))))?;
            for ch in row.buffer().chars() {
                let (ch_width, print_str) = if ch == '\t' {
                    let spaces = 4 - (screen_start % 4);
                    (spaces, &"    "[..spaces])
                } else {
                    (ch.width_cjk().unwrap_or(1), "")
                };
                if screen_start + ch_width <= self.coloff {
                    screen_start += ch_width;
                    continue;
                }
                if screen_drawn + ch_width > self.num_cols {
                    break;
                }
                if ch == '\t' {
                    queue!(writer, Print(print_str))?;
                } else {
                    queue!(writer, Print(ch))?;
                }
                screen_start += ch_width;
                screen_drawn += ch_width;
            }
            queue!(writer, Clear(ClearType::UntilNewLine))?;
        }
        Ok(())
    }

    /// Orchestrates the camera tracking, drawing, and placing the terminal cursor.
    pub fn render(&mut self, buffer: &TextBuffer) -> Result<()> {
        self.do_scroll(buffer);

        let canvas_size = (self.num_rows + 2) * self.num_cols;
        let mut canvas = Vec::with_capacity(canvas_size);

        queue!(canvas, Hide)?;
        self.draw_rows(&mut canvas, buffer.rows())?;

        /* TODO: These numbers are very brittle, define some constant or a better
        way to showcase the calculation. */
        let mut screen_col = self.rcol_idx.saturating_sub(self.coloff) as u16 + 3;
        if self.rcol_idx > 4 {
            screen_col = self.rcol_idx.saturating_sub(self.coloff) as u16 + 2;
        }
        let screen_row = buffer.row_idx().saturating_sub(self.rowoff) as u16;

        queue!(canvas, MoveTo(screen_col, screen_row), Show)?;
        self.write_flush(&canvas)
    }

    /// Renders the empty screen with a lot of Tildas(~).
    pub fn render_empty_screen(&mut self) -> Result<()> {
        let canvas_size = (self.num_rows + 2) * self.num_cols;
        let mut canvas = Vec::with_capacity(canvas_size);

        queue!(canvas, Hide)?;

        for row in 0..self.num_rows {
            queue!(
                canvas,
                MoveTo(0, row as u16),
                Print("~"),
                Clear(ClearType::UntilNewLine)
            )?;
        }
        queue!(canvas, MoveTo(0, 0), Show)?;
        self.write_flush(&canvas)
    }
}
