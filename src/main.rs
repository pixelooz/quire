#![allow(dead_code)]

use std::{io, iter, path::PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::{editor::Editor, terminal::Terminal};

mod buffer;
mod editor;
mod lang;
mod renderer;
mod row;
mod terminal;

#[derive(Parser)]
#[command(
    name = "quire",
    about = "A light-weight editor for medium-weight editing"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Open a file directly in the editor.
    Open { path: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let term = Terminal::new()?;
    let size = Terminal::size()?;

    let event_stream = iter::from_fn(|| term.read_event().ok());

    let mut editor = match cli.command {
        Some(Command::Open { path }) => Editor::open(event_stream, io::stdout(), size, path)?,
        None => Editor::new(event_stream, io::stdout(), size)?,
    };
    editor.edit()?;
    Ok(())
}
