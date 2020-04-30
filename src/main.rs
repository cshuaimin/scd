use std::fs;
use std::io;
use std::path::PathBuf;

use anyhow::Result;
use nix::sys::stat::Mode;
use nix::unistd::mkfifo;
use structopt::StructOpt;
use termion::event::Key;
use termion::raw::IntoRawMode;
use tui::backend::TermionBackend;
use tui::Terminal;

use app::*;
use draw::*;
use event::*;
use handlers::*;

mod app;
mod draw;
mod event;
mod handlers;
mod icons;
mod shell;

/// A tiny file manager focused on shell integration
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

fn run() -> Result<()> {
    mkfifo(shell::CMDS_TO_RUN, Mode::S_IRWXU)?;
    mkfifo(shell::SHELL_EVENTS, Mode::S_IRWXU)?;

    let mut terminal = {
        let stdout = io::stdout().into_raw_mode()?;
        let backend = TermionBackend::new(stdout);
        Terminal::new(backend)?
    };
    terminal.clear()?;

    let (events, watcher) = Events::new()?;
    let mut app = App::new(watcher)?;

    loop {
        terminal.draw(|mut frame| {
            draw_ui(&mut frame, &mut app);
        })?;
        match &app.mode {
            app::Mode::Input { prompt, offset, .. } => {
                terminal.set_cursor(
                    (prompt.len() + offset) as u16,
                    terminal.size()?.bottom() - 1,
                )?;
                terminal.show_cursor()?;
            }
            _ => terminal.hide_cursor()?,
        }

        match events.next()? {
            Event::Watch(_) => app.refresh_directory()?,
            Event::Shell(shell_event) => match shell_event {
                shell::Event::Pid(pid) => app.shell_pid = pid,
                shell::Event::ChangeDirectory(dir) => app.cd(dir)?,
                shell::Event::Exit => break,
            },
            Event::Key(Key::Char('q')) if matches!(app.mode, app::Mode::Normal) => {
                shell::deinit(app.shell_pid)?;
                break;
            }
            Event::Key(key) => handle_keys(&mut app, key)?,
            Event::Tick(tick) => handle_tick(&mut app, tick),
        }
    }

    fs::remove_file(shell::CMDS_TO_RUN)?;
    fs::remove_file(shell::SHELL_EVENTS)?;
    Ok(())
}

fn main() -> Result<()> {
    let opt = Opt::from_args();
    match opt.command {
        None => run()?,
        Some(command) => match command {
            Command::FishInit => println!("{}", shell::FISH_INIT),
            Command::GetCmd => println!("{}", shell::receive_command()?),
            Command::SendPid { pid } => shell::send_event(shell::Event::Pid(pid))?,
            Command::Cd { dir } => shell::send_event(shell::Event::ChangeDirectory(dir))?,
            Command::Exit => shell::send_event(shell::Event::Exit)?,
        },
    }
    Ok(())
}
