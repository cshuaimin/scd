use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;

use crossbeam_channel::Sender;
use nix::sys::signal::{kill, Signal};
use nix::sys::stat::Mode;
use nix::unistd::{mkfifo, Pid};
use serde::{Deserialize, Serialize};

const RECV_FIFO: &str = "/tmp/scd-recv-fifo";
const SEND_FILE: &str = "/tmp/scd-send";

/// Events emited from the shell.
#[derive(Serialize, Deserialize)]
pub enum ShellEvent {
    /// Shell started.
    Start(i32),

    /// The shell's current directory was changed.
    ChangeDirectory(PathBuf),

    /// Shell exited.
    Exit,
}

pub struct Shell {
    pid: AtomicI32,
}

impl Shell {
    pub fn new(tx: Sender<ShellEvent>) -> Self {
        thread::spawn(move || Self::read_commands(tx));

        Self {
            pid: AtomicI32::new(0),
        }
    }

    fn read_commands(tx: Sender<ShellEvent>) {
        let _ = mkfifo(RECV_FIFO, Mode::S_IRWXU);

        loop {
            let mut recv_fifo = File::open(RECV_FIFO).unwrap();
            let mut buf = Vec::new();
            recv_fifo.read_to_end(&mut buf).unwrap();
            let event = toml::from_slice(&buf).unwrap();
            tx.send(event).unwrap();
        }
    }

    pub fn set_pid(&mut self, pid: i32) {
        self.pid.store(pid, Ordering::Release);
    }

    pub fn emit(event: ShellEvent) {
        let mut recv_fifo = File::open(RECV_FIFO).unwrap();
        let buf = toml::to_vec(&event).unwrap();
        recv_fifo.write_all(&buf).unwrap();
    }

    pub fn run(&self, cmd: &str) {
        let pid = self.pid.load(Ordering::Acquire);
        if pid <= 0 {
            return;
        }

        let mut send_file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(SEND_FILE)
            .unwrap();
        send_file.write_all(cmd.as_bytes()).unwrap();
        kill(Pid::from_raw(pid), Signal::SIGUSR1).unwrap();
    }
}
