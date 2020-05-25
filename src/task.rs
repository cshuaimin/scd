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
    Stdout(String),
    Stderr(String),
    Exit(ExitStatus),
}

pub enum Status {
    Running(String),
    Stopped,
    Exited(ExitStatus),
}

pub struct Task {
    pub command: String,
    pub status: Status,
    stdin: ChildStdin,
}

impl Task {
    pub fn new(command: String, tx: Sender<event::Event>) -> Result<(u32, Self)> {
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
                        tx.send(event::Event::Task {
                            pid,
                            event: Event::Stdout(line),
                        })
                        .unwrap();
                    });

                let exit_status = child.wait().unwrap();
                tx.send(event::Event::Task {
                    pid,
                    event: Event::Exit(exit_status),
                })
                .unwrap();
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
                        tx.send(event::Event::Task {
                            pid,
                            event: Event::Stdout(line),
                        })
                        .unwrap();
                    });
            }
        });

        Ok((
            pid,
            Self {
                command,
                status: Status::Running("Running".to_string()),
                stdin,
            },
        ))
    }
}

pub fn handle_event(app: &mut App, pid: u32, event: Event) {
    let mut task = app.tasks.get_mut(&pid).unwrap();

    match event {
        Event::Stdout(line) | Event::Stderr(line) => {
            let name = task.command.split(' ').next().unwrap();
            let status = PARSERS
                .get(name)
                .and_then(|parser| parser.parse(line))
                .unwrap_or("Running".to_string());
            task.status = Status::Running(status);
        }
        Event::Exit(exit_status) => task.status = Status::Exited(exit_status),
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
