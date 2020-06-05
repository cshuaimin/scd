use std::collections::HashMap;
use std::env;
use std::io::Write;
use std::io::{self, BufRead, BufReader};
use std::process::{ChildStdin, Command, ExitStatus, Stdio};
use std::thread;

use anyhow::Result;
use crossbeam_channel::{self as channel, Receiver, Sender};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use once_cell::sync::Lazy;
use regex::Regex;
use termion::event::Key;
use termion::{color, cursor};
use tui::{backend::Backend, layout::Rect, Frame};
use unicode_width::UnicodeWidthStr;

use crate::status_bar::StatusBar;

pub enum Event {
    Stdout { pid: u32, line: String },
    Stderr { pid: u32, line: String },
    Exit { pid: u32, exit_status: ExitStatus },
}

pub enum Status {
    Running(String),
    Stopped,
    Exited(ExitStatus),
}

pub struct Task {
    pub pid: u32,
    pub command: String,
    pub rendered: String,
    pub status: Status,
    stdin: ChildStdin,
}

impl Task {
    pub fn new(command: String, rendered: String, tx: Sender<Event>) -> Result<Self> {
        let shell = env::var("SHELL").unwrap_or("sh".to_string());
        let mut child = Command::new(shell)
            .args(&["-c", &command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let pid = child.id();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let stderr = BufReader::new(child.stderr.take().unwrap());

        thread::spawn({
            let tx = tx.clone();
            move || {
                stdout
                    .split(b'\r')
                    .map(Result::unwrap)
                    .map(String::from_utf8)
                    .map(Result::unwrap)
                    .for_each(|line| tx.send(Event::Stdout { pid, line }).unwrap());

                let exit_status = child.wait().unwrap();
                tx.send(Event::Exit { pid, exit_status }).unwrap();
            }
        });

        thread::spawn({
            move || {
                stderr
                    .split(b'\r')
                    .map(Result::unwrap)
                    .map(String::from_utf8)
                    .map(Result::unwrap)
                    .for_each(|line| tx.send(Event::Stderr { pid, line }).unwrap());
            }
        });

        Ok(Self {
            pid,
            command,
            rendered,
            status: Status::Running("\u{f110}".to_string()),
            stdin,
        })
    }
}

pub struct TaskManager {
    tx: Sender<Event>,
    pub tasks: Vec<Task>,
    list_state: TaskListState,
}

impl TaskManager {
    pub fn new() -> Result<(TaskManager, Receiver<Event>)> {
        let (tx, rx) = channel::bounded(0);
        let task_manager = TaskManager {
            tx,
            tasks: vec![],
            list_state: TaskListState::default(),
        };
        Ok((task_manager, rx))
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Stdout { pid, line } | Event::Stderr { pid, line } => {
                let mut task = self.tasks.iter_mut().find(|t| t.pid == pid).unwrap();
                let name = task.command.split(' ').next().unwrap();
                let status = PARSERS
                    .get(name)
                    .and_then(|parser| parser.parse(line))
                    .unwrap_or("\u{f110}".to_string());
                task.status = Status::Running(status);
            }
            Event::Exit { pid, exit_status } => {
                let mut task = self.tasks.iter_mut().find(|t| t.pid == pid).unwrap();
                task.status = Status::Exited(exit_status);
                self.tasks.sort_by_key(|t| match t.status {
                    Status::Running(_) => 3,
                    Status::Stopped => 2,
                    Status::Exited(exit_status) => {
                        if !exit_status.success() {
                            1
                        } else {
                            0
                        }
                    }
                });
            }
        }
    }

    pub fn new_task(&mut self, command: String, rendered: String) -> Result<()> {
        let task = Task::new(command, rendered, self.tx.clone())?;
        self.tasks.push(task);
        Ok(())
    }

    pub fn on_key(&mut self, key: Key, status_bar: &mut StatusBar) {
        match key {
            Key::Char('k') => {
                if let Some(idx) = self.list_state.selected {
                    let pid = Pid::from_raw(self.tasks[idx].pid as i32);
                    status_bar.ask(
                        format!("Kill task {}?", self.tasks[idx].command),
                        move |_, _| {
                            kill(pid, Signal::SIGTERM).unwrap();
                        },
                    );
                }
            }
            _ => {}
        }
    }

    pub fn draw(&mut self, _frame: &mut Frame<impl Backend>, area: Rect) {
        let height = area.height as usize;
        let state = &mut self.list_state;
        // Make sure the list show the selected item
        state.offset = if let Some(selected) = state.selected {
            if selected >= height + state.offset - 1 {
                selected + 1 - height
            } else if selected < state.offset {
                selected
            } else {
                state.offset
            }
        } else {
            0
        };

        let max_status_width = self
            .tasks
            .iter()
            .map(|t| match &t.status {
                Status::Running(s) => s.width(),
                Status::Stopped | Status::Exited(_) => 1,
            })
            .max()
            .unwrap()
            .max("Status".len()) as u16;

        let mut stdout = io::stdout();

        macro_rules! draw {
            ($i:expr, $left:expr, $right_color:expr, $right:expr) => {{
                // `Goto` is (1,1)-based
                let y = area.top() + $i as u16 + 1;
                let left_pos = cursor::Goto(area.left() + 1, y);
                let right_pos = cursor::Goto(area.width - max_status_width, y);
                write!(stdout, "{}{}", left_pos, " ".repeat(area.width as usize)).unwrap();
                write!(stdout, "{}{}", left_pos, $left).unwrap();
                write!(
                    stdout,
                    "{} {}{:>w$}",
                    right_pos,
                    $right_color,
                    $right,
                    w = max_status_width as usize
                )
                .unwrap();
            }};
        }

        draw!(0, "Task", color::White.fg_str(), "Status");

        self.tasks
            .iter()
            .rev()
            .map(|task| {
                let (status_color, status) = match &task.status {
                    Status::Running(s) => (color::White.fg_str(), s.as_str()),
                    Status::Stopped => (color::LightYellow.fg_str(), "\u{f04c}"),
                    Status::Exited(s) => {
                        if s.success() {
                            (color::LightCyan.fg_str(), "✓")
                        } else {
                            (color::LightRed.fg_str(), "✗")
                        }
                    }
                };
                (task.rendered.as_str(), status_color, status)
            })
            .skip(state.offset)
            .take(height - 1)
            .enumerate()
            .for_each(|(i, (command, status_color, status))| {
                draw!(i + 1, command, status_color, status)
            });

        write!(
            stdout,
            "{}{}",
            cursor::Goto(area.left() + 1, area.bottom()),
            " ".repeat(area.width as usize)
        )
        .unwrap();
        stdout.flush().unwrap();
    }
}

pub struct TaskListState {
    offset: usize,
    selected: Option<usize>,
}

impl Default for TaskListState {
    fn default() -> Self {
        Self {
            offset: 0,
            selected: None,
        }
    }
}

static PARSERS: Lazy<HashMap<&str, Box<dyn ParseOutput>>> = Lazy::new(|| {
    let mut m: HashMap<&str, Box<dyn ParseOutput>> = HashMap::new();
    m.insert("curl", Box::new(Curl::new()));
    m
});

trait ParseOutput: Send + Sync {
    fn parse(&self, line: String) -> Option<String>;
}

struct Curl {
    re: Regex,
}

impl Curl {
    fn new() -> Self {
        Self {
            re: Regex::new(r"(\d+).*?(\w+)$").unwrap(),
        }
    }
}

impl ParseOutput for Curl {
    fn parse(&self, line: String) -> Option<String> {
        self.re
            .captures(&line)
            .map(|caps| format!("{}/s {}%", &caps[2], &caps[1]))
    }
}
