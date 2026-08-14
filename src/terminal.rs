use std::io;

use anyhow::Result;
use crossterm::{
    cursor::{Hide, Show},
    event::{self, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

/// Key sequence pressed with control/alt or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySeq {
    pub ctrl: bool,
    pub key: Key,
    pub alt: bool,
}

/// Type of event received from terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Key(KeySeq),
    Resize { cols: u16, rows: u16 },
}

/// The kind of key-presses supported by the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Backspace,

    Left,
    Right,
    Up,
    Down,

    Home,
    End,

    PageUp,
    PageDown,

    Delete,
    Esc,
    Tab,
    Enter,

    Unknown,
}

/// Introduces the RAII pattern to enable/disable raw mode and adjacent features
/// for the editor.
///
/// # Implements
/// It implements [`Drop`] to restore the terminal to its original state when
/// the guard is dropped, so as to not leave the user with a broken terminal.
pub struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

impl TerminalGuard {
    /// Enables raw mode and executes commands to the terminal to enter alternate
    /// screen, so as to not erase users terminal history, and hides the cursor
    /// to prevent screen flickering.
    pub fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, Hide)?;
        Ok(Self)
    }
}

/// The current terminal dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

/// The layer that translates terminal events into editor events.
pub struct Terminal {
    _guard: TerminalGuard,
}

impl Terminal {
    /// Enters raw mode and initializes the terminal.
    pub fn new() -> Result<Self> {
        Ok(Self {
            _guard: TerminalGuard::enter()?,
        })
    }

    /// Returns the current terminal size.
    pub fn size() -> Result<Size> {
        let (width, height) = terminal::size()?;
        Ok(Size { width, height })
    }

    /// Blocks until the next supported terminal event is received.
    pub fn read_event(&self) -> Result<Event> {
        loop {
            let event = crossterm::event::read()?;
            match Self::map_event(event) {
                Some(event) => return Ok(event),
                None => continue,
            }
        }
    }

    fn map_event(event: event::Event) -> Option<Event> {
        match event {
            event::Event::Key(key) if key.kind == KeyEventKind::Press => {
                let key = Event::Key(Self::map_key_event(key));
                Some(key)
            }
            event::Event::Resize(cols, rows) => {
                let resize = Event::Resize { cols, rows };
                Some(resize)
            }
            _ => None,
        }
    }

    fn map_key_event(key_event: event::KeyEvent) -> KeySeq {
        let key = match key_event.code {
            KeyCode::Char(ch) => Key::Char(ch),
            KeyCode::Left => Key::Left,
            KeyCode::Right => Key::Right,
            KeyCode::Up => Key::Up,
            KeyCode::Down => Key::Down,

            KeyCode::Home => Key::Home,
            KeyCode::End => Key::End,

            KeyCode::PageUp => Key::PageUp,
            KeyCode::PageDown => Key::PageDown,

            KeyCode::Delete => Key::Delete,
            KeyCode::Esc => Key::Esc,
            KeyCode::Tab => Key::Tab,
            KeyCode::Enter => Key::Enter,

            _ => Key::Unknown,
        };
        KeySeq {
            ctrl: key_event
                .modifiers
                .contains(KeyModifiers::CONTROL),
            key,
            alt: key_event.modifiers.contains(KeyModifiers::ALT),
        }
    }
}
