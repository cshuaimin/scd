use std::collections::HashMap;

use crate::FileInfo;

pub struct Icons {
    directory: &'static str,
    file: &'static str,
    from_extension: HashMap<&'static str, &'static str>,
}

impl Icons {
    pub fn new() -> Self {
        let mut m = HashMap::new();

        m.insert("png", "\u{f1c5}");
        m.insert("jpg", "\u{f1c5}");
        m.insert("webp", "\u{f1c5}");

        m.insert("mp4", "\u{f03d}");
        m.insert("mkv", "\u{f03d}");
        m.insert("avi", "\u{f03d}");
        m.insert("flv", "\u{f03d}");
        m.insert("webm", "\u{f03d}");

        m.insert("yaml", "\u{f013}");
        m.insert("yml", "\u{f013}");
        m.insert("toml", "\u{f013}");
        m.insert("conf", "\u{f013}");

        m.insert("md", "\u{f60f}");

        m.insert("py", "\u{f3e2}");
        m.insert("java", "\u{f4e4}");
        m.insert("java", "\u{f3b8}");

        Self {
            directory: "\u{f07b}",
            file: "\u{f016}",
            from_extension: m,
        }
    }

    pub fn get(&self, file: &FileInfo) -> &'static str {
        if file.metadata.is_dir() {
            return self.directory;
        }

        match &file.extension {
            None => self.file,
            Some(ext) => self.from_extension.get(ext.as_str()).unwrap_or(&self.file),
        }
    }
}
