use log::warn;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

/// Configuration version for future migrations
const CONFIG_VERSION: u32 = 1;

/// Configuration file name
const CONFIG_FILENAME: &str = "config.toml";

/// Main configuration structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: u32,

    #[serde(default)]
    pub last_keyboard_id: Option<String>,

    #[serde(default)]
    pub keyboards: HashMap<String, KeyboardConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            last_keyboard_id: None,
            keyboards: HashMap::new(),
        }
    }
}

fn default_version() -> u32 {
    CONFIG_VERSION
}

/// Per-keyboard configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyboardConfig {
    #[serde(default)]
    pub label: String,

    #[serde(default)]
    pub vid: u16,

    #[serde(default)]
    pub pid: u16,

    #[serde(default)]
    pub usage_page: u16,

    #[serde(default)]
    pub lang_layer: HashMap<String, u8>,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            label: String::new(),
            vid: 0,
            pid: 0,
            usage_page: 0,
            lang_layer: HashMap::new(),
        }
    }
}

/// Get the path to the config file (in the same directory as the executable)
fn config_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.join(CONFIG_FILENAME)))
}

/// Load configuration from file, or return defaults if file doesn't exist
pub fn load_config() -> Config {
    match config_path() {
        Some(path) => {
            if path.exists() {
                match fs::read_to_string(&path) {
                    Ok(content) => match toml::from_str::<Config>(&content) {
                        Ok(config) => config,
                        Err(e) => {
                            warn!("Failed to parse config file: {}", e);
                            Config::default()
                        }
                    },
                    Err(e) => {
                        warn!("Failed to read config file: {}", e);
                        Config::default()
                    }
                }
            } else {
                Config::default()
            }
        }
        None => {
            warn!("Could not determine config file path");
            Config::default()
        }
    }
}

/// Save configuration to file atomically
pub fn save_config(config: &Config) -> Result<(), String> {
    let path = config_path().ok_or("Could not determine config file path")?;

    // Serialize to TOML
    let toml_string =
        toml::to_string_pretty(config).map_err(|e| format!("Failed to serialize config: {}", e))?;

    // Write to temp file first (atomic write)
    let temp_path = path.with_extension("toml.tmp");
    {
        let mut file = File::create(&temp_path)
            .map_err(|e| format!("Failed to create temp config file: {}", e))?;
        file.write_all(toml_string.as_bytes())
            .map_err(|e| format!("Failed to write config: {}", e))?;
        // Avoid sync_all here to keep UI responsive; config writes are best-effort.
    }

    // Atomically replace the old config file
    fs::rename(&temp_path, &path).map_err(|e| format!("Failed to rename config file: {}", e))?;

    Ok(())
}

/// Get or create keyboard config for a given keyboard_id
pub fn get_keyboard_config<'a>(
    config: &'a mut Config,
    keyboard_id: &str,
) -> &'a mut KeyboardConfig {
    config.keyboards.entry(keyboard_id.to_string()).or_default()
}

/// Update last selected keyboard
pub fn set_last_keyboard(config: &mut Config, keyboard_id: String) {
    config.last_keyboard_id = Some(keyboard_id);
}
