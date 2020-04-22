use std::cmp;
use std::fs;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use crossbeam_channel::{bounded, select, Receiver};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use termion::{event::Key, input::TermRead};
use tui::{backend::Backend, Terminal};
use tui::{layout::*, style::*, widgets::*};

pub use shell::*;

mod shell;

pub struct FileInfo {
    path: PathBuf,
    name: String,
    file_type: FileType,
}

#[derive(PartialEq)]
enum FileType {
    Directory,
    Executable,
    Symlink,
    Fifo,
    Socket,
    CharDevice,
    BlockDevice,
    Other,
}

struct FileView {
    directory: (Style, Option<char>),
    executable: (Style, Option<char>),
    symlink: (Style, Option<char>),
    fifo: (Style, Option<char>),
    socket: (Style, Option<char>),
    char_device: (Style, Option<char>),
    block_device: (Style, Option<char>),
    other: (Style, Option<char>),
    highlight_style: Style,
}

impl Default for FileView {
    fn default() -> Self {
        let base = Style::default();
        Self {
            directory: (base.fg(Color::LightBlue), Some('/')),
            executable: (base.fg(Color::LightCyan), Some('*')),
            symlink: (base, Some('@')),
            fifo: (base, Some('|')),
            socket: (base, None),
            char_device: (base, None),
            block_device: (base, None),
            other: (base, None),
            highlight_style: base.bg(Color::Blue),
        }
    }
}

impl FileView {
    fn apply(&self, file: &FileInfo) -> Text {
        let (style, postfix) = match file.file_type {
            FileType::Directory => self.directory,
            FileType::Executable => self.executable,
            FileType::Symlink => self.symlink,
            FileType::Fifo => self.fifo,
            FileType::Socket => self.socket,
            FileType::CharDevice => self.char_device,
            FileType::BlockDevice => self.block_device,
            FileType::Other => self.other,
        };
        let mut name = file.name.clone();
        if let Some(postfix) = postfix {
            name.push(postfix);
        }
        Text::styled(name, style)
    }
}

impl StatefulWidget for FileView {
    type State = FileViewState;

    fn render(self, area: Rect, buf: &mut tui::buffer::Buffer, state: &mut Self::State) {
        let items = state.files.iter().map(|file| self.apply(file));
        let list = List::new(items).highlight_style(self.highlight_style);
        StatefulWidget::render(list, area, buf, &mut state.list_state);
    }
}

struct FileViewState {
    dir: PathBuf,
    files: Vec<FileInfo>,
    list_state: ListState,
    show_hidden_files: bool,
}

impl FileViewState {
    fn new() -> FileViewState {
        FileViewState {
            dir: PathBuf::new(),
            files: vec![],
            list_state: ListState::default(),
            show_hidden_files: false,
        }
    }

    fn read_dir(&mut self) {
        let entries = fs::read_dir(&self.dir)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
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
    }

    fn selected(&self) -> Option<&FileInfo> {
        self.list_state.selected().map(|index| &self.files[index])
    }

    fn select_first(&mut self) {
        let index = if self.files.len() == 0 { None } else { Some(0) };
        self.list_state.select(index);
    }

    fn select_last(&mut self) {
        let index = match self.files.len() {
            0 => None,
            len => Some(len - 1),
        };
        self.list_state.select(index);
    }

    fn select_next(&mut self) {
        let index = match self.list_state.selected() {
            None => 0,
            Some(i) => (i + 1) % self.files.len(),
        };
        self.list_state.select(Some(index));
    }

    fn select_prev(&mut self) {
        let index = match self.list_state.selected() {
            None => 0,
            Some(0) => self.files.len() - 1,
            Some(i) => i - 1,
        };
        self.list_state.select(Some(index));
    }
}

pub struct FileManager {
    watcher: RecommendedWatcher,
    file_view_state: FileViewState,
    shell: Arc<Shell>,

    watch_rx: Receiver<notify::Event>,
    key_rx: Receiver<Key>,
    shell_rx: Receiver<ShellEvent>,
}

impl FileManager {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let (watch_tx, watch_rx) = bounded(0);
        let watcher =
            RecommendedWatcher::new_immediate(move |res: notify::Result<notify::Event>| {
                watch_tx.send(res.unwrap()).unwrap();
            })
            .unwrap();

        let (key_tx, key_rx) = bounded(0);
        thread::spawn(move || {
            let keys = io::stdin().keys();
            for key in keys {
                key_tx.send(key.unwrap()).unwrap();
            }
        });

        let (shell_tx, shell_rx) = bounded(0);
        let shell = Shell::new(shell_tx);

        let file_view_state = FileViewState::new();

        let mut app = Self {
            watcher,
            file_view_state,
            shell,

            watch_rx,
            key_rx,
            shell_rx,
        };
        app.enter_directory(dir.into().canonicalize().unwrap());

        app
    }

    fn enter_directory(&mut self, dir: PathBuf) {
        if self.file_view_state.dir != PathBuf::new() {
            self.watcher.unwatch(&self.file_view_state.dir).unwrap();
        }
        self.file_view_state.dir = dir;
        self.file_view_state.read_dir();
        if self.file_view_state.files.len() > 0 {
            self.file_view_state.list_state.select(Some(0));
        }
        self.watcher
            .watch(&self.file_view_state.dir, RecursiveMode::NonRecursive)
            .unwrap();
    }

    pub fn draw<B: Backend>(&mut self, terminal: &mut Terminal<B>) {
        terminal
            .draw(|mut frame| {
                let file_view = FileView::default();
                frame.render_stateful_widget(file_view, frame.size(), &mut self.file_view_state);
            })
            .unwrap();
    }

    pub fn handle_event(&mut self) {
        select! {
            recv(self.watch_rx) -> _watch => self.file_view_state.read_dir(),
            recv(self.shell_rx) -> shell_event => {
                match shell_event.unwrap() {
                    ShellEvent::ChangeDirectory(dir) => {
                        if dir != self.file_view_state.dir {
                            self.enter_directory(dir);
                        }
                    }
                    ShellEvent::Exit => std::process::exit(0),
                    _ => {}
                }
            }
            recv(self.key_rx) -> key => {
                match key.unwrap() {
                    Key::Char('j') | Key::Down => self.file_view_state.select_next(),
                    Key::Char('k') | Key::Up => self.file_view_state.select_prev(),
                    Key::Char('g') | Key::Home => self.file_view_state.select_first(),
                    Key::Char('G') | Key::End => self.file_view_state.select_last(),
                    Key::Char('l') | Key::Char('\n') => {
                        if let Some(selected) = self.file_view_state.selected() {
                            if selected.file_type == FileType::Directory {
                                let dir = selected.path.clone();
                                self.enter_directory(dir);
                                self.shell.cd(&self.file_view_state.dir);
                            } else {
                                self.shell.open_file(&selected);
                            }
                        }
                    }
                    Key::Char('h') | Key::Esc => {
                        if let Some(parent) = self.file_view_state.dir.parent() {
                            let parent = parent.to_owned();
                            let current_dir_name = self
                                .file_view_state
                                .dir
                                .file_name()
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .to_owned();
                            self.enter_directory(parent);
                            let index = self
                                .file_view_state
                                .files
                                .iter()
                                .position(|file| file.name == current_dir_name);
                            self.file_view_state.list_state.select(index);
                            self.shell.cd(&self.file_view_state.dir);
                        }
                    }
                    Key::Char('q') => std::process::exit(0),
                    _ => {}
                }
            }
        }
    }
}
