use std::path::PathBuf;

use anyhow::Result;
use structopt::StructOpt;

use widgets::*;

mod app;
mod widgets;

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
fn main() -> Result<()> {
    let opt = Opt::from_args();
    match opt.command {
        None => app::run()?,
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
