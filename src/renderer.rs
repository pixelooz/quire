use std::{cmp, io::Write, time::SystemTime};

use anyhow::{Context, Result, bail};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal::{Clear, ClearType},
};
use unicode_width::UnicodeWidthChar;

use crate::{
    buffer::TextBuffer,
    color::{self, TextElement, Theme, ThemeElement},
    help::HELP,
    highlight::Highlighting,
    row::Row,
    status_bar::StatusBar,
    terminal::Size,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusMessageKind {
    Info,
    Error,
}

#[derive(Debug)]
pub struct StatusMessage {
    pub text: String,
    pub timestamp: SystemTime,
    pub kind: StatusMessageKind,
}

impl StatusMessage {
    pub fn new<T: Into<String>>(msg: T, kind: StatusMessageKind) -> Self {
        Self {
            text: msg.into(),
            timestamp: SystemTime::now(),
            kind,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawMessage {
    Open,
    Close,
    Update,
    DoNothing,
}

impl DrawMessage {
    pub fn merge(self, rhs: Self) -> Self {
        use DrawMessage::*;
        match (self, rhs) {
            (lhs, DoNothing) => lhs,
            (DoNothing, rhs) => rhs,
            (Update, Update) => Update,
            (Open, Update) => Open,
            (Update, Close) => Close,
            (Close, Open) => Update,
            (Open, Close) => DoNothing,
            (lhs, rhs) => {
                unreachable!("invalid DrawMessage transition: {:?} then {:?}", lhs, rhs)
            }
        }
    }
}

fn window_too_small(width: usize, height: usize) -> bool {
    width < 1 || height < 3
}

pub struct Renderer<W: Write> {
    /// The usable grid height in physical rows (height - 2).
    num_rows: usize,

    /// The usable grid width in physical columns.
    num_cols: usize,

    /// The underlying unbuffered output sink.
    output: W,

    /// The expanded x co-ordinate - what's rendered on screen.
    rcol_idx: usize,

    /// The active transient status message, if any.
    status_msg: Option<StatusMessage>,

    /// The current operational state of the message bar.
    draw_message: DrawMessage,

    /// The boundary from where the text is assumed to invalid.
    /// All rows from here to num_rows will be unconditionally
    /// repainted on the next frame.
    redraw_idx: Option<usize>,

    /// True if the cursor position moved during the frame tick.
    pub cursor_moved: bool,

    /* ! NOTE:
     *   1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16
     *   --------------------------------------
     * 1 |                                    |
     * 2 |                                    |
     * 3 |  ––––––––––––––––––––––––––––––    |
     * 4 |  |                            |    |
     * 5 |  |        (rowoff=3)          |    |
     * 5 |  |        (coloff=2)          |    |
     * 6 |  |         The view           |    |
     * 7 |  |                            |    |
     * 8 |  |                            |    |
     * 9 |  ––––––––––––––––––––––––––––––    |
     * 10|                                    |
     * 11--------------------------------------
     */
    /// The row aligned index at the top left corner of "viewport"
    /// from where we will paint the screen till the height of the
    /// terminal.
    pub rowoff: usize,

    /// The col aligned index at the top left corner of "viewport"
    /// from where we will paint the screen till the width of the
    /// terminal.
    pub coloff: usize,
}

impl<W: Write> Renderer<W> {
    pub fn new(size: Size, mut output: W) -> Result<Self> {
        let height = size.height as usize;
        let width = size.width as usize;

        if window_too_small(width, height) {
            bail!(
                "Terminal window too small: {}x{} (minimum required is 1x3",
                width,
                height
            )
        }
        // Hide the cursor immediately so that the cursor doesn't jump around
        // the screen chaotically.
        queue!(output, Hide)?;
        output
            .flush()
            .context("Failed to flush intial cursor hide sequence")?;

        Ok(Self {
            // The text grid is strictly 2 rows shorter than the window; to reserve
            // permanent space for the Status Bar (row N-1) and Message Bar (row N)
            num_rows: height.saturating_sub(2),
            num_cols: width,
            output,
            rcol_idx: 0,
            status_msg: None,
            draw_message: DrawMessage::DoNothing,
            redraw_idx: Some(0),
            cursor_moved: true,
            rowoff: 0,
            coloff: 0,
        })
    }

    fn set_msg(&mut self, status_msg: Option<StatusMessage>) {
        let rhs = match (&self.status_msg, &status_msg) {
            (Some(prev), Some(next)) if prev.text == next.text => DrawMessage::DoNothing,
            (Some(_), Some(_)) => DrawMessage::Update,
            (None, Some(_)) => DrawMessage::Open,
            (Some(_), None) => DrawMessage::Close,
            (None, None) => DrawMessage::DoNothing,
        };
        self.draw_message = self.draw_message.merge(rhs);
        self.status_msg = status_msg;
    }

    pub fn set_info_msg<T: Into<String>>(&mut self, msg: T) {
        use StatusMessageKind::*;
        self.set_msg(Some(StatusMessage::new(msg, Info)));
    }

    pub fn set_error_msg<T: Into<String>>(&mut self, msg: T) {
        use StatusMessageKind::*;
        self.set_msg(Some(StatusMessage::new(msg, Error)));
    }

    pub fn remove_msg(&mut self) {
        self.set_msg(None);
    }

    pub fn set_redraw_idx(&mut self, index: usize) {
        self.redraw_idx = Some(
            self.redraw_idx
                .map_or(index, |curr| curr.min(index)),
        )
    }

    pub fn rows(&self) -> usize {
        if self.status_msg.is_some() {
            self.num_rows
        } else {
            self.num_rows + 1
        }
    }

    pub fn cols(&self) -> usize {
        self.num_cols
    }

    pub fn message_text(&self) -> &str {
        self.status_msg
            .as_ref()
            .map(|m| m.text.as_str())
            .unwrap_or("")
    }

    pub fn force_set_cursor(&mut self, col: usize, row: usize) -> Result<()> {
        queue!(self.output, MoveTo(col as u16, row as u16), Show)?;
        self.output.flush()?;
        Ok(())
    }

    pub fn render_smoke_test(&mut self) -> Result<()> {
        let mut canvas = Vec::with_capacity((self.num_rows + 2) * self.num_cols);

        queue!(
            canvas,
            Hide,
            Clear(ClearType::All),
            MoveTo(0, 0),
            Print("Quill Viewport Architecture: Active"),
            MoveTo(0, 1),
            Print(format!(
                "Usable text grid: {} cols x {} rows",
                self.num_cols, self.num_rows
            )),
            MoveTo(0, 2),
            Print("Hardware buffer safely bound. Ready for Layer 2..."),
            Show
        )?;
        self.write_flush(&canvas)
    }
}

impl<W: Write> Renderer<W> {
    pub fn resize(&mut self, size: Size) -> Result<()> {
        let height = size.height as usize;
        let width = size.width as usize;

        if window_too_small(width, height) {
            bail!(
                "Terminal resized below minimum viable bounds: {}x{}",
                width,
                height
            )
        }
        self.num_rows = height.saturating_sub(2);
        self.num_cols = width;
        Ok(())
    }

    fn write_flush(&mut self, bytes: &[u8]) -> Result<()> {
        self.output.write_all(bytes)?;
        self.output.flush()?;
        Ok(())
    }

    fn update_message_bar(&mut self) -> Result<()> {
        if let Some(sm) = &self.status_msg
            && let Ok(elapsed) = sm.timestamp.elapsed()
            && elapsed.as_secs() > 5
        {
            self.remove_msg();
        }
        if self.draw_message == DrawMessage::Close {
            // Closing the message bar reveals one more line of text at the bottom
            // of the grid.
            self.set_redraw_idx(self.num_rows);
        }
        Ok(())
    }

    fn trim_visual(text: &str, max_width: usize) -> (&str, usize) {
        let mut curr_width = 0;
        for (byte_idx, ch) in text.char_indices() {
            let ch_width = ch.width_cjk().unwrap_or(1);
            if curr_width + ch_width > max_width {
                return (&text[..byte_idx], curr_width);
            }
            curr_width += ch_width;
        }
        (text, curr_width)
    }

    pub fn draw_message_bar<B: Write>(&self, mut buf: B, msg: &StatusMessage) -> Result<()> {
        let (trimmed, _) = Self::trim_visual(&msg.text, self.num_cols);

        queue!(buf, MoveTo(0, self.num_rows as u16 + 1))?;
        if msg.kind == StatusMessageKind::Error {
            queue!(buf, SetBackgroundColor(Color::Red))?;
        }
        queue!(
            buf,
            Print(trimmed),
            ResetColor,
            Clear(ClearType::UntilNewLine)
        )?;
        Ok(())
    }

    pub fn draw_status_bar<B: Write>(&self, buf: &mut B, status_bar: &StatusBar) -> Result<()> {
        let right = status_bar.right();
        let left = status_bar.left();

        let (left_str, llen) = Self::trim_visual(&left, self.num_cols);

        queue!(
            buf,
            MoveTo(0, self.num_rows as u16),
            SetAttribute(Attribute::Reverse),
            Print(left_str)
        )?;
        let rem_cols = self.num_cols.saturating_sub(llen);

        let (right_str, rlen) = Self::trim_visual(&right, rem_cols);
        let padding = rem_cols.saturating_sub(rlen);

        if padding > 0 {
            queue!(buf, Print(" ".repeat(padding)))?;
        }
        queue!(
            buf,
            Print(right_str),
            SetAttribute(Attribute::Reset),
            Clear(ClearType::UntilNewLine),
        )?;
        Ok(())
    }

    pub fn render_welcome<B: Write>(&self, writer: &mut B) -> Result<()> {
        let welcome = "Quill Editor -- Version 0.1";

        let (_, msg_len) = Self::trim_visual(welcome, self.num_cols);
        for row in 0..self.num_rows {
            queue!(writer, MoveTo(0, row as u16))?;
            // Draw the welcome message approximately 1/3rd down the screen.
            if row == self.num_rows / 3 {
                let padding = self.num_cols.saturating_sub(msg_len) / 2;
                if padding > 0 {
                    queue!(
                        writer,
                        Print("~"),
                        Print(" ".repeat(padding.saturating_sub(1))),
                        Print(welcome)
                    )?;
                } else {
                    // This means terminal is extremely narrow and can't even
                    // fit the message, trim the message to fit.
                    let (trimmed, _) = Self::trim_visual(welcome, self.num_cols.saturating_sub(1));
                    queue!(writer, Print("~"), Print(trimmed))?;
                }
            } else {
                // Draw the standard empty-row markers "~".
                queue!(writer, Print("~"))?;
            }
            queue!(writer, Clear(ClearType::UntilNewLine))?;
        }
        Ok(())
    }

    pub fn render_help(&mut self) -> Result<()> {
        let help_lines: Vec<_> = HELP
            .split('\n')
            .skip_while(|s| !s.contains(':'))
            .map(|s| s.trim_start())
            .collect();

        let vertical_margin = if help_lines.len() < self.rows() {
            (self.rows() - help_lines.len()) / 2
        } else {
            0
        };
        let help_max_width = help_lines
            .iter()
            .map(|line| line.len())
            .max()
            .unwrap_or(0);

        let horizontal_margin = if help_max_width < self.num_cols {
            (self.num_cols - help_max_width) / 2
        } else {
            0
        };
        let mut canvas = Vec::with_capacity(self.rows() * self.num_cols);

        queue!(canvas, Hide)?;
        // Pad the top
        for row_idx in 0..vertical_margin {
            queue!(
                canvas,
                MoveTo(0, row_idx as u16),
                Clear(ClearType::UntilNewLine)
            )?;
        }
        let left_pad = " ".repeat(horizontal_margin);

        let help_height = cmp::min(vertical_margin + help_lines.len(), self.rows());

        // Draw the help text with syntax highlighting for the hotkeys
        for row_idx in vertical_margin..help_height {
            let idx = row_idx - vertical_margin;
            queue!(canvas, MoveTo(0, row_idx as u16), Print(&left_pad))?;

            let curr_line = help_lines[idx];
            let display_line = &curr_line[..cmp::min(
                curr_line.len(),
                self.num_cols.saturating_sub(horizontal_margin),
            )];
            let mut parts = display_line.split(':');

            // Print the keybinding in cyan.
            if let Some(key_part) = parts.next() {
                queue!(canvas, SetForegroundColor(Color::Cyan), Print(key_part))?;
            }
            queue!(canvas, ResetColor)?;

            // Print the description in default text color.
            if let Some(desc_part) = parts.next() {
                queue!(canvas, Print(":"), Print(desc_part))?;
            }
            queue!(canvas, Clear(ClearType::UntilNewLine))?;
        }
        // Pad the bottom
        for row_idx in help_height..self.rows() {
            queue!(
                canvas,
                MoveTo(0, row_idx as u16),
                Clear(ClearType::UntilNewLine)
            )?;
        }
        self.write_flush(&canvas)?;
        Ok(())
    }

    fn draw_rows<B: Write>(
        &self,
        writer: &mut B,
        redraw_idx: usize,
        rows: &[Row],
        highlight: &Highlighting,
    ) -> Result<()> {
        // We use saturating_sub so if the dirty state is above the camera, we
        // ignore everything above screen row = 0.
        let redraw_idx = redraw_idx.saturating_sub(self.rowoff);
        queue!(writer, ResetColor)?;

        for screen_row in redraw_idx..self.num_rows {
            let buffer_row = self.rowoff + screen_row;
            queue!(writer, MoveTo(0, screen_row as u16))?;

            // If we are past the end of the actual text file.
            if buffer_row >= rows.len() {
                queue!(writer, Print("~"), Clear(ClearType::UntilNewLine))?;
                continue;
            }
            let base_spans = highlight.lines(buffer_row).unwrap_or(&[]);

            let spans = highlight.apply_overlays(buffer_row, base_spans);
            let row = &rows[buffer_row];

            let mut span_idx = 0;
            let mut span_len = spans.first().map_or(0, |hs| hs.len);

            let mut prev_style: Option<color::Style> = None;

            let mut visual_col = 0;
            let mut drawn_cols = 0;

            // loop responsible for printing and highlighting a single line left
            // to right.
            for (byte_idx, ch) in row.render().char_indices() {
                // Advance the color span if our current byte index has crossed
                // the boundary.
                while byte_idx >= span_len && span_idx < spans.len() {
                    span_idx += 1;
                    if span_idx < spans.len() {
                        span_len += spans[span_idx].len;
                    }
                }
                let ch_width = ch.width_cjk().unwrap_or(1);

                // cursor outside of viewport on the left side.
                if visual_col < self.coloff {
                    visual_col += ch_width;
                    continue;
                }
                if drawn_cols + ch_width > self.num_cols {
                    break;
                }
                let style = match spans.get(span_idx) {
                    Some(span) => {
                        let element = match span.overlay {
                            Some(ui) => ThemeElement::Ui(ui),
                            None => ThemeElement::Text(span.highlight),
                        };
                        Theme::color_for(element)
                    }
                    None => Theme::color_for(ThemeElement::Text(TextElement::Normal)),
                };
                if Some(style) != prev_style {
                    if prev_style.is_some_and(|s| s.bg.is_some()) && style.bg.is_none() {
                        queue!(writer, ResetColor)?;
                    }
                    queue!(writer, SetForegroundColor(style.fg))?;
                    if let Some(bg) = style.bg {
                        queue!(writer, SetBackgroundColor(bg))?;
                    }
                    prev_style = Some(style)
                }
                queue!(writer, Print(ch))?;

                visual_col += ch_width;
                drawn_cols += ch_width;
            }
            queue!(writer, ResetColor, Clear(ClearType::UntilNewLine))?;
        }
        Ok(())
    }

    fn redraw(
        &mut self,
        buffer: &TextBuffer,
        highlight: &Highlighting,
        status_bar: &StatusBar,
    ) -> Result<()> {
        let mut canvas = Vec::with_capacity((self.num_rows + 2) * self.num_cols);

        // Hide the cursor during painting the canvas.
        queue!(canvas, Hide)?;

        if buffer.is_scratch() && self.redraw_idx == Some(0) {
            self.render_welcome(&mut canvas)?;
        } else if let Some(redraw_idx) = self.redraw_idx {
            self.draw_rows(&mut canvas, redraw_idx, buffer.rows(), highlight)?;
        }
        if status_bar.redraw || self.redraw_idx.is_some() {
            self.draw_status_bar(&mut canvas, status_bar)?;
        }
        if (self.draw_message != DrawMessage::DoNothing || self.redraw_idx.is_some())
            && let Some(msg) = &self.status_msg
        {
            self.draw_message_bar(&mut canvas, msg)?;
        }
        let col_idx = buffer.col_idx();
        let row_idx = buffer.row_idx();

        let rcol_idx = if row_idx < buffer.rows().len() {
            buffer.rows()[row_idx].rcol_idx_from(col_idx)
        } else {
            0
        };
        let screen_col = rcol_idx.saturating_sub(self.coloff) as u16;
        let screen_row = row_idx.saturating_sub(self.rowoff) as u16;

        queue!(canvas, MoveTo(screen_col, screen_row), Show)?;
        self.write_flush(&canvas)
    }

    fn after_render(&mut self) {
        self.redraw_idx = None;
        self.cursor_moved = false;
        self.draw_message = DrawMessage::DoNothing;
    }

    fn do_scroll(&mut self, buffer: &TextBuffer) {
        let col_idx = buffer.col_idx();
        let row_idx = buffer.row_idx();

        let mut camera_moved = false;

        // Shift the viewport when cursor is outside screen.
        if row_idx < self.rowoff {
            self.rowoff = row_idx;
            camera_moved = true;
        }
        if row_idx >= self.rowoff + self.num_rows {
            self.rowoff = row_idx - self.num_rows + 1;
            camera_moved = true;
        }
        let rcol_idx = if row_idx < buffer.rows().len() {
            buffer.rows()[row_idx].rcol_idx_from(col_idx)
        } else {
            0
        };
        self.rcol_idx = rcol_idx;

        if self.rcol_idx < self.coloff {
            self.coloff = self.rcol_idx;
            camera_moved = true;
        }
        if self.rcol_idx >= self.coloff + self.num_cols {
            self.coloff = self.rcol_idx - self.num_cols + 1;
            camera_moved = true;
        }
        if camera_moved {
            self.set_redraw_idx(self.rowoff);
        }
    }

    pub fn render(
        &mut self,
        buffer: &TextBuffer,
        highlight: &mut Highlighting,
        status_bar: &StatusBar,
    ) -> Result<()> {
        self.do_scroll(buffer);
        self.update_message_bar()?;

        highlight.update(buffer.rows(), self.rowoff + self.num_rows);

        self.redraw(buffer, highlight, status_bar)?;
        self.after_render();
        Ok(())
    }
}
