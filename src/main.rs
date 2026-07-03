#![allow(dead_code)]

use crate::{editor::Editor, terminal::Terminal};

use anyhow::Result;
use clap::Parser;
use env_logger::Target;
use std::{
    fs::{self, File, OpenOptions},
    io::{self},
    iter,
};

mod buffer;
mod buffer_tests;
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

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    paths: Vec<String>,
}

fn setup_log_path() -> Option<File> {
    let log_path = dirs::home_dir()?
        .join(".quill")
        .join("logs")
        .join("quill.log");

    if let Err(err) = fs::create_dir_all(log_path.parent()?) {
        eprintln!("quill: could not create log directory: {}", err);
        return None;
    }

    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(file) => {
            eprintln!("quill: logging to {}", log_path.display());
            Some(file)
        }
        Err(err) => {
            eprintln!("quill: could not open log file: {}", err);
            None
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    if let Some(log_file) = setup_log_path() {
        env_logger::Builder::new()
            .target(Target::Pipe(Box::new(log_file)))
            .filter_level(log::LevelFilter::Debug)
            .init();
    }
    let term = Terminal::new()?;
    let size = Terminal::size()?;

    let event_stream = iter::from_fn(|| Some(term.read_event()));

    let mut editor = if !args.paths.is_empty() {
        Editor::open(event_stream, io::stdout(), size, &args.paths)?
    } else {
        Editor::new(event_stream, io::stdout(), size)?
    };
    editor.edit()?;
    Ok(())
}
