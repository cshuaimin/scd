use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use nix::sys::signal::{kill, Signal};
use nix::sys::stat::Mode;
use nix::unistd::mkfifo;
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};

use crate::{App, FileInfo};

/// Send shell commands from scd to shell.
pub const CMDS_TO_RUN: &str = "/tmp/scd-cmds-to-run";

/// Send `ShellEvent` from the shell to scd.
pub const SHELL_EVENTS: &str = "/tmp/scd-shell-events";

pub const OPEN_METHODS_CONFIG: &str = "open-methods.yml";

/// Events emitted from the shell.
#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    /// Shell PID.
    Pid(i32),

    /// The shell's current directory was changed.
    ChangeDirectory(PathBuf),

    /// Shell exited.
    Exit,
}

/// Send a command to the shell and notify it via SIGUSR1.
fn send_command(cmd: impl AsRef<[u8]>, pid: i32) -> Result<()> {
    if pid > 0 {
        let pid = Pid::from_raw(pid);
        kill(pid, Signal::SIGUSR1).with_context(|| "Failed to notify the shell")?;
        fs::write(CMDS_TO_RUN, cmd).with_context(|| "Failed to send command to shell")?;
    }
    Ok(())
}

/// Receive a shell command to run.
///
/// This function is called on the shell side.
pub fn receive_command() -> Result<String> {
    let _ = mkfifo(CMDS_TO_RUN, Mode::S_IRWXU);
    fs::read_to_string(CMDS_TO_RUN).with_context(|| "Failed to receive command")
}

/// Send a shell event to the file manager.
///
/// This function is called on the shell side.
pub fn send_event(event: Event) -> Result<()> {
    let _ = mkfifo(SHELL_EVENTS, Mode::S_IRWXU);
    let buf = serde_yaml::to_vec(&event)?;
    fs::write(SHELL_EVENTS, buf).with_context(|| "Failed to send event to file manager")
}

/// Receive a shell event.
pub fn receive_event() -> Result<Event> {
    let buf = fs::read_to_string(SHELL_EVENTS).with_context(|| "Failed to read shell event")?;
    Ok(serde_yaml::from_str(&buf)?)
}

/// Run a command in the shell.
///
/// The command will be shown in the terminal, as if typed by user.
pub fn run(cmd: &str, args: &[impl AsRef<str>], pid: i32) -> Result<()> {
    let args = args
        .into_iter()
        .map(|a| format!("'{}'", a.as_ref()))
        .collect::<Vec<_>>()
        .join(" ");
    let cmd = match cmd.contains("{}") {
        true => cmd.replace("{}", &args),
        false => format!("{} {}", cmd, args),
    };
    let cmd = format!("commandline '{}' && commandline -f execute", cmd);
    send_command(cmd, pid)
}

pub fn cd(dir: &Path, pid: i32) -> Result<()> {
    let cmd = format!("cd '{}' && commandline -f repaint", dir.to_str().unwrap());
    send_command(cmd, pid)
}

pub fn open_file(file: &FileInfo, app: &App) -> Result<()> {
    let open_cmd = match &file.extension {
        None => "xdg-open",
        Some(ext) => app
            .open_methods
            .get(ext)
            .map(String::as_str)
            .unwrap_or("xdg-open"),
    };
    run(open_cmd, &[&file.name], app.shell_pid)
}

pub fn deinit(pid: i32) -> Result<()> {
    send_command(FISH_DEINIT, pid)
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

pub const FISH_DEINIT: &str = r#"
functions --erase __eval_cmd __scd_cd __scd_exit
"#;
