use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;

use crossbeam_channel::Sender;
use nix::sys::signal::{kill, Signal};
use nix::sys::stat::Mode;
use nix::unistd::{mkfifo, Pid};
use serde::{Deserialize, Serialize};

pub const RECV_FIFO: &str = "/tmp/scd-recv-fifo";
pub const SEND_FILE: &str = "/tmp/scd-send";

/// Events emited from the shell.
#[derive(Serialize, Deserialize)]
pub enum ShellEvent {
    /// Shell PID.
    Pid(i32),

    /// The shell's current directory was changed.
    ChangeDirectory(PathBuf),

    /// Shell exited.
    Exit,
}

impl ShellEvent {
    pub fn emit(&self) {
        let _ = mkfifo(RECV_FIFO, Mode::S_IRWXU);
        let buf = serde_json::to_vec(self).unwrap();
        fs::write(RECV_FIFO, buf).unwrap();
    }
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
            let buf = fs::read_to_string(RECV_FIFO).unwrap();
            let event = serde_json::from_str(&buf).unwrap();
            tx.send(event).unwrap();
        }
    }

    pub fn set_pid(&mut self, pid: i32) {
        self.pid.store(pid, Ordering::Release);
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

    pub fn cd(&self, dir: &Path) {
        self.run(&format!(
            "cd '{}' && commandline -f repaint",
            dir.to_str().unwrap()
        ));
    }
}

pub const FISH_INIT: &str = r#"
function __eval_cmd --on-signal SIGUSR1
    eval (scd get-cmd)
end

function __scd_cd --on-variable PWD
    scd cd "$PWD"
end

function __scd_exit --on-event fish_exit
    scd exit
end

scd send-pid $fish_pid
__scd_cd
"#;
