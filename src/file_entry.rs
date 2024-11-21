// from https://github.com/fenjalien/obsidian-typst/blob/master/compiler/src/file_entry.rs

use std::{cell::OnceCell, sync::OnceLock};

use typst::{
    foundations::Bytes,
    syntax::{FileId, Source},
};

#[derive(Clone)]
pub struct FileEntry {
    bytes: OnceLock<Bytes>,
    pub source: Source,
}

impl FileEntry {
    pub fn new(id: FileId, text: String) -> Self {
        Self {
            bytes: OnceLock::new(),
            source: Source::new(id, text),
        }
    }

    pub fn source(&self) -> Source {
        self.source.clone()
    }

    pub fn bytes(&self) -> Bytes {
        self.bytes
            .get_or_init(|| Bytes::from(self.source.text().as_bytes()))
            .clone()
    }
}
