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

const OPEN_METHODS_CONFIG: &str = "open-methods.yml";

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
    pub fn new(event_tx: Sender<ShellEvent>) -> Result<Arc<Self>> {
        let open_methods = {
            let buf = fs::read_to_string(OPEN_METHODS_CONFIG).with_context(|| {
                format!("Failed to read open methods from {}", OPEN_METHODS_CONFIG)
            })?;
            let raw: HashMap<String, String> = serde_yaml::from_str(&buf)
                .with_context(|| format!("Failed to parse config file {}", OPEN_METHODS_CONFIG))?;
            let mut res = HashMap::new();
            for (exts, cmd) in raw {
                for ext in exts.split(',').map(str::trim) {
                    res.insert(ext.to_string(), cmd.clone());
                }
            }
            res
        };

        let (cmd_tx, cmd_rx) = bounded(0);
        let shell = Arc::new(Self {
            pid: AtomicI32::new(0),
            cmd_tx,
            open_methods,
        });

        thread::spawn({
            let shell = shell.clone();
            move || shell.receive_events(event_tx)
        });
        thread::spawn({
            let shell = shell.clone();
            move || shell.send_commands(cmd_rx)
        });

        Ok(shell)
    }

    /// Send commands sent from `rx` to the shell
    /// and notify the shell via SIGUSR1.
    fn send_commands(self: Arc<Self>, rx: Receiver<String>) -> Result<()> {
        let _ = mkfifo(CMDS_TO_RUN, Mode::S_IRWXU);
        loop {
            let cmd = rx.recv()?;
            let pid = Pid::from_raw(self.pid.load(Ordering::Acquire));
            kill(pid, Signal::SIGUSR1).with_context(|| "Failed to notify the shell")?;
            fs::write(CMDS_TO_RUN, cmd).with_context(|| "Failed to send command to shell")?;
        }
    }

    /// Receive a shell command to run.
    /// This function is called on the shell side.
    pub fn receive_command() -> Result<String> {
        let _ = mkfifo(CMDS_TO_RUN, Mode::S_IRWXU);
        fs::read_to_string(CMDS_TO_RUN).with_context(|| "Failed to receive command")
    }

    /// Send a shell event to the file manager.
    /// This function is called on the shell side.
    pub fn send_event(event: ShellEvent) -> Result<()> {
        let _ = mkfifo(SHELL_EVENTS, Mode::S_IRWXU);
        let buf = serde_json::to_vec(&event)?;
        fs::write(SHELL_EVENTS, buf).with_context(|| "Failed to send event to file manager")
    }

    /// Receive shell events and send it to `tx`.
    pub fn receive_events(self: Arc<Self>, tx: Sender<ShellEvent>) -> Result<()> {
        let _ = mkfifo(SHELL_EVENTS, Mode::S_IRWXU);
        loop {
            let buf =
                fs::read_to_string(SHELL_EVENTS).with_context(|| "Failed to read shell event")?;
            match serde_json::from_str(&buf)? {
                ShellEvent::Pid(pid) => self.pid.store(pid, Ordering::Release),
                other => tx.send(other)?,
            }
        }
    }

    /// Run a command in the shell.
    pub fn run(&self, cmd: &str, arg: &str) -> Result<()> {
        if self.pid.load(Ordering::Acquire) > 0 {
            let arg = format!(" '{}'", arg);
            let mut cmd = match cmd.contains("{}") {
                true => cmd.replace("{}", &arg),
                false => {
                    let mut cmd = cmd.to_string();
                    cmd.push_str(&arg);
                    cmd
                }
            };
            cmd.push_str(" && commandline -f repaint");
            self.cmd_tx.send(cmd)?;
        }
        Ok(())
    }

    pub fn cd(&self, dir: &Path) -> Result<()> {
        self.run("cd", dir.to_str().unwrap())
    }

    pub fn open_file(&self, file: &FileInfo) -> Result<()> {
        let open_cmd = match file.path.extension() {
            None => "xdg-open",
            Some(ext) => self
                .open_methods
                .get(ext.to_str().unwrap())
                .map(|s| s.as_str())
                .unwrap_or("xdg-open"),
        };
        self.run(open_cmd, file.path.to_str().unwrap())
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
