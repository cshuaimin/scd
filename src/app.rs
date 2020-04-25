use std::env;
use std::io;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_channel::{bounded, select, tick, Receiver};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use sysinfo::{System, SystemExt};
use termion::input::MouseTerminal;
use termion::raw::IntoRawMode;
use termion::{event::Key, input::TermRead};
use tui::backend::TermionBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::{backend::Backend, Terminal};

use crate::widgets::*;

pub struct App {
    watcher: RecommendedWatcher,
    file_view_state: FileViewState,
    shell: Shell,
    system: System,

    watch_rx: Receiver<notify::Event>,
    key_rx: Receiver<Key>,
    shell_rx: Receiver<ShellEvent>,
    tick: Receiver<Instant>,
}

impl App {
    pub fn new() -> Result<Self> {
        let (watch_tx, watch_rx) = bounded(0);
        let watcher =
            RecommendedWatcher::new_immediate(move |res: notify::Result<notify::Event>| {
                watch_tx.send(res.unwrap()).unwrap();
            })?;

        let (key_tx, key_rx) = bounded(0);
        thread::spawn(move || {
            let keys = io::stdin().keys();
            for key in keys {
                key_tx.send(key.unwrap()).unwrap();
            }
        });

        let (shell_tx, shell_rx) = bounded(0);
        let shell = Shell::new(shell_tx)?;

        let tick = tick(Duration::from_secs(2));

        let file_view_state = FileViewState::new();
        let mut system = System::new_all();
        system.refresh_cpu();
        system.refresh_memory();

        let mut app = Self {
            watcher,
            file_view_state,
            shell,
            system,

            watch_rx,
            key_rx,
            shell_rx,
            tick,
        };
        app.enter_directory(env::current_dir()?)?;

        Ok(app)
    }

    fn enter_directory(&mut self, dir: PathBuf) -> Result<()> {
        if self.file_view_state.dir != PathBuf::new() {
            self.watcher.unwatch(&self.file_view_state.dir)?;
        }
        self.file_view_state.dir = dir;
        self.file_view_state.read_dir()?;
        if self.file_view_state.files.len() > 0 {
            self.file_view_state.list_state.select(Some(0));
        }
        self.watcher
            .watch(&self.file_view_state.dir, RecursiveMode::NonRecursive)?;
        Ok(())
    }

    pub fn draw<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        terminal.draw(|mut frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .horizontal_margin(1)
                .constraints([Constraint::Length(9), Constraint::Min(0)].as_ref())
                .split(frame.size());
            frame.render_stateful_widget(SystemMonitor, chunks[0], &mut self.system);
            frame.render_stateful_widget(FileView, chunks[1], &mut self.file_view_state);
        })
    }

    pub fn handle_event(&mut self) -> Result<bool> {
        select! {
            recv(self.tick) -> _tick => {
                self.system.refresh_cpu();
                self.system.refresh_memory();
            }
            recv(self.watch_rx) -> _watch => self.file_view_state.read_dir()?,
            recv(self.shell_rx) -> shell_event => {
                match shell_event? {
                    ShellEvent::ChangeDirectory(dir) => {
                        if dir != self.file_view_state.dir {
                            self.enter_directory(dir)?;
                        }
                    }
                    ShellEvent::Exit => return Ok(true),
                    _ => {}
                }
            }
            recv(self.key_rx) -> key => {
                match key? {
                    Key::Char('j') | Key::Down => self.file_view_state.select_next(),
                    Key::Char('k') | Key::Up => self.file_view_state.select_prev(),
                    Key::Char('g') | Key::Home => self.file_view_state.select_first(),
                    Key::Char('G') | Key::End => self.file_view_state.select_last(),
                    Key::Char('l') | Key::Char('\n') => {
                        if let Some(selected) = self.file_view_state.selected() {
                            if selected.file_type == FileType::Directory {
                                let dir = selected.path.clone();
                                self.enter_directory(dir)?;
                                self.shell.cd(&self.file_view_state.dir)?;
                            } else {
                                self.shell.open_file(&selected)?;
                            }
                        }
                    }
                    Key::Char('h') | Key::Esc => {
                        if let Some(parent) = self.file_view_state.dir.parent() {
                            let parent = parent.to_owned();
                            let current_dir_name = self
                                .file_view_state
                                .dir
                                .file_name()
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .to_owned();
                            self.enter_directory(parent)?;
                            let index = self
                                .file_view_state
                                .files
                                .iter()
                                .position(|file| file.name == current_dir_name);
                            self.file_view_state.list_state.select(index);
                            self.shell.cd(&self.file_view_state.dir)?;
                        }
                    }
                    Key::Char('.') => {
                        self.file_view_state.show_hidden_files = !self.file_view_state.show_hidden_files;
                        self.file_view_state.read_dir()?;
                    }
                    Key::Char('q') => return Ok(true),
                    _ => {}
                }
            }
        }
        Ok(false)
    }
}

pub fn run() -> Result<()> {
    let mut terminal = {
        let stdout = io::stdout().into_raw_mode()?;
        let stdout = MouseTerminal::from(stdout);
        // let stdout = AlternateScreen::from(stdout);
        let backend = TermionBackend::new(stdout);
        Terminal::new(backend)?
    };
    terminal.hide_cursor()?;
    terminal.clear()?;

    let mut app = App::new()?;
    loop {
        app.draw(&mut terminal)?;
        let exit = app.handle_event()?;
        if exit {
            break;
        }
    }
    Ok(())
}
