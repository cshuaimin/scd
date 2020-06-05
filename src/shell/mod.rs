use std::fs::{File, OpenOptions};
use std::io::prelude::*;
use std::mem;
use std::path::PathBuf;

use anyhow::{ensure, Context, Result};
use crossbeam_channel::Sender;
use nix::sys::signal::{kill, Signal};
use nix::sys::stat::Mode;
use nix::unistd::{mkfifo, Pid};
use serde::{Deserialize, Serialize};

/// Send shell commands from scd to shell.
pub const CMDS_TO_RUN: &str = "/tmp/scd-cmds-to-run";

/// Send shell events from the shell to scd.
pub const SHELL_EVENTS: &str = "/tmp/scd-shell-events";

pub const FISH_INIT: &str = include_str!("scd.fish");
pub const ZSH_INIT: &str = include_str!("scd.zsh");

/// Events emitted from the shell.
#[derive(Debug, Serialize, Deserialize)]
pub enum Event {
    /// Shell PID.
    Pid(i32),

    /// The shell's current directory was changed.
    ChangeDirectory(PathBuf),

    /// Shell exited.
    Exit,

    /// Run and montior the task.
    Task { command: String, rendered: String },
}

fn write(mut file: impl Write, buf: impl AsRef<[u8]>) -> Result<()> {
    let buf = buf.as_ref();
    file.write_all(&buf.len().to_ne_bytes())?;
    file.write_all(buf)?;
    Ok(())
}

fn read(mut file: impl Read) -> Result<String> {
    let mut len_bytes = [0; mem::size_of::<usize>()];
    file.read_exact(&mut len_bytes)?;
    let len = usize::from_ne_bytes(len_bytes);
    let mut buf = vec![0; len];
    file.read_exact(&mut buf)?;
    let s = String::from_utf8(buf)?;
    Ok(s)
}

/// Run a command in the shell.
pub fn run(pid: Pid, cmd: &str, args: &[impl AsRef<str>], echo: bool) -> Result<()> {
    ensure!(pid.as_raw() > 0, "shell not initialized");
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

    kill(pid, Signal::SIGUSR1).with_context(|| "Failed to notify the shell")?;
    let mut fifo = OpenOptions::new().write(true).open(CMDS_TO_RUN)?;
    write(fifo, cmd)
}
/// Receive a shell command to run.
///
/// This function is called on the shell side.
pub fn receive_command() -> Result<String> {
    let _ = mkfifo(CMDS_TO_RUN, Mode::S_IRWXU);
    let file = File::open(CMDS_TO_RUN)?;
    read(file)
}

/// Send a shell event to the file manager.
///
/// This function is called on the shell side.
pub fn send_event(event: Event) -> Result<()> {
    let _ = mkfifo(SHELL_EVENTS, Mode::S_IRWXU);
    let fifo = OpenOptions::new().write(true).open(SHELL_EVENTS)?;
    write(fifo, &serde_yaml::to_vec(&event)?)
}

pub fn receive_events(tx: Sender<Event>) -> Result<()> {
    let _ = mkfifo(SHELL_EVENTS, Mode::S_IRWXU);
    loop {
        let fifo = File::open(SHELL_EVENTS)?;
        let event = serde_yaml::from_str(&read(fifo)?)?;
        tx.send(event)?;
    }
}

pub fn deinit(pid: Pid) -> Result<()> {
    run(pid, "scd_deinit", &[""], false)
}
