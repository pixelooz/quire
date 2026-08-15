use std::{io::Write, path::Path};

use anyhow::Result;

use crate::{
    buffer::{CursorDir, TextBuffer},
    renderer::Renderer,
    terminal::{Event, Key, KeySeq, Size},
};

#[derive(Debug)]
/// Represents an open file.
pub struct Document {
    pub buffer: TextBuffer,
}

impl Document {
    pub fn new(buffer: TextBuffer) -> Self {
        Self { buffer }
    }
}

/// The orchestrator of the application loop.
#[derive(Debug)]
pub struct Editor<I, W>
where
    W: Write,
    I: Iterator<Item = Event>,
{
    renderer: Renderer<W>,
    document: Document,
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
        let buffer = TextBuffer::empty();
        Ok(Self {
            document: Document::new(buffer),
            renderer,
            input,
        })
    }

    /// Initializes the editor with an opened file.
    pub fn open<P: AsRef<Path>>(input: I, output: W, size: Size, path: P) -> Result<Self> {
        let renderer = Renderer::new(size, output)?;
        let buffer = TextBuffer::open(path)?;
        Ok(Self {
            document: Document::new(buffer),
            renderer,
            input,
        })
    }

    /// The main run loop.
    pub fn edit(&mut self) -> Result<()> {
        self.renderer.render(&self.document.buffer)?;
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
                self.renderer.render(&self.document.buffer)?;
                Ok(true)
            }
            Event::Key(key_seq) => {
                let keep_running = self.process_keypress(key_seq)?;
                if keep_running {
                    self.renderer.render(&self.document.buffer)?;
                }
                Ok(keep_running)
            }
        }
    }

    fn process_keypress(&mut self, seq: KeySeq) -> Result<bool> {
        match (seq.key, seq.ctrl) {
            (Key::Left, false) => self.document.buffer.step(CursorDir::Left),
            (Key::Right, false) => self.document.buffer.step(CursorDir::Right),
            (Key::Up, false) => self.document.buffer.step(CursorDir::Up),
            (Key::Down, false) => self.document.buffer.step(CursorDir::Down),
            (Key::Char('q'), true) => return Ok(false),
            _ => {}
        }
        Ok(true)
    }
}
