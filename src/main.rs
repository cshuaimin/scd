use std::path::PathBuf;

use anyhow::Result;
use structopt::StructOpt;

use app::App;

mod app;
mod file_manager;
mod shell;
mod status_bar;
mod system_monitor;
mod task_manager;

/// A tiny file manager focused on shell integration
#[derive(Debug, StructOpt)]
struct Opt {
    #[structopt(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, StructOpt)]
enum Command {
    FishInit,
    ZshInit,
    GetCmd,

    Cd { dir: PathBuf },
    SendPid { pid: i32 },
    SendTask { command: String, rendered: String },
    Exit,
}

fn main() -> Result<()> {
    let opt = Opt::from_args();
    match opt.command {
        None => App::new()?.run()?,
        Some(command) => match command {
            Command::FishInit => println!("{}", shell::FISH_INIT),
            Command::ZshInit => println!("{}", shell::ZSH_INIT),
            Command::GetCmd => println!("{}", shell::receive_command()?),

            Command::SendPid { pid } => shell::send_event(shell::Event::Pid(pid))?,
            Command::SendTask { command, rendered } => {
                shell::send_event(shell::Event::Task { command, rendered })?
            }
            Command::Cd { dir } => shell::send_event(shell::Event::ChangeDirectory(dir))?,
            Command::Exit => shell::send_event(shell::Event::Exit)?,
        },
    }
    Ok(())
}
