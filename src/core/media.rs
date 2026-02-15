use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaFormat {
    Png,
    Jpeg,
    Bmp,
    Gif,
    WebP,
    Tiff,
    Tga,
    Ico,
    Pnm,
    // Hdr,
    Dds,
    Farbfeld,
}

impl MediaFormat {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" | "jfif" => Some(Self::Jpeg),
            "bmp" => Some(Self::Bmp),
            "gif" => Some(Self::Gif),
            "webp" => Some(Self::WebP),
            "tif" | "tiff" => Some(Self::Tiff),
            "tga" => Some(Self::Tga),
            "ico" => Some(Self::Ico),
            "pbm" | "pgm" | "ppm" | "pnm" => Some(Self::Pnm),
            // "hdr" => Some(Self::Hdr),
            "dds" => Some(Self::Dds),
            "ff" | "farbfeld" => Some(Self::Farbfeld),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Bmp => "BMP",
            Self::Gif => "GIF",
            Self::WebP => "WebP",
            Self::Tiff => "TIFF",
            Self::Tga => "TGA",
            Self::Ico => "ICO",
            Self::Pnm => "PNM",
            // Self::Hdr => "HDR",
            Self::Dds => "DDS",
            Self::Farbfeld => "Farbfeld",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MediaEntry {
    pub path: PathBuf,
    pub file_name: String,
    pub format: MediaFormat,
    pub file_size: u64,
}

pub fn scan_directory(root: &Path) -> Result<Vec<MediaEntry>> {
    let mut entries = Vec::new();

    let read_dir = fs::read_dir(root)
        .with_context(|| format!("failed to read directory {}", root.display()))?;

    for entry in read_dir {
        let entry = entry.with_context(|| {
            format!("failed to iterate directory entries in {}", root.display())
        })?;

        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to read file type for {}", path.display()))?;

        if !file_type.is_file() {
            continue;
        }

        let Some(ext) = path.extension().and_then(OsStr::to_str) else {
            continue;
        };

        let Some(format) = MediaFormat::from_extension(ext) else {
            continue;
        };

        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to read metadata for {}", path.display()))?;

        let file_size = metadata.len();

        let file_name = path
            .file_name()
            .and_then(OsStr::to_str)
            .map(|s| s.to_owned())
            .unwrap_or_else(|| path.display().to_string());

        entries.push(MediaEntry {
            path,
            file_name,
            format,
            file_size,
        });
    }

    entries.sort_by(|a, b| a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()));

    Ok(entries)
}
