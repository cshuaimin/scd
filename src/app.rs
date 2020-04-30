use std::cmp;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::env;
use std::fs::{self, DirEntry, Metadata};
use std::io;
use std::mem;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use sysinfo::{RefreshKind, System, SystemExt};
use tui::widgets::ListState;

use crate::icons::Icons;

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

#[derive(Debug)]
pub enum Action {
    Delete(FileInfo),
    Rename(FileInfo),
    Filter,
}

#[derive(Debug)]
pub enum Mode {
    /// Show selected file's mode, size, etc.
    Normal,

    /// Display a short lived message.
    Message { text: String, expire_at: Instant },

    /// Ask a yes/no question.
    Ask { prompt: String, action: Action },

    /// Input some text.
    Input {
        prompt: String,
        input: String,
        offset: usize,
        action: Action,
    },
}

/// App contains all the state of the application.
pub struct App {
    // file manager states
    pub dir: PathBuf,
    pub all_files: Vec<FileInfo>,
    pub files: Vec<FileInfo>, // filtered
    pub files_marked: Vec<PathBuf>,
    pub filter: String,
    pub show_hidden: bool,
    pub icons: Icons,
    pub list_state: ListState,
    pub watcher: RecommendedWatcher,
    pub shell_pid: i32,
    pub open_methods: HashMap<String, String>,

    // bottom input line states
    pub mode: Mode,

    // system monitor states
    pub system: System,
}

impl App {
    pub fn new(watcher: RecommendedWatcher) -> Result<Self> {
        let mut app = Self {
            dir: env::current_dir()?,
            all_files: vec![],
            files: vec![],
            files_marked: vec![],
            filter: "".to_string(),
            show_hidden: false,
            icons: Icons::new(),
            list_state: ListState::default(),
            watcher,
            shell_pid: 0,
            open_methods: get_open_methods()?,
            mode: Mode::Normal,
            system: System::new_with_specifics(RefreshKind::new().with_cpu().with_memory()),
        };
        app.refresh_directory()?;
        app.select_first();
        app.watcher.watch(&app.dir, RecursiveMode::NonRecursive)?;

        Ok(app)
    }

    pub fn refresh_directory(&mut self) -> Result<()> {
        self.all_files.clear();
        for entry in fs::read_dir(&self.dir)? {
            self.all_files.push(FileInfo::try_from(entry?)?);
        }
        self.all_files
            .sort_unstable_by(|a, b| match (a.metadata.is_dir(), b.metadata.is_dir()) {
                (true, false) => cmp::Ordering::Less,
                (false, true) => cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            });
        self.apply_filter();

        Ok(())
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

    pub fn cd(&mut self, dir: PathBuf) -> Result<()> {
        if dir != self.dir {
            self.watcher.unwatch(&self.dir)?;
            self.dir = dir;
            self.refresh_directory()?;
            self.select_first();
            self.watcher.watch(&self.dir, RecursiveMode::NonRecursive)?;
        }
        Ok(())
    }

    pub fn files_marked(&mut self) -> Vec<String> {
        let mut marked = vec![];
        mem::swap(&mut self.files_marked, &mut marked);
        marked
            .iter()
            .map(|p| {
                if p.parent().unwrap() == self.dir {
                    p.file_name().unwrap().to_str().unwrap()
                } else {
                    p.to_str().unwrap()
                }
            })
            .map(|s| s.to_string())
            .collect()
    }

    pub fn update_on_tick(&mut self) {
        self.system.refresh_cpu();
        self.system.refresh_memory();
    }

    pub fn show_message(&mut self, text: &str) {
        self.mode = Mode::Message {
            text: text.to_string(),
            expire_at: Instant::now() + Duration::from_secs(3),
        };
    }

    pub fn selected(&self) -> Option<&FileInfo> {
        if self.files.is_empty() {
            None
        } else {
            let idx = self.list_state.selected().unwrap();
            Some(&self.files[idx])
        }
    }

    pub fn select_first(&mut self) {
        let index = if self.files.is_empty() { None } else { Some(0) };
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
        let index = self
            .list_state
            .selected()
            .map(|i| (i + 1) % self.files.len());
        self.list_state.select(index);
    }

    pub fn select_prev(&mut self) {
        let index = match self.list_state.selected() {
            None => None,
            Some(0) if self.files.len() == 0 => None,
            Some(0) => Some(self.files.len() - 1),
            Some(i) => Some(i - 1),
        };
        self.list_state.select(index);
    }
}

fn get_open_methods() -> Result<HashMap<String, String>> {
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
        Err(_) => {} // fall back to xdg-open
    }
    Ok(res)
}
