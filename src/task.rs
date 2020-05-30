use std::collections::HashMap;
use std::env;
use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::thread;

use anyhow::Result;
use crossbeam_channel::Sender;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::app::App;
use crate::event;

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
    pub command: String,
    pub rendered: String,
    pub status: Status,
    stdin: ChildStdin,
}

impl Task {
    pub fn new(command: String, rendered: String, tx: Sender<event::Event>) -> Result<(u32, Self)> {
        let shell = env::var("SHELL").unwrap_or("sh".to_string());
        // let c = format!("stdbuf -i0 -o0 -e0 {}", command);
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
                    .for_each(|line| {
                        let event = event::Event::Task(Event::Stdout { pid, line });
                        tx.send(event).unwrap();
                    });

                let exit_status = child.wait().unwrap();
                let event = event::Event::Task(Event::Exit { pid, exit_status });
                tx.send(event).unwrap();
            }
        });

        thread::spawn({
            move || {
                stderr
                    .split(b'\r')
                    .map(Result::unwrap)
                    .map(String::from_utf8)
                    .map(Result::unwrap)
                    .for_each(|line| {
                        let event = event::Event::Task(Event::Stderr { pid, line });
                        tx.send(event).unwrap();
                    });
            }
        });

        Ok((
            pid,
            Self {
                command,
                rendered,
                status: Status::Running("Running".to_string()),
                stdin,
            },
        ))
    }
}

pub fn handle_event(app: &mut App, event: Event) {
    match event {
        Event::Stdout { pid, line } | Event::Stderr { pid, line } => {
            let mut task = app.tasks.get_mut(&pid).unwrap();
            let name = task.command.split(' ').next().unwrap();
            let status = PARSERS
                .get(name)
                .and_then(|parser| parser.parse(line))
                .unwrap_or("Running".to_string());
            task.status = Status::Running(status);
        }
        Event::Exit { pid, exit_status } => {
            let mut task = app.tasks.get_mut(&pid).unwrap();
            task.status = Status::Exited(exit_status);
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
