use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use crate::app::{ImageViewMode, LibrarySortField, SortDirection};

const QUALIFIER: &str = "dev";
const ORGANIZATION: &str = "Vibe";
const APPLICATION: &str = "ImageViewer";
const CONFIG_FILENAME: &str = "settings.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    #[serde(alias = "restore_last_folder")]
    pub restore_last_directory: bool,
    #[serde(alias = "last_open_folder")]
    pub last_open_directory: Option<PathBuf>,
    pub ui_font_filename: String,
    pub ui_font_size_pixels: f32,
    pub ui_scale_factor: f32,
    pub background_style: BackgroundStyle,
    pub image_cache_count: usize,
    #[serde(alias = "max_cache_size")]
    pub texture_cache_max_entries: usize,
    pub show_library: bool,
    pub show_info: bool,
    pub show_selection_window: bool,
    pub library_width: f32,
    pub image_view_mode: ImageViewMode,
    pub library_sort_field: LibrarySortField,
    pub sort_direction: SortDirection,
    pub show_thumbnail: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            restore_last_directory: true,
            last_open_directory: None,
            ui_font_filename: String::new(),
            ui_font_size_pixels: 14.0,
            ui_scale_factor: 1.0,
            background_style: BackgroundStyle::default(),
            image_cache_count: 32,
            texture_cache_max_entries: 20,
            show_library: true,
            show_info: true,
            show_selection_window: false,
            library_width: 300.0,
            image_view_mode: ImageViewMode::FitToWindow,
            library_sort_field: LibrarySortField::Name,
            sort_direction: SortDirection::Ascending,
            show_thumbnail: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackgroundStyle {
    pub mode: BackgroundMode,
    pub brightness: f32,
}

impl Default for BackgroundStyle {
    fn default() -> Self {
        Self {
            mode: BackgroundMode::Checker,
            brightness: 0.5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundMode {
    Checker,
    Solid,
}

impl Default for BackgroundMode {
    fn default() -> Self {
        BackgroundMode::Checker
    }
}

#[derive(Debug, Clone)]
pub struct ConfigHandle {
    pub settings: AppConfig,
    pub path: PathBuf,
}

/// Load config file, or create one from template on first run.
pub fn load_or_create(reset_config: bool) -> Result<ConfigHandle> {
    let config_dir = config_dir()?;
    fs::create_dir_all(&config_dir).with_context(|| {
        format!(
            "failed to create config directory at {}",
            config_dir.display()
        )
    })?;

    let config_path = config_dir.join(CONFIG_FILENAME);

    if reset_config || !config_path.exists() {
        let template = default_template();
        fs::write(&config_path, template).with_context(|| {
            format!(
                "failed to write default configuration to {}",
                config_path.display()
            )
        })?;
    }

    let raw = fs::read_to_string(&config_path).with_context(|| {
        format!(
            "failed to read configuration file {}",
            config_path.display()
        )
    })?;

    let settings: AppConfig = toml::from_str(&raw).with_context(|| {
        format!(
            "failed to parse configuration file {}",
            config_path.display()
        )
    })?;

    Ok(ConfigHandle {
        settings,
        path: config_path,
    })
}

/// Configuration directory path:
/// `$HOME/{QUALIFIER}.{ORGANIZATION}.{APPLICATION}` (Windows: `%USERPROFILE%/...`).
pub fn config_dir() -> Result<PathBuf> {
    let Some(base_dirs) = BaseDirs::new() else {
        return Err(anyhow::anyhow!(
            "failed to determine home directory for configuration path"
        ));
    };

    Ok(base_dirs
        .home_dir()
        .join(format!("{QUALIFIER}.{ORGANIZATION}.{APPLICATION}")))
}

/// Save current config state back to disk.
pub fn save(path: &Path, settings: &AppConfig) -> Result<()> {
    let serialized = toml::to_string_pretty(settings).context("failed to serialize config")?;
    fs::write(path, serialized)
        .with_context(|| format!("failed to write configuration file {}", path.display()))
}

fn default_template() -> &'static str {
    include_str!("../../config/default_settings.toml")
}
