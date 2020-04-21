use std::fs;
use std::path::PathBuf;

use structopt::StructOpt;

use app::*;
use shell::*;

mod app;
mod shell;

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
        Some(Command::Exit) => return,
    }
}
