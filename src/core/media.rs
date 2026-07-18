use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
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
    pub modified_time: Duration,
    pub dimensions: Option<(usize, usize)>,
    pub thumbnail: Option<ThumbnailInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThumbnailInfo {
    pub atlas_image_id: u64,
    pub texture_index: imgui::TextureId,
    pub uvs: [f32; 4],
    pub image_size: (u32, u32),
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

        // A file may be mid-write while we scan (e.g. during Refresh). Do not
        // abort the whole scan: keep the entry with default metadata so the UI
        // shows the empty-image icon. The zeroed size/mtime will not match the
        // real values on the next Refresh, so the file is picked up as modified
        // and its thumbnail is regenerated then.
        let (file_size, modified_time) = match entry.metadata() {
            Ok(metadata) => {
                let modified_time = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .unwrap_or_default();
                (metadata.len(), modified_time)
            }
            Err(err) => {
                log::warn!("failed to read metadata for {}: {err}", path.display());
                (0, Duration::default())
            }
        };

        let file_name = path
            .file_name()
            .and_then(OsStr::to_str)
            .map(|s| s.to_owned())
            .unwrap_or_else(|| path.display().to_string());
        // Read only image header info here so UI can show resolution without full decode.
        let dimensions = image::image_dimensions(&path)
            .ok()
            .map(|(width, height)| (width as usize, height as usize));

        entries.push(MediaEntry {
            path,
            file_name,
            format,
            file_size,
            modified_time,
            dimensions,
            thumbnail: None,
        });
    }

    Ok(entries)
}
