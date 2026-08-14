use std::io::Write;

use anyhow::Result;

use crate::{
    renderer::Renderer,
    terminal::{Event, Key, KeySeq, Size},
};

/// The orchestrator of the application loop.
#[derive(Debug)]
pub struct Editor<I, W>
where
    W: Write,
    I: Iterator<Item = Event>,
{
    renderer: Renderer<W>,
    input: I,
}

impl<I, W> Editor<I, W>
where
    W: Write,
    I: Iterator<Item = Event>,
{
    /// Initializes the Renderer with the given output type and size and constructs
    /// the Editor with that and input iterator.
    pub fn new(input: I, output: W, size: Size) -> Result<Self> {
        let renderer = Renderer::new(size, output)?;
        Ok(Self { renderer, input })
    }

    /// The main run loop.
    pub fn edit(&mut self) -> Result<()> {
        self.renderer.render_empty_screen()?;
        loop {
            if !self.step()? {
                break;
            }
        }
        Ok(())
    }

    fn step(&mut self) -> Result<bool> {
        let Some(event) = self.input.next() else {
            return Ok(false);
        };
        match event {
            Event::Resize { cols, rows } => {
                self.renderer.resize(Size {
                    width: cols,
                    height: rows,
                })?;
                self.renderer.render_empty_screen()?;
                Ok(true)
            }
            Event::Key(key_seq) => {
                let keep_running = self.process_keypress(key_seq)?;
                if keep_running {
                    self.renderer.render_empty_screen()?;
                }
                Ok(keep_running)
            }
        }
    }

    fn process_keypress(&mut self, seq: KeySeq) -> Result<bool> {
        match (seq.key, seq.ctrl) {
            (Key::Char('q'), true) => Ok(false),
            _ => Ok(true),
        }
    }
}
