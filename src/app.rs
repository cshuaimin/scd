use std::io;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use crossbeam_channel::{self as channel, select, Receiver};
use notify::RecommendedWatcher;
use termion::{event::Key, input::TermRead, raw::IntoRawMode, screen::AlternateScreen};
use tui::backend::TermionBackend;
use tui::layout::{Constraint, Direction, Layout};
use tui::Terminal;

use crate::file_manager::FileManager;
use crate::shell;
use crate::status_bar::{Mode, StatusBar};
use crate::system_monitor::SystemMonitor;
use crate::task_manager::{self, TaskManager};

enum InputFocus {
    FileManager,
    TaskManager,
}

pub struct App {
    system_monitor: SystemMonitor,
    file_manager: FileManager<RecommendedWatcher>,
    task_manager: TaskManager,
    status_bar: StatusBar,

    input_focus: InputFocus,
    keys: channel::Receiver<Key>,
    ticks: Receiver<Instant>,
    watch_events: Receiver<notify::Event>,
    task_events: Receiver<task_manager::Event>,
    shell_events: Receiver<shell::Event>,
}

impl App {
    pub fn new() -> Result<App> {
        let system_monitor = SystemMonitor::new();
        let (file_manager, watch_events) = FileManager::new()?;
        let (task_manager, task_events) = TaskManager::new()?;
        let status_bar = StatusBar::new();

        let (tx, keys) = channel::bounded(0);
        thread::spawn(move || {
            io::stdin()
                .keys()
                .map(Result::unwrap)
                .for_each(|k| tx.send(k).unwrap());
        });

        let (tx, shell_events) = channel::bounded(0);
        thread::spawn(move || shell::receive_events(tx));

        Ok(App {
            system_monitor,
            file_manager,
            task_manager,
            status_bar,

            input_focus: InputFocus::FileManager,
            keys,
            ticks: channel::tick(Duration::from_secs(2)),
            watch_events,
            task_events,
            shell_events,
        })
    }

    pub fn run(&mut self) -> Result<()> {
        let mut terminal = {
            let stdout = io::stdout().into_raw_mode()?;
            let stdout = AlternateScreen::from(stdout);
            let backend = TermionBackend::new(stdout);
            Terminal::new(backend)?
        };

        loop {
            terminal.draw(|mut frame| {
                // + 3: seperator, title, seperator
                let task_height =
                    (self.task_manager.tasks.len() as u16 + 3).min(frame.size().height / 3);
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(
                        [
                            Constraint::Length(8),
                            Constraint::Min(0),
                            Constraint::Length(task_height),
                            Constraint::Length(1),
                        ]
                        .as_ref(),
                    )
                    .split(frame.size());

                self.system_monitor.draw(&mut frame, chunks[0]);
                self.file_manager.draw(&mut frame, chunks[1]);
                if !self.task_manager.tasks.is_empty() {
                    self.task_manager.draw(&mut frame, chunks[2]);
                }
                self.status_bar
                    .draw(&mut self.file_manager, &mut frame, chunks[3]);
            })?;

            let bottom = terminal.size()?.bottom() - 1;
            match &self.status_bar.mode {
                Mode::Ask { prompt, .. } => {
                    terminal.set_cursor(prompt.len() as u16, bottom)?;
                    terminal.show_cursor()?;
                }
                Mode::Edit { prompt, cursor, .. } => {
                    terminal.set_cursor((prompt.len() + cursor) as u16, bottom)?;
                    terminal.show_cursor()?;
                }
                _ => terminal.hide_cursor()?,
            }

            macro_rules! catch_error {
                ($r:expr) => {
                    if let Err(e) = $r {
                        self.status_bar.show_message(e.to_string());
                    }
                };
            }

            select! {
                recv(self.keys) -> key => {
                    let key = key.unwrap();
                    match self.status_bar.mode {
                        Mode::Ask { .. } | Mode::Edit { .. } => {
                            catch_error!(self.status_bar.on_key(key, &mut self.file_manager, &mut self.task_manager));
                        }
                        _ => match key {
                            Key::Char('q') => {
                                shell::deinit(self.file_manager.shell_pid)?;
                                break;
                            }
                            Key::Char('\t') => match self.input_focus {
                                InputFocus::FileManager => self.input_focus = InputFocus::TaskManager,
                                InputFocus::TaskManager => self.input_focus = InputFocus::FileManager,
                            }
                            key => catch_error!(match self.input_focus {
                                InputFocus::FileManager => self.file_manager.on_key(key, &mut self.status_bar),
                                InputFocus::TaskManager => self.task_manager.on_key(key, &mut self.status_bar),
                            })
                        }
                    }
                }
                recv(self.ticks) -> tick => {
                    let tick = tick.unwrap();
                    self.system_monitor.on_tick(tick);
                    self.status_bar.on_tick(tick);
                }
                recv(self.watch_events) -> watch => catch_error!(self.file_manager.on_notify(watch.unwrap())),
                recv(self.task_events) -> task_event => self.task_manager.on_event(task_event.unwrap()),
                recv(self.shell_events) -> shell_event => {
                    catch_error!(match shell_event.unwrap() {
                        shell::Event::Exit => break,
                        shell::Event::Task { command, rendered } => self.task_manager.new_task(command, rendered),
                        event => self.file_manager.on_shell_event(event),
                    })
                }
            }
        }

        Ok(())
    }
}

pub trait ListExt {
    type Item;

    fn get_index(&self) -> Option<usize>;
    fn get_list(&self) -> &[Self::Item];
    fn select(&mut self, index: Option<usize>);

    fn selected(&self) -> Option<&Self::Item> {
        if self.get_list().is_empty() {
            None
        } else {
            let idx = self.get_index().unwrap_or(0);
            Some(&self.get_list()[idx])
        }
    }

    fn select_first(&mut self) {
        let index = if self.get_list().is_empty() {
            None
        } else {
            Some(0)
        };
        self.select(index);
    }

    fn select_last(&mut self) {
        let index = match self.get_list().len() {
            0 => None,
            len => Some(len - 1),
        };
        self.select(index);
    }

    fn select_next(&mut self) {
        let index = self.get_index().map(|i| (i + 1) % self.get_list().len());
        self.select(index);
    }

    fn select_prev(&mut self) {
        let index = match self.get_index() {
            None => None,
            Some(0) if self.get_list().is_empty() => None,
            Some(0) => Some(self.get_list().len() - 1),
            Some(i) => Some(i - 1),
        };
        self.select(index);
    }

    fn on_list_key(&mut self, key: Key) -> Result<()> {
        match key {
            Key::Char('j') | Key::Ctrl('n') | Key::Down => self.select_next(),
            Key::Char('k') | Key::Ctrl('p') | Key::Up => self.select_prev(),
            Key::Char('g') | Key::Home => self.select_first(),
            Key::Char('G') | Key::End => self.select_last(),
            uk => bail!("Unknown key: {:?}", uk),
        }
        Ok(())
    }
}
