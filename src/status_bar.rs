use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::Watcher;
use strmode::strmode;
use termion::event::Key;
use tui::backend::Backend;
use tui::layout::{Alignment, Rect};
use tui::style::{Color, Style};
use tui::widgets::{Paragraph, Text};
use tui::Frame;

use crate::app::ListExt;
use crate::file_manager::FileManager;
use crate::task_manager::TaskManager;

pub enum Mode {
    /// Show some properties of selected file/task.
    Normal,

    /// Display a short lived message.
    Message { text: String, expire_at: Instant },

    /// Ask a yes/no question.
    Ask {
        prompt: String,
        on_yes: Box<dyn Fn(&mut FileManager, &mut TaskManager) -> Result<()>>,
    },

    /// Edit some text.
    Edit {
        prompt: String,
        text: String,
        cursor: usize,
        on_change: Box<dyn Fn(&str, &mut FileManager, &mut TaskManager) -> Result<()>>,
        on_enter: Box<dyn Fn(&str, &mut FileManager, &mut TaskManager) -> Result<()>>,
    },
}

pub struct StatusBar {
    pub mode: Mode,
}

impl StatusBar {
    pub fn new() -> StatusBar {
        StatusBar { mode: Mode::Normal }
    }

    pub fn show_message(&mut self, text: impl Into<String>) {
        self.mode = Mode::Message {
            text: text.into(),
            expire_at: Instant::now() + Duration::from_secs(4),
        };
    }

    pub fn ask(
        &mut self,
        prompt: impl Into<String>,
        on_yes: impl Fn(&mut FileManager, &mut TaskManager) -> Result<()> + 'static,
    ) {
        self.mode = Mode::Ask {
            prompt: prompt.into(),
            on_yes: Box::new(on_yes),
        }
    }

    pub fn edit(
        &mut self,
        prompt: impl Into<String>,
        text: impl Into<String>,
        on_change: impl Fn(&str, &mut FileManager, &mut TaskManager) -> Result<()> + 'static,
        on_enter: impl Fn(&str, &mut FileManager, &mut TaskManager) -> Result<()> + 'static,
    ) {
        let text = text.into();
        let cursor = text.len();
        self.mode = Mode::Edit {
            prompt: prompt.into(),
            text,
            cursor,
            on_change: Box::new(on_change),
            on_enter: Box::new(on_enter),
        };
    }

    pub fn on_tick(&mut self, now: Instant) {
        if let Mode::Message { expire_at, .. } = self.mode {
            if expire_at >= now {
                self.mode = Mode::Normal;
            }
        }
    }

    pub fn on_key(
        &mut self,
        key: Key,
        file_manager: &mut FileManager,
        task_manager: &mut TaskManager,
    ) -> Result<()> {
        match &mut self.mode {
            Mode::Normal => panic!(),
            Mode::Message { .. } => self.mode = Mode::Normal,
            Mode::Ask { on_yes, .. } => {
                if key == Key::Char('y') {
                    on_yes(file_manager, task_manager)?;
                }
                self.mode = Mode::Normal;
            }
            Mode::Edit {
                text,
                cursor,
                on_change,
                on_enter,
                ..
            } => match key {
                Key::Char('\n') => {
                    on_enter(text, file_manager, task_manager)?;
                    self.mode = Mode::Normal;
                }
                Key::Esc | Key::Ctrl('[') => {
                    on_change("", file_manager, task_manager)?;
                    self.mode = Mode::Normal;
                }

                Key::Home | Key::Ctrl('a') => *cursor = 0,
                Key::End | Key::Ctrl('e') => *cursor = text.len(),
                Key::Left | Key::Ctrl('b') => {
                    if *cursor > 0 {
                        *cursor -= 1;
                    }
                }
                Key::Right | Key::Ctrl('f') => {
                    if *cursor < text.len() {
                        *cursor += 1;
                    }
                }

                Key::Backspace | Key::Ctrl('h') => {
                    if *cursor > 0 {
                        text.remove(*cursor - 1);
                        *cursor -= 1;
                        on_change(text, file_manager, task_manager)?;
                    }
                }
                Key::Delete | Key::Ctrl('d') => {
                    if *cursor < text.len() {
                        text.remove(*cursor);
                        on_change(text, file_manager, task_manager)?;
                    }
                }
                Key::Ctrl('u') => {
                    text.clear();
                    *cursor = 0;
                    on_change(text, file_manager, task_manager)?;
                }

                Key::Char(ch) => {
                    text.insert(*cursor, ch);
                    *cursor += 1;
                    on_change(text, file_manager, task_manager)?;
                }
                _ => {}
            },
        }
        Ok(())
    }

    pub fn draw(
        &self,
        file_manager: &FileManager<impl Watcher>,
        frame: &mut Frame<impl Backend>,
        area: Rect,
    ) {
        let prompt_style = Style::default().fg(Color::LightYellow);
        match &self.mode {
            Mode::Normal => {
                if let Some(file) = file_manager.selected() {
                    let mode = strmode(file.metadata.permissions().mode());
                    let size = format_size(file.metadata.len());
                    let texts = [
                        Text::styled(mode, Style::default().fg(Color::LightGreen)),
                        Text::raw(" "),
                        Text::raw(size),
                    ];
                    frame.render_widget(
                        Paragraph::new(texts.iter()).alignment(Alignment::Left),
                        area,
                    );
                }

                let mut text = String::new();
                if !file_manager.files_marked.is_empty() {
                    text.push_str("M:");
                    text.push_str(&file_manager.files_marked.len().to_string());
                }
                text.push_str(&format!(
                    " {}/{}",
                    file_manager
                        .list_state
                        .selected()
                        .map(|i| i + 1)
                        .unwrap_or(0),
                    file_manager.files.len()
                ));
                frame.render_widget(
                    Paragraph::new([Text::raw(text)].iter()).alignment(Alignment::Right),
                    area,
                );
            }
            Mode::Message { text, .. } => {
                let texts = [Text::styled(text, prompt_style)];
                frame.render_widget(Paragraph::new(texts.iter()), area);
            }
            Mode::Ask { prompt, .. } => {
                let texts = [Text::styled(prompt, prompt_style)];
                frame.render_widget(Paragraph::new(texts.iter()), area);
            }
            Mode::Edit { prompt, text, .. } => {
                let texts = [
                    Text::styled(prompt, prompt_style),
                    Text::styled(text, Style::default().fg(Color::LightCyan)),
                ];
                frame.render_widget(Paragraph::new(texts.iter()), area);
            }
        }
    }
}

fn format_size(size: u64) -> String {
    const UNITS: &[(u64, &str)] = &[
        (1024 * 1024 * 1024 * 1024, "T"),
        (1024 * 1024 * 1024, "G"),
        (1024 * 1024, "M"),
        (1024, "K"),
        (1, "B"),
    ];

    for &(div, unit) in UNITS {
        if size >= div {
            return format!("{:.1}{}", size as f32 / div as f32, unit);
        }
    }
    "0B".to_string()
}
