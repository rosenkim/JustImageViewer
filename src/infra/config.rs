use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

const QUALIFIER: &str = "dev";
const ORGANIZATION: &str = "Vibe";
const APPLICATION: &str = "ImageViewer";
const CONFIG_FILENAME: &str = "settings.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub restore_last_folder: bool,
    pub slideshow_interval_secs: u64,
    pub cache_mb: u64,
    pub prefetch_neighbors: usize,
    pub background_style: BackgroundStyle,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            restore_last_folder: true,
            slideshow_interval_secs: 5,
            cache_mb: 512,
            prefetch_neighbors: 2,
            background_style: BackgroundStyle::default(),
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

pub fn load_or_create() -> Result<ConfigHandle> {
    let Some(dirs) = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION) else {
        return Err(anyhow::anyhow!(
            "failed to determine configuration directory for {QUALIFIER}.{ORGANIZATION}.{APPLICATION}"
        ));
    };

    let config_dir = dirs.config_dir();
    fs::create_dir_all(config_dir).with_context(|| {
        format!(
            "failed to create config directory at {}",
            config_dir.display()
        )
    })?;

    let config_path = config_dir.join(CONFIG_FILENAME);

    if !config_path.exists() {
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

fn default_template() -> &'static str {
    include_str!("../../config/default_settings.toml")
}
