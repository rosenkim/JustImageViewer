use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::core::helper::canonicalize_path;

const BOOKMARK_FILENAME: &str = "bookmarks.json";

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
    bookmarks: Vec<BookmarkRecord>,
    #[serde(default)]
    recent_directories: Vec<PathBuf>,
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
        let file = self.read_file()?;

        let mut entries: Vec<BookmarkEntry> = file
            .bookmarks
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
        let mut entries = self.load_all()?;
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
            recent_directories: self.read_file()?.recent_directories,
            bookmarks: entries
                .iter()
                .map(|e| BookmarkRecord {
                    path: e.path.to_string_lossy().into_owned(),
                    bookmarked_at: e.bookmarked_at.clone(),
                })
                .collect(),
        };
        self.write_file(&file)
    }

    pub fn load_recent_directories(&self) -> Result<Vec<PathBuf>> {
        Ok(self.read_file()?.recent_directories)
    }

    pub fn save_recent_directories(&self, directories: &[PathBuf]) -> Result<()> {
        let mut file = self.read_file()?;
        file.recent_directories = directories.to_vec();
        self.write_file(&file)
    }

    fn read_file(&self) -> Result<BookmarkFile> {
        let path = &self.path;
        if !path.exists() {
            return Ok(BookmarkFile::default());
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&content)
            .with_context(|| format!("failed to parse {}", path.display()))
    }

    fn write_file(&self, file: &BookmarkFile) -> Result<()> {
        // Pretty JSON so the file stays readable when opened by hand.
        let mut content = serde_json::to_string_pretty(&file)
            .context("failed to serialize bookmarks to JSON")?;
        content.push('\n');
        fs::write(&self.path, &content)
            .with_context(|| format!("failed to write {}", self.path.display()))
    }
}

impl BookmarkEntry {
    pub fn new(path: PathBuf, bookmarked_at: String) -> Self {
        Self {
            path: canonicalize_path(&path),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_and_bookmarks_share_existing_bookmarks_file() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("jiv-bookmarks-{unique}"));
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("bookmarks.json"),
            r#"{"bookmarks":[{"path":"/pictures/a","bookmarked_at":"2026-09-04"}]}"#,
        ).unwrap();
        let store = BookmarkStore::new(&directory.join("settings.toml"));
        assert!(store.load_recent_directories().unwrap().is_empty());
        let entries = store.load_all().unwrap();
        let recent = vec![PathBuf::from("/pictures/recent")];
        store.save_recent_directories(&recent).unwrap();
        assert!(directory.join("bookmarks.json").is_file());
        assert!(!directory.join("bookmark.json").exists());
        assert_eq!(store.load_all().unwrap(), entries);
        store.replace_all(&[]).unwrap();
        assert_eq!(store.load_recent_directories().unwrap(), recent);
        store.save_entry(&entries[0]).unwrap();
        assert_eq!(store.load_recent_directories().unwrap(), recent);
        assert_eq!(store.load_all().unwrap(), entries);
        fs::remove_file(directory.join("bookmarks.json")).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
