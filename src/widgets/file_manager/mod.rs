use std::cmp;
use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::Result;
use crossbeam_channel::{bounded, Receiver};
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{List, ListState, Paragraph, StatefulWidget, Text, Widget};

use icons::*;
pub use shell::*;

mod icons;
pub mod shell;

pub struct FileInfo {
    pub path: PathBuf,
    pub name: String,
    pub file_type: FileType,
}

#[derive(PartialEq)]
pub enum FileType {
    Directory,
    Executable,
    Symlink,
    Fifo,
    Socket,
    CharDevice,
    BlockDevice,
    Other,
}

pub struct FileView;

impl StatefulWidget for FileView {
    type State = FileViewState;

    fn render(self, area: Rect, buf: &mut tui::buffer::Buffer, state: &mut Self::State) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(2), Constraint::Min(0)].as_ref())
            .split(area);
        Paragraph::new(
            [Text::styled(
                state.dir.to_str().unwrap(),
                Style::default().modifier(Modifier::UNDERLINED),
            )]
            .iter(),
        )
        .render(chunks[0], buf);
        let items = state
            .files
            .iter()
            .map(|file| {
                let color = match file.file_type {
                    FileType::Directory => Color::Blue,
                    FileType::Executable => Color::Green,
                    _ => Color::White,
                };
                let icon = state.icons.get(file);
                let postfix = match file.file_type {
                    FileType::Directory => "/",
                    FileType::Executable => "*",
                    FileType::Fifo => "|",
                    FileType::Symlink => "@",
                    _ => "",
                };
                Text::styled(
                    format!("{}  {}{}", icon, file.name, postfix),
                    Style::default().fg(color),
                )
            })
            .collect::<Vec<_>>()
            .into_iter();
        let list =
            List::new(items).highlight_style(Style::default().fg(Color::Black).bg(Color::Blue));
        StatefulWidget::render(list, chunks[1], buf, &mut state.list_state);
    }
}

pub struct FileViewState {
    pub dir: PathBuf,
    pub files: Vec<FileInfo>,
    pub list_state: ListState,
    pub show_hidden_files: bool,
    icons: Icons,
}

impl FileViewState {
    pub fn new() -> FileViewState {
        FileViewState {
            dir: PathBuf::new(),
            files: vec![],
            list_state: ListState::default(),
            show_hidden_files: false,
            icons: Icons::new(),
        }
    }

    pub fn read_dir(&mut self) -> Result<()> {
        let entries = fs::read_dir(&self.dir)?.collect::<Result<Vec<_>, _>>()?;
        let mut files: Vec<FileInfo> = entries
            .into_iter()
            .map(|entry| {
                let path = entry.path();
                let name = entry.file_name().to_str().unwrap().to_string();
                let file_type = {
                    let file_type = entry.file_type().unwrap();
                    if file_type.is_dir() {
                        FileType::Directory
                    } else if file_type.is_symlink() {
                        FileType::Symlink
                    } else if file_type.is_fifo() {
                        FileType::Fifo
                    } else if file_type.is_socket() {
                        FileType::Socket
                    } else if file_type.is_char_device() {
                        FileType::CharDevice
                    } else if file_type.is_block_device() {
                        FileType::BlockDevice
                    } else if entry.metadata().unwrap().permissions().mode() & 0o1 != 0 {
                        FileType::Executable
                    } else {
                        FileType::Other
                    }
                };
                FileInfo {
                    path,
                    name,
                    file_type,
                }
            })
            .filter(|file| self.show_hidden_files || !file.name.starts_with('.'))
            .collect();

        files.sort_unstable_by(|a, b| {
            if a.file_type == FileType::Directory && b.file_type != FileType::Directory {
                cmp::Ordering::Less
            } else if a.file_type != FileType::Directory && b.file_type == FileType::Directory {
                cmp::Ordering::Greater
            } else {
                a.name.cmp(&b.name)
            }
        });
        self.files = files;
        Ok(())
    }

    pub fn selected(&self) -> Option<&FileInfo> {
        self.list_state.selected().map(|index| &self.files[index])
    }

    pub fn select_first(&mut self) {
        let index = if self.files.len() == 0 { None } else { Some(0) };
        self.list_state.select(index);
    }

    pub fn select_last(&mut self) {
        let index = match self.files.len() {
            0 => None,
            len => Some(len - 1),
        };
        self.list_state.select(index);
    }

    pub fn select_next(&mut self) {
        let index = match self.list_state.selected() {
            None => 0,
            Some(i) => (i + 1) % self.files.len(),
        };
        self.list_state.select(Some(index));
    }

    pub fn select_prev(&mut self) {
        let index = match self.list_state.selected() {
            None => 0,
            Some(0) => self.files.len() - 1,
            Some(i) => i - 1,
        };
        self.list_state.select(Some(index));
    }
}
