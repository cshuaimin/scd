use std::fs::{self, File, OpenOptions};
use std::io::prelude::*;
use std::mem;
use std::path::PathBuf;

use anyhow::{Context, Result};
use nix::sys::signal::{kill, Signal};
use nix::sys::stat::Mode;
use nix::unistd::mkfifo;
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};

/// Send shell commands from scd to shell.
pub const CMDS_TO_RUN: &str = "/tmp/scd-cmds-to-run";

/// Send `ShellEvent` from the shell to scd.
pub const SHELL_EVENTS: &str = "/tmp/scd-shell-events";

/// Run a command in the shell.
pub fn run(pid: i32, cmd: &str, args: &[impl AsRef<str>], echo: bool) -> Result<()> {
    let _ = mkfifo(CMDS_TO_RUN, Mode::S_IRWXU);
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
    let cmd = if echo {
        format!("scd_run_with_echo \"{}\"", cmd)
    } else {
        format!("scd_run_silently \"{}\"", cmd)
    };

    kill(Pid::from_raw(pid), Signal::SIGUSR1).with_context(|| "Failed to notify the shell")?;
    let mut fifo = OpenOptions::new().write(true).open(CMDS_TO_RUN)?;
    let cmd = cmd.as_bytes();
    fifo.write_all(&cmd.len().to_ne_bytes())?;
    fifo.write_all(cmd)?;
    Ok(())
}

/// Receive a shell command to run.
///
/// This function is called on the shell side.
pub fn receive_command() -> Result<String> {
    let _ = mkfifo(CMDS_TO_RUN, Mode::S_IRWXU);
    let mut file = File::open(CMDS_TO_RUN)?;

    let mut len_bytes = [0; mem::size_of::<usize>()];
    file.read_exact(&mut len_bytes)?;
    let len = usize::from_ne_bytes(len_bytes);
    let mut buf = vec![0; len];
    file.read_exact(&mut buf)?;
    Ok(String::from_utf8(buf)?)
}

/// Events emitted from the shell.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub enum Event {
    /// Shell PID.
    Pid(i32),

    /// The shell's current directory was changed.
    ChangeDirectory(PathBuf),

    /// Shell exited.
    Exit,
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
    let _ = mkfifo(SHELL_EVENTS, Mode::S_IRWXU);
    let buf = fs::read_to_string(SHELL_EVENTS).with_context(|| "Failed to read shell event")?;
    Ok(serde_yaml::from_str(&buf)?)
}

pub fn deinit(pid: i32) -> Result<()> {
    run(pid, "scd_deinit", &[] as &[&str], false)
}

pub const FISH_INIT: &str = r#"
function scd_eval --on-signal SIGUSR1
    eval (scd get-cmd)
end

function scd_run_silently
    eval $argv && commandline -f repaint
end

function scd_run_with_echo
    commandline $argv && commandline -f execute
end

function scd_cd --on-variable PWD
    scd cd $PWD
end

function scd_exit --on-event fish_exit
    scd exit
end

scd send-pid $fish_pid
scd_cd

function scd_deinit
    functions --erase scd_eval scd_run_silently scd_run_with_echo scd_cd scd_exit scd_deinit
end
"#;

pub const ZSH_INIT: &str = r#"
TRAPUSR1() {
    eval $(scd get-cmd)
}

scd_run_silently() {
    eval $@ && zle reset-prompt
}

scd_run_with_echo() {
    echo $@ && eval $@ && print -s $@ && zle reset-prompt
}

scd_cd() {
    scd cd $PWD
}

scd_exit() {
    scd exit
}

autoload add-zsh-hook
add-zsh-hook chpwd scd_cd
add-zsh-hook zshexit scd_exit
scd send-pid $$

scd_deinit() {
    add-zsh-hook -d chpwd scd_cd
    add-zsh-hook -d zshexit scd_exit
    unfunction TRAPUSR1 scd_run_silently scd_run_with_echo scd_cd scd_exit scd_deinit
}
"#;
