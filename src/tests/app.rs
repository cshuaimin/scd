use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;

use anyhow::Result;
use notify::{NullWatcher, Watcher};
use tempfile::tempdir;

use crate::app::*;

#[test]
fn test_toggle_hidden_files() -> Result<()> {
    let temp = tempdir()?;
    File::create(temp.path().join(".hidden"))?;
    File::create(temp.path().join("show"))?;
    let mut app = App::new(NullWatcher::new_immediate(|_| {})?, temp.path())?;

    app.show_hidden = false;
    app.apply_filter();
    assert_eq!(app.file_names(), ["show"]);

    app.show_hidden = true;
    app.apply_filter();
    assert_eq!(app.file_names(), [".hidden", "show"]);

    Ok(())
}

#[test]
fn test_filter() -> Result<()> {
    let temp = tempdir()?;
    File::create(temp.path().join("abc"))?;
    File::create(temp.path().join("bcd"))?;
    let mut app = App::new(NullWatcher::new_immediate(|_| {})?, temp.path())?;

    app.filter = "ab".to_string();
    app.apply_filter();
    assert_eq!(app.file_names(), ["abc"]);

    app.filter = "bc".to_string();
    app.apply_filter();
    assert_eq!(app.file_names(), ["abc", "bcd"]);

    Ok(())
}

#[test]
fn test_selection_after_filter() -> Result<()> {
    let temp = tempdir()?;
    File::create(temp.path().join("a"))?;
    File::create(temp.path().join("b"))?;
    File::create(temp.path().join("c"))?;
    let mut app = App::new(NullWatcher::new_immediate(|_| {})?, temp.path())?;

    app.filter = "b".to_string();
    app.apply_filter();
    app.filter = "".to_string();
    app.apply_filter();
    assert_eq!(app.list_state.selected(), Some(1));

    Ok(())
}

#[test]
fn test_cd() -> Result<()> {
    let temp = tempdir()?;
    let dir1 = temp.path().join("dir1");
    let dir2 = temp.path().join("dir2");
    fs::create_dir(&dir1)?;
    fs::create_dir(&dir2)?;
    File::create(dir1.join("file1"))?;
    File::create(dir2.join("file2"))?;
    let mut app = App::new(NullWatcher::new_immediate(|_| {})?, temp.path())?;

    app.cd(dir1)?;
    assert_eq!(app.file_names(), ["file1"]);

    app.cd(dir2)?;
    assert_eq!(app.file_names(), ["file2"]);

    Ok(())
}

#[test]
fn test_permission_denied() -> Result<()> {
    let temp = tempdir()?;
    let dir = temp.path().join("d");
    fs::create_dir(&dir)?;
    File::create(dir.join("f"))?;
    let mut perm = fs::metadata(&dir)?.permissions();
    perm.set_mode(0);
    fs::set_permissions(&dir, perm)?;

    let mut app = App::new(NullWatcher::new_immediate(|_| {})?, temp.path())?;
    assert_eq!(app.file_names(), ["d"]);

    assert!(app.cd(dir.clone()).is_err());
    assert_eq!(app.dir, temp.path());
    assert_eq!(app.file_names(), ["d"]);
    assert!(matches!(app.mode, Mode::Message {..}));

    // Restore the mode, so it can be removed.
    let mut perm = fs::metadata(&dir)?.permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&dir, perm)?;

    Ok(())
}
