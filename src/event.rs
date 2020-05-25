use std::io;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_channel::{bounded, tick, Receiver, Sender};
use notify::{RecommendedWatcher, Watcher};
use termion::event::Key;
use termion::input::TermRead;

use crate::shell;
use crate::task;

pub enum Event {
    Watch(notify::Event),
    Shell(shell::Event),
    Key(Key),
    Tick(Instant),
    Task{ pid: u32, event: task::Event },
}

pub struct Events {
    pub tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl Events {
    pub fn new() -> Result<(Self, RecommendedWatcher)> {
        let (tx, rx) = bounded(0);
        let watch_tx = tx.clone();
        let watcher =
            RecommendedWatcher::new_immediate(move |res: notify::Result<notify::Event>| {
                let event = Event::Watch(res.unwrap());
                watch_tx.send(event).unwrap();
            })?;

        thread::spawn({
            let tx = tx.clone();
            move || loop {
                let event = shell::receive_event().unwrap();
                tx.send(Event::Shell(event)).unwrap();
            }
        });

        thread::spawn({
            let tx = tx.clone();
            move || {
                io::stdin()
                    .keys()
                    .map(Result::unwrap)
                    .map(Event::Key)
                    .for_each(|k| tx.send(k).unwrap())
            }
        });

        thread::spawn({
            let tx = tx.clone();
            move || {
                tick(Duration::from_secs(2))
                    .into_iter()
                    .map(Event::Tick)
                    .for_each(|t| tx.send(t).unwrap())
            }
        });
        Ok((Self { tx, rx }, watcher))
    }

    pub fn next(&self) -> Result<Event> {
        Ok(self.rx.recv()?)
    }
}
