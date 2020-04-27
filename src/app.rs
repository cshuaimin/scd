use std::cmp;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::env;
use std::fs::{self, DirEntry, Metadata};
use std::io;
use std::mem;
use std::path::PathBuf;

use anyhow::{Context, Result};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use sysinfo::{RefreshKind, System, SystemExt};
use tui::widgets::ListState;

use crate::icons::Icons;
use crate::shell::*;

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

/// App contains all the state of the application.
pub struct App {
    // file manager states
    pub dir: PathBuf,
    pub all_files: Vec<FileInfo>,
    // filtered files
    pub files: Vec<FileInfo>,
    pub files_marked: Vec<PathBuf>,
    pub filter: String,
    pub show_hidden: bool,
    pub icons: Icons,
    pub list_state: ListState,
    pub watcher: RecommendedWatcher,
    pub shell_pid: i32,
    pub open_methods: HashMap<String, String>,
    // system monitor states
    pub system: System,
}

impl App {
    pub fn new(watcher: RecommendedWatcher) -> Result<Self> {
        let open_methods = {
            let buf = fs::read_to_string(OPEN_METHODS_CONFIG).with_context(|| {
                format!("Failed to read open methods from {}", OPEN_METHODS_CONFIG)
            })?;
            let raw: HashMap<String, String> = serde_yaml::from_str(&buf)
                .with_context(|| format!("Failed to parse config file {}", OPEN_METHODS_CONFIG))?;
            let mut res = HashMap::new();
            for (exts, cmd) in raw {
                for ext in exts.split(',').map(str::trim) {
                    res.insert(ext.to_string(), cmd.clone());
                }
            }
            res
        };
        let system = System::new_with_specifics(RefreshKind::new().with_cpu().with_memory());

        let mut app = Self {
            dir: env::current_dir()?,
            all_files: vec![],
            files: vec![],
            files_marked: vec![],
            filter: String::new(),
            show_hidden: false,
            icons: Icons::new(),
            list_state: ListState::default(),
            watcher,
            shell_pid: 0,
            open_methods,
            system,
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
        self.filter_files();

        Ok(())
    }

    pub fn filter_files(&mut self) {
        self.files = self
            .all_files
            .iter()
            .filter(|f| self.show_hidden || !f.name.starts_with('.'))
            .filter(|f| f.name.contains(&self.filter))
            .cloned()
            .collect();
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
