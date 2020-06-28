use std::cmp;
use std::convert::TryFrom;
use std::env;
use std::fs::{self, DirEntry, Metadata};
use std::io;
use std::mem;
use std::os::unix::fs::PermissionsExt;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Result;
use crossbeam_channel::{self as channel, Receiver};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use termion::event::Key;
use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Color, Modifier, Style};
use tui::widgets::{List, ListState, Paragraph, Text};
use tui::Frame;

use crate::app::ListExt;
use crate::shell;
use crate::status_bar::StatusBar;
use nix::unistd::Pid;

#[derive(Debug, Clone)]
pub struct FileInfo {
    pub path: PathBuf,
    pub name: String,
    pub extension: Option<String>,
    pub metadata: Metadata,
}

impl TryFrom<DirEntry> for FileInfo {
    type Error = io::Error;

    fn try_from(entry: DirEntry) -> Result<Self, Self::Error> {
        let path = entry.path();
        let name = entry.file_name().to_str().unwrap().to_owned();
        let extension = path.extension().map(|e| e.to_str().unwrap().to_owned());
        Ok(Self {
            path,
            name,
            extension,
            metadata: entry.metadata()?,
        })
    }
}

pub struct FileManager<W = RecommendedWatcher>
where
    W: Watcher,
{
    dir: PathBuf,
    all_files: Vec<FileInfo>,
    pub files: Vec<FileInfo>, // filtered
    pub files_marked: Vec<PathBuf>,
    pub filter: String,
    show_hidden: bool,
    pub list_state: ListState,
    watcher: W,
    pub shell_pid: Pid,
    open_methods: HashMap<String, String>,
}

impl<W> FileManager<W>
where
    W: Watcher,
{
    pub fn new() -> Result<(FileManager<W>, Receiver<notify::Event>)> {
        let (tx, rx) = channel::bounded(0);
        let watcher = W::new_immediate(move |event: notify::Result<notify::Event>| {
            tx.send(event.unwrap()).unwrap()
        })?;

        let mut file_manager = FileManager {
            dir: PathBuf::new(),
            all_files: vec![],
            files: vec![],
            files_marked: vec![],
            filter: "".to_string(),
            show_hidden: false,
            list_state: ListState::default(),
            watcher,
            shell_pid: Pid::from_raw(0),
            open_methods: load_open_methods()?,
        };
        file_manager.cd(env::current_dir()?)?;

        Ok((file_manager, rx))
    }

    pub fn cd(&mut self, mut dir: PathBuf) -> Result<()> {
        if dir != self.dir {
            if self.dir != Path::new("") {
                self.watcher.unwatch(&self.dir)?;
            }
            mem::swap(&mut self.dir, &mut dir);
            match self.read_dir() {
                Ok(res) => {
                    self.all_files = res;
                    self.apply_filter();
                    self.select_first();
                    self.watcher.watch(&self.dir, RecursiveMode::NonRecursive)?;
                }
                Err(e) => {
                    self.dir = dir;
                    self.watcher.watch(&self.dir, RecursiveMode::NonRecursive)?;
                    return Err(e.into());
                }
            }
        }
        Ok(())
    }

    pub fn read_dir(&self) -> io::Result<Vec<FileInfo>> {
        let mut res = vec![];
        for entry in fs::read_dir(&self.dir)? {
            res.push(FileInfo::try_from(entry?)?);
        }
        res.sort_unstable_by(|a, b| match (a.metadata.is_dir(), b.metadata.is_dir()) {
            (true, false) => cmp::Ordering::Less,
            (false, true) => cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });
        Ok(res)
    }

    pub fn apply_filter(&mut self) {
        let selected = self.selected().map(|f| f.name.clone());
        self.files = self
            .all_files
            .iter()
            .filter(|f| self.show_hidden || !f.name.starts_with('.'))
            .filter(|f| f.name.to_lowercase().contains(&self.filter.to_lowercase()))
            .cloned()
            .collect();

        // Keep selection after filter.
        if let Some(name) = selected {
            self.select_file(name);
        }
    }

    pub fn select_file(&mut self, name: String) {
        let index = self.files.iter().position(|f| f.name == name).unwrap_or(0);
        self.list_state.select(Some(index));
    }

    pub fn on_notify(&mut self, event: notify::Event) -> io::Result<()> {
        match event.kind {
            EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_) => {
                self.read_dir().map(|res| {
                    self.all_files = res;
                    self.apply_filter();
                })
            }
            _ => Ok(()),
        }
    }

    pub fn on_shell_event(&mut self, shell_event: shell::Event) -> Result<()> {
        match shell_event {
            shell::Event::Pid(pid) => self.shell_pid = Pid::from_raw(pid),
            shell::Event::ChangeDirectory(dir) => self.cd(dir)?,
            _ => {}
        }
        Ok(())
    }

    pub fn on_key(&mut self, key: Key, status_bar: &mut StatusBar) -> Result<()> {
        match key {
            Key::Char('l') | Key::Char('\n') => {
                if let Some(file) = self.selected() {
                    if file.metadata.is_dir() {
                        let path = file.path.clone();
                        self.cd(path.clone())?;
                        shell::run(self.shell_pid, "cd", &[path.to_str().unwrap()], false)?;
                    } else {
                        let open_cmd = match &file.extension {
                            None => "xdg-open",
                            Some(ext) => self
                                .open_methods
                                .get(ext)
                                .map(String::as_str)
                                .unwrap_or("xdg-open"),
                        };
                        shell::run(self.shell_pid, open_cmd, &[&file.name], true)?;
                    }
                }
            }
            Key::Char('h') | Key::Esc => {
                if let Some(parent) = self.dir.parent() {
                    let parent = parent.to_owned();
                    let current = self.dir.file_name().unwrap().to_str().unwrap().to_owned();
                    self.cd(parent.clone())?;
                    self.select_file(current);
                    shell::run(self.shell_pid, "cd", &[parent.to_str().unwrap()], false)?;
                }
            }
            Key::Char('.') => {
                self.show_hidden = !self.show_hidden;
                self.apply_filter();
            }
            Key::Char(' ') => {
                if let Some(file) = self.selected() {
                    if let Some(index) = self.files_marked.iter().position(|p| p == &file.path) {
                        self.files_marked.remove(index);
                    } else {
                        let path = file.path.clone();
                        self.files_marked.push(path);
                    }
                    if self.list_state.selected().unwrap() != self.files.len() - 1 {
                        self.select_next();
                    }
                }
            }
            Key::Char('p') => {
                if self.files_marked.is_empty() {
                    status_bar.show_message("No files marked");
                } else {
                    let files = mem::take(&mut self.files_marked);
                    let files: Vec<&str> = files.iter().map(|f| f.to_str().unwrap()).collect();
                    shell::run(self.shell_pid, "cp -r {} .", &files, true)?;
                }
            }
            Key::Char('m') => {
                if self.files_marked.is_empty() {
                    status_bar.show_message("No files marked");
                } else {
                    let files = mem::take(&mut self.files_marked);
                    let files: Vec<&str> = files.iter().map(|f| f.to_str().unwrap()).collect();
                    shell::run(self.shell_pid, "mv {} .", &files, true)?;
                }
            }
            Key::Char('d') => {
                if let Some(selected) = self.selected() {
                    let tp = if selected.metadata.is_file() {
                        "file"
                    } else {
                        "directory"
                    };
                    let file = selected.path.to_str().unwrap().to_owned();
                    status_bar.ask(
                        format!("Delete {} {}? [y/N]", tp, selected.name),
                        move |this, _| shell::run(this.shell_pid, "rm -r", &[&file], true),
                    );
                }
            }
            Key::Char('r') => {
                if let Some(file) = self.selected() {
                    let path = file.path.clone();
                    status_bar.edit(
                        "Rename: ",
                        &file.name,
                        |_, _, _| Ok(()),
                        move |new_name, this, _| {
                            shell::run(
                                this.shell_pid,
                                "mv",
                                &[
                                    path.to_str().unwrap(),
                                    path.with_file_name(new_name).to_str().unwrap(),
                                ],
                                true,
                            )
                        },
                    );
                }
            }
            Key::Char('/') => {
                status_bar.edit(
                    "/",
                    "",
                    |filter, this, _| {
                        this.filter = filter.to_owned();
                        this.apply_filter();
                        Ok(())
                    },
                    |_, this, _| {
                        this.filter.clear();
                        this.apply_filter();
                        Ok(())
                    },
                );
            }
            key => self.on_list_key(key)?,
        }
        Ok(())
    }

    pub fn draw(&mut self, frame: &mut Frame<impl Backend>, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)].as_ref())
            .split(area);

        frame.render_widget(
            Paragraph::new(
                [Text::styled(
                    self.dir.to_str().unwrap(),
                    Style::default().modifier(Modifier::UNDERLINED),
                )]
                .iter(),
            ),
            chunks[0],
        );

        let items = self
            .files
            .iter()
            .map(|file| {
                let color = if file.metadata.is_dir() {
                    Color::Blue
                } else if file.metadata.permissions().mode() & 0o1 != 0 {
                    Color::Green
                } else {
                    Color::White
                };
                let is_selected = if self.files_marked.contains(&file.path) {
                    "+"
                } else {
                    " "
                };
                let icon = ""; //self.icons.get(file);
                let suffix = if file.metadata.is_dir() { "/" } else { "" };
                Text::styled(
                    format!("{}{} {}{}", is_selected, icon, file.name, suffix),
                    Style::default().fg(color),
                )
            })
            .collect::<Vec<_>>()
            .into_iter();
        let list =
            List::new(items).highlight_style(Style::default().fg(Color::Black).bg(Color::Blue));
        frame.render_stateful_widget(list, chunks[1], &mut self.list_state);
    }
}

fn load_open_methods() -> Result<HashMap<String, String>> {
    let config = &env::var("HOME")?;
    let config = Path::new(&config);
    let config = config.join(".config/scd/open.yml");
    let mut res = HashMap::new();
    match fs::read_to_string(config) {
        Ok(buf) => {
            let raw: HashMap<String, String> = serde_yaml::from_str(&buf)?;
            for (exts, cmd) in raw {
                for ext in exts.split(',').map(str::trim) {
                    res.insert(ext.to_string(), cmd.clone());
                }
            }
        }
        Err(e) => {
            if e.kind() != io::ErrorKind::NotFound {
                return Err(e.into());
            }
        }
    }
    Ok(res)
}

impl<W> ListExt for FileManager<W>
where
    W: Watcher,
{
    type Item = FileInfo;

    fn get_index(&self) -> Option<usize> {
        self.list_state.selected()
    }

    fn get_list(&self) -> &[Self::Item] {
        &self.files
    }

    fn select(&mut self, index: Option<usize>) {
        self.list_state.select(index)
    }
}
