use crate::{editor::Editor, terminal::Terminal};

use anyhow::Result;
use clap::{Parser, Subcommand};
use env_logger::Target;
use std::{
    fs::{self, File, OpenOptions},
    io::{self},
    iter,
    path::PathBuf,
};

mod buffer;
mod color;
mod command;
mod diff;
mod editor;
mod help;
mod highlight;
mod history;
mod lang;
mod renderer;
mod row;
mod status_bar;
mod terminal;

/* TODO 1. document the code and for simple functions just introduce what
the function does.
*/

#[derive(Parser)]
#[command(
    name = "quire",
    about = "A light-weight editor for medium-weight editor"
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Open { path: Vec<PathBuf> },
}

fn setup_log_path() -> Option<File> {
    let log_path = dirs::home_dir()?
        .join(".quire")
        .join("logs")
        .join("quire.log");

    if let Err(err) = fs::create_dir_all(log_path.parent()?) {
        eprintln!("quire: could not create log directory: {}", err);
        return None;
    }

    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(file) => {
            eprintln!("quire: logging to {}", log_path.display());
            Some(file)
        }
        Err(err) => {
            eprintln!("quire: could not open log file: {}", err);
            None
        }
    }
}

fn main() -> Result<()> {
    if let Some(log_file) = setup_log_path() {
        env_logger::Builder::new()
            .target(Target::Pipe(Box::new(log_file)))
            .filter_level(log::LevelFilter::Debug)
            .init();
    }
    let args = Args::parse();

    let term = Terminal::new()?;
    let size = Terminal::size()?;

    let event_stream = iter::from_fn(|| Some(term.read_event()));

    let mut editor = match args.command {
        Some(Command::Open { path }) => Editor::open(event_stream, io::stdout(), size, &path)?,
        None => Editor::new(event_stream, io::stdout(), size)?,
    };
    editor.edit()?;
    Ok(())
}
