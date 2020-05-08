use std::fs::{self, File};

use anyhow::Result;
use notify::{NullWatcher, Watcher};
use tempfile::tempdir;
use termion::event::Key;

use crate::app::*;
use crate::handlers::*;

#[test]
fn test_pid_0() -> Result<()> {
    let temp = tempdir()?;
    File::create(temp.path().join("f"))?;
    fs::create_dir(temp.path().join("d"))?;
    let mut app = App::new(NullWatcher::new_immediate(|_| {})?, temp.path())?;

    let res = handle_keys(&mut app, Key::Char('l'));
    assert!(res.is_ok());

    handle_keys(&mut app, Key::Char('h'))?;
    handle_keys(&mut app, Key::Char('j'))?;
    let res = handle_keys(&mut app, Key::Char('l'));
    assert!(res.is_err());

    let res = handle_keys(&mut app, Key::Char('r'));
    assert!(res.is_err());

    let res = handle_keys(&mut app, Key::Char('d'));
    assert!(res.is_err());

    Ok(())
}
