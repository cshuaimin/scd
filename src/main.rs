use std::io;
use std::path::PathBuf;

use anyhow::{Context, Result};
use structopt::StructOpt;
use termion::raw::IntoRawMode;
use tui::{backend::TermionBackend, Terminal};

use file_manager::*;

mod file_manager;

#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, StructOpt)]
enum Command {
    FishInit,
    GetCmd,
    Cd { dir: PathBuf },
    SendPid { pid: i32 },
    Exit,
}

pub fn run() -> Result<()> {
    let mut terminal = {
        let stdout = io::stdout().into_raw_mode()?;
        let backend = TermionBackend::new(stdout);
        Terminal::new(backend)?
    };
    let mut file_manager = FileManager::new(".")?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    loop {
        file_manager.draw(&mut terminal)?;
        file_manager.handle_event()?;
    }
}

fn main() -> Result<()> {
    let opt = Opt::from_args();
    match opt.command {
        None => run()?,
        Some(command) => match command {
            Command::FishInit => println!("{}", FISH_INIT),
            Command::GetCmd => println!("{}", Shell::receive_command()?),
            Command::SendPid { pid } => Shell::send_event(ShellEvent::Pid(pid))?,
            Command::Cd { dir } => Shell::send_event(ShellEvent::ChangeDirectory(dir))?,
            Command::Exit => Shell::send_event(ShellEvent::Exit)?,
        },
    }
    Ok(())
}
