use std::time::Instant;

use anyhow::Result;
use termion::event::Key;

use crate::app::{Action, App, Mode};
use crate::shell;

pub fn handle_keys(app: &mut App, key: Key) -> Result<()> {
    match &mut app.mode {
        Mode::Normal => handle_normal_mode_keys(app, key)?,
        Mode::Message { .. } => {
            app.mode = Mode::Normal;
            handle_normal_mode_keys(app, key)?;
        }
        Mode::Ask { action, .. } => match action {
            Action::Delete(file) => match key {
                Key::Char('y') => {
                    shell::run("rm -r", &[&file.name], app.shell_pid)?;
                    app.mode = Mode::Normal;
                }
                _ => app.mode = Mode::Normal,
            },
            _ => panic!("Unknown action {:?}", action),
        },
        Mode::Input { .. } => handle_input_mode_keys(app, key)?,
    }
    Ok(())
}

fn handle_normal_mode_keys(app: &mut App, key: Key) -> Result<()> {
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
        Key::Char('m') => shell::run("mv {} .", &app.files_marked(), app.shell_pid)?,
        Key::Char('d') => {
            if let Some(file) = app.selected() {
                let tp = match file.metadata.is_dir() {
                    true => "directory",
                    false => "file",
                };
                app.mode = Mode::Ask {
                    prompt: format!("Delete {} {}? [y/N]", tp, file.name),
                    action: Action::Delete(file.clone()),
                };
            }
        }
        Key::Char('r') => {
            if let Some(file) = app.selected() {
                app.mode = Mode::Input {
                    prompt: "Rename: ".to_string(),
                    input: file.name.clone(),
                    offset: file.name.len(),
                    action: Action::Rename(file.clone()),
                };
            }
        }
        Key::Char('/') => {
            app.mode = Mode::Input {
                prompt: "/".to_string(),
                input: "".to_string(),
                offset: 0,
                action: Action::Filter,
            };
        }
        uk => app.show_message(&format!("Unknown key: {:?}", uk)),
    }
    Ok(())
}

fn handle_input_mode_keys(app: &mut App, key: Key) -> Result<()> {
    let (input, offset, action) = match &mut app.mode {
        Mode::Input {
            input,
            offset,
            action,
            ..
        } => (input, offset, action),
        _ => panic!(),
    };
    match key {
        Key::Char('\n') => match action {
            Action::Rename(file) => {
                shell::run("mv", &[&file.name, &input], app.shell_pid)?;
                app.mode = Mode::Normal;
            }
            Action::Filter => {
                app.filter = input.clone();
                app.filter_files();
            }
            _ => panic!(),
        },
        Key::Esc | Key::Ctrl('[') => app.mode = Mode::Normal,
        Key::Backspace | Key::Ctrl('h') => {
            if *offset > 0 {
                input.remove(*offset - 1);
                *offset -= 1;
            }
        }
        Key::Delete | Key::Ctrl('d') => {
            if *offset < input.len() {
                input.remove(*offset);
            }
        }
        Key::Left | Key::Ctrl('b') => {
            if *offset > 0 {
                *offset -= 1;
            }
        }
        Key::Right | Key::Ctrl('f') => {
            if *offset < input.len() {
                *offset += 1;
            }
        }
        Key::Ctrl('u') => {
            input.clear();
            *offset = 0;
        }
        Key::Ctrl('a') => *offset = 0,
        Key::Ctrl('e') => *offset = input.len(),
        Key::Char(ch) => {
            input.insert(*offset as usize, ch);
            *offset += 1;
        }
        _ => {}
    }
    Ok(())
}

pub fn handle_tick(app: &mut App, tick: Instant) {
    app.update_on_tick();
    if let Mode::Message { expire_at, .. } = app.mode {
        if expire_at >= tick {
            app.mode = Mode::Normal;
        }
    }
}
