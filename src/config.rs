use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::device::CompleteDeviceState;

const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: u32,
    pub auto_switch_enabled: bool,
    pub ac_profile: CompleteDeviceState,
    pub battery_profile: CompleteDeviceState,
    #[serde(default)]
    pub auto_fan_limit_enabled: bool,
    #[serde(default = "default_auto_fan_max_rpm")]
    pub auto_fan_max_rpm: u16,
}

fn default_auto_fan_max_rpm() -> u16 {
    4500
}

impl Default for AppConfig {
    fn default() -> Self {
        use librazer::types::PerfMode;

        Self {
            version: CONFIG_VERSION,
            auto_switch_enabled: true,
            ac_profile: CompleteDeviceState::default(),
            battery_profile: CompleteDeviceState {
                perf_mode: PerfMode::Battery,
                ..CompleteDeviceState::default()
            },
            auto_fan_limit_enabled: false,
            auto_fan_max_rpm: default_auto_fan_max_rpm(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        match Self::load_from_disk() {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Failed to load config (using defaults): {}", e);
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = config_file_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory {:?}", parent))?;
        }
        let json = serde_json::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(&path, json).with_context(|| format!("Failed to write config to {:?}", path))?;
        Ok(())
    }

    fn load_from_disk() -> Result<Self> {
        let path = config_file_path();
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {:?}", path))?;
        let mut config: AppConfig =
            serde_json::from_str(&contents).context("Failed to parse config JSON")?;
        if config.version == 0 {
            config.version = CONFIG_VERSION;
        }
        Ok(config)
    }
}

fn config_dir() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("r-helper")
}

fn config_file_path() -> PathBuf {
    config_dir().join("config.json")
}
