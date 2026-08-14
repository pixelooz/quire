#![allow(dead_code)]

use std::{io, iter, path::PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::{editor::Editor, terminal::Terminal};

mod editor;
mod renderer;
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
    let _cli = Cli::parse();

    let term = Terminal::new()?;
    let size = Terminal::size()?;

    let event_stream = iter::from_fn(|| term.read_event().ok());
    Editor::new(event_stream, io::stdout(), size)?.edit()?;

    Ok(())
}
