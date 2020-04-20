use std::cmp::Ordering;
use std::fs;
use std::io;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::thread;

use fish::Fish;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use termion::{event::Key, input::TermRead, raw::IntoRawMode};
use tui::{backend::TermionBackend, layout::*, style::*, widgets::*, Terminal};

mod fish;

enum Event {
    Key(Key),
    FileSystemNotify,
    FishWorkingDirChanged(String),
}

struct FileView {
    theme: Theme,
}

impl Default for FileView {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
        }
    }
}

impl StatefulWidget for FileView {
    type State = FileViewState;

    fn render(self, area: Rect, buf: &mut tui::buffer::Buffer, state: &mut Self::State) {
        let items = state.files.iter().map(|file| self.theme.apply(file));
        let list = List::new(items).highlight_style(self.theme.highlight_style);
        StatefulWidget::render(list, area, buf, &mut state.list_state);
    }
}

struct Theme {
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

impl Default for Theme {
    fn default() -> Self {
        let base = Style::default().fg(Color::White);
        Self {
            directory: (base.fg(Color::LightBlue), Some('/')),
            executable: (base.fg(Color::LightCyan), Some('*')),
            symlink: (base, None),
            fifo: (base, None),
            socket: (base, None),
            char_device: (base, None),
            block_device: (base, None),
            other: (base, None),
            highlight_style: base.bg(Color::Blue),
        }
    }
}

impl Theme {
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

struct FileInfo {
    path: PathBuf,
    name: String,
    file_type: FileType,
}

struct FileViewState {
    dir: PathBuf,
    files: Vec<FileInfo>,
    list_state: ListState,
    show_hidden_files: bool,
    watcher: RecommendedWatcher,
    fish: Arc<Fish>,
}

impl FileViewState {
    fn new(dir: impl Into<PathBuf>, tx: SyncSender<Event>) -> FileViewState {
        let watcher = RecommendedWatcher::new_immediate({
            let tx = tx.clone();
            move |_| {
                tx.send(Event::FileSystemNotify).unwrap();
            }
        })
        .unwrap();
        let mut file_view = FileViewState {
            dir: PathBuf::new(),
            files: vec![],
            list_state: ListState::default(),
            show_hidden_files: false,
            watcher,
            fish: Fish::new(tx),
        };
        file_view.enter_directory(dir.into().canonicalize().unwrap());
        file_view
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
                Ordering::Less
            } else if a.file_type != FileType::Directory && b.file_type == FileType::Directory {
                Ordering::Greater
            } else {
                a.name.cmp(&b.name)
            }
        });
        self.files = files;
    }

    fn enter_directory(&mut self, dir: PathBuf) {
        if self.dir != PathBuf::new() {
            self.watcher.unwatch(&self.dir).unwrap();
        }
        self.dir = dir;
        self.read_dir();
        self.watcher
            .watch(&self.dir, RecursiveMode::NonRecursive)
            .unwrap();
        self.select_first();
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

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Key(Key::Char('j')) | Event::Key(Key::Down) => self.select_next(),
            Event::Key(Key::Char('k')) | Event::Key(Key::Up) => self.select_prev(),
            Event::Key(Key::Char('g')) | Event::Key(Key::Home) => self.select_first(),
            Event::Key(Key::Char('G')) | Event::Key(Key::End) => self.select_last(),
            Event::Key(Key::Char('l')) | Event::Key(Key::Char('\n')) => {
                if let Some(index) = self.list_state.selected() {
                    self.enter_directory(self.files[index].path.to_owned());
                    self.fish.send_cwd(&self.dir);
                }
            }
            Event::Key(Key::Char('h')) | Event::Key(Key::Esc) => {
                if let Some(parent) = self.dir.parent() {
                    let parent = parent.to_owned();
                    let current_dir_name =
                        self.dir.file_name().unwrap().to_str().unwrap().to_owned();
                    self.enter_directory(parent);
                    let index = self
                        .files
                        .iter()
                        .position(|file| file.name == current_dir_name);
                    self.list_state.select(index);
                    self.fish.send_cwd(&self.dir);
                }
            }
            Event::FileSystemNotify => self.read_dir(),
            Event::FishWorkingDirChanged(cwd) => self.enter_directory(cwd.into()),
            _ => {}
        }
    }
}

fn main() {
    let mut terminal = {
        let stdout = io::stdout().into_raw_mode().unwrap();
        let backend = TermionBackend::new(stdout);
        Terminal::new(backend).unwrap()
    };
    terminal.hide_cursor().unwrap();
    terminal.clear().unwrap();

    let (tx, rx) = sync_channel(0);
    let mut file_view_state = FileViewState::new(".", tx.clone());
    thread::spawn(move || {
        let keys = io::stdin().keys();
        for key in keys {
            tx.send(Event::Key(key.unwrap())).unwrap();
        }
    });
    loop {
        terminal
            .draw(|mut frame| {
                let file_view = FileView::default();
                frame.render_stateful_widget(file_view, frame.size(), &mut file_view_state);
            })
            .unwrap();

        match rx.recv().unwrap() {
            Event::Key(Key::Char('q')) => break,
            event => file_view_state.handle_event(event),
        }
    }
}
