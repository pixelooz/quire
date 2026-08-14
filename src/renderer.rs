use std::io::Write;

use anyhow::{Context, Result, bail};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    queue,
    style::Print,
    terminal::{Clear, ClearType},
};

use crate::terminal::Size;

/// Returns whether the window is small than 4x4
fn window_too_small(width: u16, height: u16) -> bool {
    width < 4 || height < 4
}

/// Stateless type responsible for painting the screen with content.
#[derive(Debug)]
pub struct Renderer<W: Write> {
    num_cols: u16,
    num_rows: u16,
    output: W,
}

impl<W: Write> Renderer<W> {
    /// Initializes the `Renderer` and queue the first cursor Hide command.
    pub fn new(size: Size, mut output: W) -> Result<Self> {
        let height = size.height;
        let width = size.width;

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
            num_cols: width.saturating_sub(2), // reserving the bottom 2 lines for status bar
            num_rows: height,
            output,
        })
    }

    /// Sets the `Renderer`'s num_cols and num_rows, given they are valid.
    pub fn resize(&mut self, size: Size) -> Result<()> {
        let height = size.height;
        let width = size.width;

        if window_too_small(width, height) {
            bail!(
                "Terminal window too small: {}x{} (minimum required is 4x4",
                width,
                height
            )
        }
        self.num_cols = width.saturating_sub(2); // reserving the bottom 2 lines for status bar
        self.num_rows = height;
        Ok(())
    }

    /// Flushes the in-memory canvas to the hardware in a singly syscall.
    fn write_flush(&mut self, bytes: &[u8]) -> Result<()> {
        self.output.write_all(bytes)?;
        self.output.flush()?;
        Ok(())
    }

    /// Renders the empty screen with a lot of Tildas(~).
    pub fn render_empty_screen(&mut self) -> Result<()> {
        let canvas_size = ((self.num_rows + 2) * self.num_cols) as usize;
        let mut canvas = Vec::with_capacity(canvas_size);

        queue!(canvas, Hide)?;

        for row in 0..self.num_rows {
            queue!(
                canvas,
                MoveTo(0, row),
                Print("~"),
                Clear(ClearType::UntilNewLine)
            )?;
        }
        queue!(canvas, MoveTo(0, 0), Show)?;
        self.write_flush(&canvas)
    }
}
