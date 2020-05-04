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

#[derive(Debug, Serialize, Deserialize)]
enum RunCommand {
    Silently(String),
    WithEcho(String),
}

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

pub trait Shell {
    fn silently(&self, cmd: String) -> String;
    fn with_echo(&self, cmd: String) -> String;
}

pub struct Fish;

impl Shell for Fish {
    fn silently(&self, cmd: String) -> String {
        format!("{} && commandline -f repaint", cmd)
    }

    fn with_echo(&self, cmd: String) -> String {
        format!("commandline \"{}\" && commandline -f execute", cmd)
    }
}

pub struct Zsh;

impl Shell for Zsh {
    fn silently(&self, cmd: String) -> String {
        format!("{} && zle reset-prompt", cmd)
    }

    fn with_echo(&self, cmd: String) -> String {
        // manually echo and add cmd to history
        format!("echo {0} && {0} && print -s {0} && zle reset-prompt", cmd)
    }
}

/// Receive a shell command to run.
///
/// This function is called on the shell side.
pub fn receive_command(shell: impl Shell) -> Result<String> {
    let _ = mkfifo(CMDS_TO_RUN, Mode::S_IRWXU);
    let buf = fs::read_to_string(CMDS_TO_RUN).with_context(|| "Failed to receive command")?;
    Ok(match serde_yaml::from_str(&buf)? {
        RunCommand::Silently(cmd) => shell.silently(cmd),
        RunCommand::WithEcho(cmd) => shell.with_echo(cmd),
    })
}

/// Send a shell event to the file manager.
///
/// This function is called on the shell side.
pub fn send_event(event: Event) -> Result<()> {
    let _ = mkfifo(SHELL_EVENTS, Mode::S_IRWXU);
    let buf = serde_yaml::to_vec(&event)?;
    fs::write(SHELL_EVENTS, buf).with_context(|| "Failed to send event to file manager")
}

/// Send a command to the shell and notify it via SIGUSR1.
fn send_command(cmd: RunCommand, pid: i32) -> Result<()> {
    if pid > 0 {
        let pid = Pid::from_raw(pid);
        kill(pid, Signal::SIGUSR1).with_context(|| "Failed to notify the shell")?;
        fs::write(CMDS_TO_RUN, serde_yaml::to_string(&cmd)?)
            .with_context(|| "Failed to send command to shell")?;
    }
    Ok(())
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
        .iter()
        .map(|a| format!("'{}'", a.as_ref()))
        .collect::<Vec<_>>()
        .join(" ");
    let cmd = if cmd.contains("{}") {
        cmd.replace("{}", &args)
    } else {
        format!("{} {}", cmd, args)
    };
    send_command(RunCommand::WithEcho(cmd), pid)
}

pub fn cd(dir: &Path, pid: i32) -> Result<()> {
    send_command(
        RunCommand::Silently(format!("cd '{}'", dir.to_str().unwrap().to_string())),
        pid,
    )
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
    send_command(RunCommand::Silently("scd_deinit".to_string()), pid)
}

pub const FISH_INIT: &str = r#"
function scd_run_cmd --on-signal SIGUSR1
    eval (scd get-cmd-fish)
end

function scd_cd --on-variable PWD
    scd cd "$PWD"
end

function scd_exit --on-event fish_exit
    scd exit
end

scd send-pid $fish_pid
scd_cd

function scd_deinit
    functions --erase scd_run_cmd scd_cd scd_exit scd_deinit
end
"#;

pub const ZSH_INIT: &str = r#"
TRAPUSR1() {
    eval $(scd get-cmd-zsh)
}

scd_cd() {
    scd cd "$PWD"
}

scd_exit() {
    scd exit
}

autoload add-zsh-hook
add-zsh-hook chpwd scd_cd
add-zsh-hook zshexit scd_exit
scd send-pid "$$"

scd_deinit() {
    add-zsh-hook -d chpwd scd_cd
    add-zsh-hook -d zshexit scd_exit
    unfunction TRAPUSR1 scd_cd scd_exit scd_deinit
}
"#;
