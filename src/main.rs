use std::fs;
use std::io;
use std::path::PathBuf;

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

pub fn run() {
    let mut terminal = {
        let stdout = io::stdout().into_raw_mode().unwrap();
        let backend = TermionBackend::new(stdout);
        Terminal::new(backend).unwrap()
    };
    let mut file_manager = FileManager::new(".");
    terminal.hide_cursor().unwrap();
    terminal.clear().unwrap();
    loop {
        file_manager.draw(&mut terminal);
        file_manager.handle_event();
    }
}

fn main() {
    let opt = Opt::from_args();
    match opt.command {
        None => run(),
        Some(Command::FishInit) => println!("{}", FISH_INIT),
        Some(Command::GetCmd) => {
            let buf = fs::read_to_string(SEND_FILE).unwrap();
            println!("{}", buf);
        }
        Some(Command::Cd { dir }) => ShellEvent::ChangeDirectory(dir).emit(),
        Some(Command::SendPid { pid }) => ShellEvent::Pid(pid).emit(),
        Some(Command::Exit) => ShellEvent::Exit.emit(),
    }
}
