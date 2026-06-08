use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use humphrey_json::prelude::*;
use jasondb::Database;

const BOOKMARK_FILENAME: &str = "bookmark.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkEntry {
    pub path: PathBuf,
    pub bookmarked_at: String,
}

#[derive(Debug, Clone, FromJson, IntoJson)]
struct BookmarkRecord {
    path: String,
    bookmarked_at: String,
}

#[derive(Debug, Clone)]
pub struct BookmarkStore {
    path: PathBuf,
}

impl BookmarkStore {
    /// Build the bookmark store path next to settings.toml.
    pub fn new(settings_path: &Path) -> Self {
        let path = settings_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(BOOKMARK_FILENAME);
        Self { path }
    }
    /// Read all bookmarks from disk.
    pub fn load_all(&self) -> Result<Vec<BookmarkEntry>> {
        let mut database = Database::<BookmarkRecord>::new(&self.path)
            .with_context(|| format!("failed to open bookmark database {}", self.path.display()))?;

        let mut entries = Vec::new();
        for item in database.iter() {
            let (_, record) = item.with_context(|| {
                format!("failed to read bookmark data from {}", self.path.display())
            })?;
            entries.push(BookmarkEntry::from_record(record));
        }

        sort_bookmarks(&mut entries);
        Ok(entries)
    }

    /// Save one bookmark right away.
    pub fn save_entry(&self, entry: &BookmarkEntry) -> Result<()> {
        let mut database = Database::<BookmarkRecord>::new(&self.path)
            .with_context(|| format!("failed to open bookmark database {}", self.path.display()))?;

        database
            .set(entry.key(), &entry.to_record())
            .with_context(|| format!("failed to save bookmark {}", entry.path.display()))
    }

    /// Rewrite the full bookmark file from the current in-memory state.
    pub fn replace_all(&self, entries: &[BookmarkEntry]) -> Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path).with_context(|| {
                format!("failed to remove old bookmark file {}", self.path.display())
            })?;
        }

        let mut database = Database::<BookmarkRecord>::new(&self.path).with_context(|| {
            format!("failed to create bookmark database {}", self.path.display())
        })?;

        for entry in entries {
            database
                .set(entry.key(), &entry.to_record())
                .with_context(|| format!("failed to write bookmark {}", entry.path.display()))?;
        }

        Ok(())
    }
}

impl BookmarkEntry {
    pub fn new(path: PathBuf, bookmarked_at: String) -> Self {
        Self {
            path,
            bookmarked_at,
        }
    }

    pub fn key(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    fn to_record(&self) -> BookmarkRecord {
        BookmarkRecord {
            path: self.path.to_string_lossy().into_owned(),
            bookmarked_at: self.bookmarked_at.clone(),
        }
    }

    fn from_record(record: BookmarkRecord) -> Self {
        Self {
            path: PathBuf::from(record.path),
            bookmarked_at: record.bookmarked_at,
        }
    }
}

pub fn sort_bookmarks(entries: &mut [BookmarkEntry]) {
    entries.sort_by(|a, b| {
        b.bookmarked_at
            .cmp(&a.bookmarked_at)
            .then_with(|| a.path.cmp(&b.path))
    });
}
