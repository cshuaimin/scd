use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{mpsc::SyncSender, Arc};
use std::thread;

use nix::sys::signal::{kill, Signal};
use nix::sys::stat::Mode;
use nix::unistd::{mkfifo, Pid};

use super::Event;

const RECV_FIFO: &str = "/tmp/scd-recv-fifo";
const SEND_FILE: &str = "/tmp/scd-send";

pub(crate) struct Fish {
    pid: AtomicI32,
}

impl Fish {
    pub(crate) fn new(tx: SyncSender<Event>) -> Arc<Self> {
        let fish = Arc::new(Self {
            pid: AtomicI32::new(0),
        });

        thread::spawn({
            let fish = Arc::clone(&fish);
            move || {
                let _ = mkfifo(RECV_FIFO, Mode::S_IRWXU);

                loop {
                    let mut recv_fifo = File::open(RECV_FIFO).unwrap();
                    let mut buf = String::new();
                    recv_fifo.read_to_string(&mut buf).unwrap();
                    for cmd in buf.trim().split('\n') {
                        if cmd == "fish_exit" {
                            tx.send(Event::FishExited).unwrap();
                        } else if cmd.starts_with("cd ") {
                            tx.send(Event::FishWorkingDirChanged(cmd[3..].to_string()))
                                .unwrap();
                        } else {
                            let pid = cmd.parse::<i32>().unwrap();
                            fish.pid.store(pid, Ordering::Release);
                        }
                    }
                }
            }
        });

        fish
    }

    pub(crate) fn send_cwd(&self, cwd: &Path) {
        let pid = self.pid.load(Ordering::Acquire);
        if pid > 0 {
            let mut send_file = OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .open(SEND_FILE)
                .unwrap();
            let cwd = cwd.to_str().unwrap().as_bytes();
            send_file.write_all(cwd).unwrap();
            kill(Pid::from_raw(pid), Signal::SIGUSR1).unwrap();
        }
    }
}
