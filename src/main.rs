use std::env;
use std::io;
use std::path::PathBuf;

use anyhow::Result;
use notify::EventKind;
use structopt::StructOpt;
use termion::event::Key;
use termion::raw::IntoRawMode;
use termion::screen::AlternateScreen;
use tui::backend::TermionBackend;
use tui::Terminal;

use app::*;
use draw::*;
use event::*;
use handlers::*;
use task::Task;

mod app;
mod draw;
mod event;
mod handlers;
mod icons;
mod shell;
mod task;
#[cfg(test)]
mod tests;

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
        None => run()?,
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

fn run() -> Result<()> {
    let mut terminal = {
        let stdout = io::stdout().into_raw_mode()?;
        let stdout = AlternateScreen::from(stdout);
        let backend = TermionBackend::new(stdout);
        Terminal::new(backend)?
    };

    let (events, watcher) = Events::new()?;
    let mut app = App::new(watcher, env::current_dir()?)?;

    loop {
        terminal.draw(|mut frame| draw_ui(&mut frame, &mut app))?;

        match &app.mode {
            Mode::Input { prompt, offset, .. } => {
                terminal.set_cursor(
                    (prompt.len() + offset) as u16,
                    terminal.size()?.bottom() - 1,
                )?;
                terminal.show_cursor()?;
            }
            _ => terminal.hide_cursor()?,
        }

        match events.next()? {
            Event::Watch(watch) => match watch.kind {
                EventKind::Create(_) | EventKind::Remove(_) => match app.read_dir() {
                    Ok(res) => {
                        app.all_files = res;
                        app.apply_filter();
                    }
                    Err(e) => {
                        app.show_message(&e.to_string());
                    }
                },
                _ => {}
            },
            Event::Shell(shell_event) => match shell_event {
                shell::Event::Pid(pid) => app.shell_pid = pid,
                shell::Event::ChangeDirectory(dir) => app.cd(dir)?,
                shell::Event::Exit => break,
                shell::Event::Task { command, rendered } => {
                    app.tasks
                        .push(Task::new(command, rendered, events.tx.clone())?);
                }
            },
            Event::Key(Key::Char('q')) if app.mode == app::Mode::Normal => {
                if app.shell_pid > 0 {
                    shell::deinit(app.shell_pid)?;
                }
                break;
            }
            Event::Key(key) => {
                if let Err(e) = handle_keys(&mut app, key) {
                    app.show_message(&e.to_string());
                }
            }
            Event::Tick(tick) => handle_tick(&mut app, tick),
            Event::Task(task_event) => task::handle_event(&mut app, task_event),
        }
    }
    Ok(())
}
