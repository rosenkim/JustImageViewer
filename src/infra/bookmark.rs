use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const BOOKMARK_FILENAME: &str = "bookmarks.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkEntry {
    pub path: PathBuf,
    pub bookmarked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BookmarkRecord {
    path: String,
    bookmarked_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BookmarkFile {
    bookmark: Vec<BookmarkRecord>,
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
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        let file: BookmarkFile = toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", self.path.display()))?;

        let mut entries: Vec<BookmarkEntry> = file
            .bookmark
            .into_iter()
            .map(|r| BookmarkEntry {
                path: PathBuf::from(r.path),
                bookmarked_at: r.bookmarked_at,
            })
            .collect();

        sort_bookmarks(&mut entries);
        Ok(entries)
    }

    /// Save one bookmark, skipping duplicates.
    pub fn save_entry(&self, entry: &BookmarkEntry) -> Result<()> {
        let mut entries = self.load_all().unwrap_or_default();
        if !entries.iter().any(|e| e.path == entry.path) {
            entries.push(entry.clone());
        }
        self.write_all(&entries)
    }

    /// Rewrite the full bookmark file from the current in-memory state.
    pub fn replace_all(&self, entries: &[BookmarkEntry]) -> Result<()> {
        self.write_all(entries)
    }

    fn write_all(&self, entries: &[BookmarkEntry]) -> Result<()> {
        let file = BookmarkFile {
            bookmark: entries
                .iter()
                .map(|e| BookmarkRecord {
                    path: e.path.to_string_lossy().into_owned(),
                    bookmarked_at: e.bookmarked_at.clone(),
                })
                .collect(),
        };
        let content = toml::to_string_pretty(&file)
            .context("failed to serialize bookmarks to TOML")?;
        fs::write(&self.path, &content)
            .with_context(|| format!("failed to write {}", self.path.display()))
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
}

pub fn sort_bookmarks(entries: &mut [BookmarkEntry]) {
    entries.sort_by(|a, b| {
        b.bookmarked_at
            .cmp(&a.bookmarked_at)
            .then_with(|| a.path.cmp(&b.path))
    });
}
