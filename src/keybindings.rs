use anyhow::Result;
use termion::event::Key;

use crate::app::App;
use crate::shell;

pub fn handle_keys(app: &mut App, key: Key) -> Result<()> {
    match key {
        Key::Char('j') | Key::Down => app.select_next(),
        Key::Char('k') | Key::Up => app.select_prev(),
        Key::Char('g') | Key::Home => app.select_first(),
        Key::Char('G') | Key::End => app.select_last(),
        Key::Char('l') | Key::Char('\n') => {
            if let Some(file) = app.selected() {
                if file.metadata.is_dir() {
                    let path = file.path.clone();
                    shell::cd(&path, app.shell_pid)?;
                    app.cd(path)?;
                } else {
                    shell::open_file(file, app)?;
                }
            }
        }
        Key::Char('h') | Key::Esc => {
            if let Some(parent) = app.dir.parent() {
                let parent = parent.to_path_buf();
                let current = app.dir.file_name().unwrap().to_str().unwrap().to_owned();
                shell::cd(&parent, app.shell_pid)?;
                app.cd(parent)?;
                let index = app.files.iter().position(|f| f.name == current);
                app.list_state.select(index);
            }
        }
        Key::Char('.') => {
            let selected = app.selected().map(|f| f.name.clone());
            app.show_hidden = !app.show_hidden;
            app.filter_files();
            if let Some(name) = selected {
                let index = app.files.iter().position(|f| f.name == name).unwrap_or(0);
                app.list_state.select(Some(index));
            }
        }
        Key::Char(' ') => {
            if let Some(file) = app.selected() {
                if let Some(index) = app.files_marked.iter().position(|p| p == &file.path) {
                    app.files_marked.remove(index);
                } else {
                    let path = file.path.clone();
                    app.files_marked.push(path);
                }
                if app.list_state.selected().unwrap() != app.files.len() - 1 {
                    app.select_next();
                }
            }
        }
        Key::Char('p') => shell::run("cp -r {} .", &app.files_marked(), app.shell_pid)?,
        Key::Char('m') => shell::run("mv -r {} .", &app.files_marked(), app.shell_pid)?,
        _ => {}
    }
    Ok(())
}
