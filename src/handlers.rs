use std::time::Instant;

use anyhow::Result;
use termion::event::Key;

use crate::app::{Action, App, Mode};
use crate::shell;

pub fn handle_keys(app: &mut App, key: Key) -> Result<()> {
    match &mut app.mode {
        Mode::Normal => handle_normal_mode_keys(app, key)?,
        Mode::Message { .. } => {
            // Dismiss message when any key is pressed.
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
        Key::Char('j') | Key::Ctrl('n') | Key::Down => app.select_next(),
        Key::Char('k') | Key::Ctrl('p') | Key::Up => app.select_prev(),
        Key::Char('g') | Key::Home => app.select_first(),
        Key::Char('G') | Key::End => app.select_last(),
        Key::Char('l') | Key::Char('\n') => {
            if let Some(file) = app.selected() {
                if file.metadata.is_dir() {
                    let path = file.path.clone();
                    if let Ok(_) = app.cd(path.clone()) {
                        shell::cd(&path, app.shell_pid)?;
                    }
                } else {
                    shell::open_file(file, app)?;
                }
            }
        }
        Key::Char('h') | Key::Esc => {
            if let Some(parent) = app.dir.parent() {
                let parent = parent.to_path_buf();
                let current = app.dir.file_name().unwrap().to_str().unwrap().to_owned();
                if let Ok(_) = app.cd(parent.clone()) {
                    shell::cd(&parent, app.shell_pid)?;
                    app.select_file(current);
                }
            }
        }
        Key::Char('.') => {
            app.show_hidden = !app.show_hidden;
            app.apply_filter();
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
        Key::Char('p') => {
            let marked = app.files_marked();
            if marked.is_empty() {
                app.show_message("No marked files");
            } else {
                shell::run("cp -r {} .", &marked, app.shell_pid)?;
            }
        }
        Key::Char('m') => {
            let marked = app.files_marked();
            if marked.is_empty() {
                app.show_message("No marked files");
            } else {
                shell::run("mv {} .", &marked, app.shell_pid)?;
            }
        }
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
        Key::Down | Key::Ctrl('n') => app.select_next(),
        Key::Up | Key::Ctrl('p') => app.select_prev(),
        Key::Char('\n') => match action {
            Action::Rename(file) => {
                shell::run("mv", &[&file.name, &input], app.shell_pid)?;
                app.mode = Mode::Normal;
            }
            Action::Filter => {
                app.mode = Mode::Normal;
                app.filter.clear();
                app.apply_filter();
            }
            _ => panic!(),
        },
        Key::Esc => {
            app.mode = Mode::Normal;
            app.filter.clear();
            app.apply_filter();
        }
        Key::Backspace | Key::Ctrl('h') => {
            if *offset > 0 {
                input.remove(*offset - 1);
                *offset -= 1;
            }
            if matches!(action, Action::Filter) {
                let input = input.clone();
                app.filter = input;
                app.apply_filter();
            }
        }
        Key::Delete | Key::Ctrl('d') => {
            if *offset < input.len() {
                input.remove(*offset);
            }
            if matches!(action, Action::Filter) {
                app.filter = input.clone();
                app.apply_filter();
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
            if matches!(action, Action::Filter) {
                app.filter = input.clone();
                app.apply_filter();
            }
        }
        Key::Ctrl('a') | Key::Home => *offset = 0,
        Key::Ctrl('e') | Key::End => *offset = input.len(),
        Key::Char(ch) => {
            input.insert(*offset, ch);
            *offset += 1;
            if matches!(action, Action::Filter) {
                app.filter = input.clone();
                app.apply_filter();
            }
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
