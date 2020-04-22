use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::Sender;
use nix::sys::signal::{kill, Signal};
use nix::sys::stat::Mode;
use nix::unistd::{mkfifo, Pid};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicI32, Ordering};

/// Send shell commands from scd to shell.
pub const CMDS_TO_RUN: &str = "/tmp/scd-cmds-to-run";

/// Send `ShellEvent` from the shell to scd.
pub const SHELL_EVENTS: &str = "/tmp/scd-shell-events";

const OPEN_METHODS_CONFIG: &str = "open-methods.toml";

use super::*;

/// Events emitted from the shell.
#[derive(Debug, Serialize, Deserialize)]
pub enum ShellEvent {
    /// Shell PID.
    Pid(i32),

    /// The shell's current directory was changed.
    ChangeDirectory(PathBuf),

    /// Shell exited.
    Exit,
}

#[derive(Debug)]
pub struct Shell {
    pid: AtomicI32,
    cmd_tx: Sender<String>,
    open_methods: HashMap<String, String>,
}

impl Shell {
    pub fn new(event_tx: Sender<ShellEvent>) -> Arc<Self> {
        let open_methods = {
            let buf = fs::read_to_string(OPEN_METHODS_CONFIG).unwrap();
            let raw: HashMap<String, Vec<String>> = toml::from_str(&buf).unwrap();
            let mut open_methods = HashMap::new();
            for (cmd, exts) in raw {
                for ext in exts {
                    open_methods.insert(ext, cmd.clone());
                }
            }
            open_methods
        };

        let (cmd_tx, cmd_rx) = bounded(0);
        let shell = Arc::new(Self {
            pid: AtomicI32::new(0),
            cmd_tx,
            open_methods,
        });

        let _ = mkfifo(CMDS_TO_RUN, Mode::S_IRWXU);
        let _ = mkfifo(SHELL_EVENTS, Mode::S_IRWXU);

        thread::spawn({
            let shell = shell.clone();
            move || shell.receive_events(event_tx)
        });
        thread::spawn({
            let shell = shell.clone();
            move || shell.send_commands(cmd_rx)
        });

        shell
    }

    /// Send commands sent from `rx` to the shell
    /// and notify the shell via SIGUSR1.
    fn send_commands(self: Arc<Self>, rx: Receiver<String>) {
        loop {
            let cmd = rx.recv().unwrap();
            let pid = Pid::from_raw(self.pid.load(Ordering::Acquire));
            kill(pid, Signal::SIGUSR1).unwrap();
            fs::write(CMDS_TO_RUN, cmd).unwrap();
        }
    }

    /// Receive a shell command to run.
    /// This function is called on the shell side.
    pub fn receive_command() -> String {
        fs::read_to_string(CMDS_TO_RUN).unwrap()
    }

    /// Send a shell event to the file manager.
    /// This function is called on the shell side.
    pub fn send_event(event: ShellEvent) {
        let buf = serde_json::to_vec(&event).unwrap();
        fs::write(SHELL_EVENTS, buf).unwrap();
    }

    /// Receive shell events and send it to `tx`.
    pub fn receive_events(self: Arc<Self>, tx: Sender<ShellEvent>) {
        loop {
            let buf = fs::read_to_string(SHELL_EVENTS).unwrap();
            match serde_json::from_str(&buf).unwrap() {
                ShellEvent::Pid(pid) => self.pid.store(pid, Ordering::Release),
                other => tx.send(other).unwrap(),
            }
        }
    }

    /// Run a command in the shell.
    pub fn run(&self, cmd: &str, args: &[&str]) {
        if self.pid.load(Ordering::Acquire) > 0 {
            let args = args.join(" ");
            let cmd = format!("{} \"{}\" && commandline -f repaint", cmd, args);
            self.cmd_tx.send(cmd).unwrap();
        }
    }

    pub fn cd(&self, dir: &Path) {
        self.run("cd", &[dir.to_str().unwrap()]);
    }

    pub fn open_file(&self, file: &FileInfo) {
        let open_cmd = match file.path.extension() {
            None => "xdg-open",
            Some(ext) => self
                .open_methods
                .get(ext.to_str().unwrap())
                .map(|s| s.as_str())
                .unwrap_or("xdg-open"),
        };
        self.run(open_cmd, &[file.path.to_str().unwrap()]);
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
