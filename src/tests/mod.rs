use notify::Watcher;

use crate::app::*;

mod app;

impl<W: Watcher> App<W> {
    fn file_names(&self) -> Vec<&str> {
        self.files.iter().map(|f| f.name.as_str()).collect()
    }
}
