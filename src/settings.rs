use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppSettings {
    pub external_diff_tool: Option<String>,
    #[serde(default = "default_check_updates")]
    pub check_updates: bool,
}

fn default_check_updates() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            external_diff_tool: None,
            check_updates: true,
        }
    }
}

impl AppSettings {
    pub fn config_dir() -> Option<PathBuf> {
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(|h| PathBuf::from(h).join(".config").join("duodiff"))
    }

    pub fn config_path() -> Option<PathBuf> {
        Self::config_dir().map(|d| d.join("config.toml"))
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(path) {
                    if let Ok(settings) = toml::from_str::<AppSettings>(&content) {
                        return settings;
                    }
                }
            }
        }
        AppSettings::default()
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        if let Some(dir) = Self::config_dir() {
            fs::create_dir_all(&dir)?;
            if let Some(path) = Self::config_path() {
                let content = toml::to_string(self).map_err(std::io::Error::other)?;
                fs::write(path, content)?;
            }
        }
        Ok(())
    }
}
