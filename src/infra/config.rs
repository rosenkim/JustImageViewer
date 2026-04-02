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
const DEFAULT_BACKGROUND_COLOR1: &str = "#CCCCCC";
const DEFAULT_BACKGROUND_COLOR2: &str = "#FFFFFF";
const LOGICAL_DPI: f32 = 96.0;
const POINTS_PER_INCH: f32 = 72.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    #[serde(alias = "restore_last_folder")]
    pub restore_last_directory: bool,
    #[serde(alias = "last_open_folder")]
    pub last_open_directory: Option<PathBuf>,
    pub ui_font_filename: String,
    #[serde(alias = "ui_font_size_pixels")]
    pub ui_font_size_pt: f32,
    pub ui_scale_factor: f32,
    pub background_style: BackgroundStyle,
    pub image_cache_count: usize,
    pub focused_fps: u32,
    pub unfocused_fps: u32,

    pub show_library: bool,
    pub show_info: bool,
    pub show_selection_window: bool,
    pub library_width: f32,
    pub image_view_mode: ImageViewMode,
    pub library_sort_field: LibrarySortField,
    pub sort_direction: SortDirection,
    pub show_thumbnail: bool,
    pub show_grid_view: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            restore_last_directory: true,
            last_open_directory: None,
            ui_font_filename: String::new(),
            // 10.5pt maps to about 14px at 96 DPI.
            ui_font_size_pt: 10.5,
            ui_scale_factor: 1.0,
            background_style: BackgroundStyle::default(),
            image_cache_count: 32,
            focused_fps: 60,
            unfocused_fps: 5,
            show_library: true,
            show_info: true,
            show_selection_window: false,
            library_width: 300.0,
            image_view_mode: ImageViewMode::FitToWindow,
            library_sort_field: LibrarySortField::Name,
            sort_direction: SortDirection::Ascending,
            show_thumbnail: true,
            show_grid_view: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackgroundStyle {
    pub mode: BackgroundMode,
    #[serde(default = "default_background_color1")]
    pub color1: String,
    #[serde(default = "default_background_color2")]
    pub color2: String,
}

impl Default for BackgroundStyle {
    fn default() -> Self {
        Self {
            mode: BackgroundMode::Checker,
            color1: default_background_color1(),
            color2: default_background_color2(),
        }
    }
}

impl BackgroundStyle {
    pub fn resolved_colors_rgb(&self) -> ([f32; 3], [f32; 3]) {
        let color1 = parse_hex_rgb(&self.color1).unwrap_or([0.3725, 0.3725, 0.3725]);
        let color2 = parse_hex_rgb(&self.color2).unwrap_or([0.2902, 0.2902, 0.2902]);
        (color1, color2)
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

    let mut settings: AppConfig = toml::from_str(&raw).with_context(|| {
        format!(
            "failed to parse configuration file {}",
            config_path.display()
        )
    })?;

    // Backward-compatibility migration:
    // Old config used `ui_font_size_pixels`. Convert that value to pt.
    if let Ok(raw_value) = raw.parse::<toml::Value>() {
        let has_pt_key = raw_value.get("ui_font_size_pt").is_some();
        if !has_pt_key {
            if let Some(old_px) = raw_value
                .get("ui_font_size_pixels")
                .and_then(toml_number_to_f32)
            {
                if old_px > 0.0 {
                    settings.ui_font_size_pt = old_px * (POINTS_PER_INCH / LOGICAL_DPI);
                }
            }
        }
    }

    normalize_background_style(&mut settings.background_style);

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

fn toml_number_to_f32(value: &toml::Value) -> Option<f32> {
    if let Some(v) = value.as_float() {
        return Some(v as f32);
    }

    value.as_integer().map(|v| v as f32)
}

fn default_background_color1() -> String {
    DEFAULT_BACKGROUND_COLOR1.to_owned()
}

fn default_background_color2() -> String {
    DEFAULT_BACKGROUND_COLOR2.to_owned()
}

fn normalize_background_style(style: &mut BackgroundStyle) {
    if parse_hex_rgb(&style.color1).is_none() {
        log::warn!(
            "Invalid background_style.color1 '{}'. Using default {}",
            style.color1,
            DEFAULT_BACKGROUND_COLOR1
        );
        style.color1 = default_background_color1();
    }

    if parse_hex_rgb(&style.color2).is_none() {
        log::warn!(
            "Invalid background_style.color2 '{}'. Using default {}",
            style.color2,
            DEFAULT_BACKGROUND_COLOR2
        );
        style.color2 = default_background_color2();
    }
}

pub fn parse_hex_rgb(value: &str) -> Option<[f32; 3]> {
    if (value.len() != 7 && value.len() != 9) || !value.starts_with('#') {
        return None;
    }

    let r = u8::from_str_radix(&value[1..3], 16).ok()?;
    let g = u8::from_str_radix(&value[3..5], 16).ok()?;
    let b = u8::from_str_radix(&value[5..7], 16).ok()?;

    Some([
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    ])
}
