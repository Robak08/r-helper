use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::device::CompleteDeviceState;
use crate::cooling_pad_auto::{
    DEFAULT_FOLLOW_TEMP_MARGIN_C, DEFAULT_OVERCOOL_HOLD_SECS, DEFAULT_RPM_SLEW_DOWN_PER_SEC,
    DEFAULT_RPM_SLEW_UP_PER_SEC, DEFAULT_TEMP_EMA_ALPHA, DEFAULT_TEMP_HYSTERESIS_C,
    DEFAULT_TURN_OFF_DELAY_SECS, DEFAULT_TURN_ON_DELAY_SECS,
};
use crate::startup;

const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoolingPadConfig {
    #[serde(default = "default_fan_mode")]
    pub fan_mode: String,
    /// Legacy field — used only when `fan_mode` is absent from saved JSON.
    #[serde(default, skip_serializing)]
    pub fan_on: bool,
    #[serde(default = "default_cooling_pad_rpm")]
    pub manual_rpm: u16,
    #[serde(default = "default_auto_min_rpm")]
    pub auto_min_rpm: u16,
    #[serde(default = "default_auto_max_rpm")]
    pub auto_max_rpm: u16,
    #[serde(default = "default_auto_off_below_c")]
    pub auto_off_below_c: f32,
    #[serde(default = "default_auto_full_above_c")]
    pub auto_full_above_c: f32,
    #[serde(default = "default_auto_turn_on_delay_secs")]
    pub auto_turn_on_delay_secs: f32,
    #[serde(default = "default_auto_turn_off_delay_secs")]
    pub auto_turn_off_delay_secs: f32,
    #[serde(default = "default_auto_overcool_hold_secs")]
    pub auto_overcool_hold_secs: f32,
    #[serde(default = "default_auto_temp_ema_alpha")]
    pub auto_temp_ema_alpha: f32,
    #[serde(default = "default_auto_rpm_slew_up_per_sec")]
    pub auto_rpm_slew_up_per_sec: u16,
    #[serde(default = "default_auto_rpm_slew_down_per_sec")]
    pub auto_rpm_slew_down_per_sec: u16,
    #[serde(default = "default_auto_follow_temp_margin_c")]
    pub auto_follow_temp_margin_c: f32,
    #[serde(default = "default_auto_temp_hysteresis_c")]
    pub auto_temp_hysteresis_c: f32,
    #[serde(default = "default_cooling_pad_lighting_mode")]
    pub lighting_mode: String,
    #[serde(default = "default_cooling_pad_color")]
    pub color: [u8; 3],
    #[serde(default = "default_cooling_pad_brightness_step")]
    pub brightness_step: usize,
}

fn default_fan_mode() -> String {
    "off".to_string()
}

fn default_cooling_pad_rpm() -> u16 {
    1500
}

fn default_auto_min_rpm() -> u16 {
    500
}

fn default_auto_max_rpm() -> u16 {
    3200
}

fn default_auto_off_below_c() -> f32 {
    60.0
}

fn default_auto_full_above_c() -> f32 {
    86.0
}

fn default_auto_turn_on_delay_secs() -> f32 {
    DEFAULT_TURN_ON_DELAY_SECS
}

fn default_auto_turn_off_delay_secs() -> f32 {
    DEFAULT_TURN_OFF_DELAY_SECS
}

fn default_auto_overcool_hold_secs() -> f32 {
    DEFAULT_OVERCOOL_HOLD_SECS
}

fn default_auto_temp_ema_alpha() -> f32 {
    DEFAULT_TEMP_EMA_ALPHA
}

fn default_auto_rpm_slew_up_per_sec() -> u16 {
    DEFAULT_RPM_SLEW_UP_PER_SEC
}

fn default_auto_rpm_slew_down_per_sec() -> u16 {
    DEFAULT_RPM_SLEW_DOWN_PER_SEC
}

fn default_auto_follow_temp_margin_c() -> f32 {
    DEFAULT_FOLLOW_TEMP_MARGIN_C
}

fn default_auto_temp_hysteresis_c() -> f32 {
    DEFAULT_TEMP_HYSTERESIS_C
}

fn default_cooling_pad_lighting_mode() -> String {
    "Static".to_string()
}

fn default_cooling_pad_color() -> [u8; 3] {
    [0x00, 0xFF, 0x00]
}

fn default_cooling_pad_brightness_step() -> usize {
    8
}

impl Default for CoolingPadConfig {
    fn default() -> Self {
        Self {
            fan_mode: default_fan_mode(),
            fan_on: false,
            manual_rpm: default_cooling_pad_rpm(),
            auto_min_rpm: default_auto_min_rpm(),
            auto_max_rpm: default_auto_max_rpm(),
            auto_off_below_c: default_auto_off_below_c(),
            auto_full_above_c: default_auto_full_above_c(),
            auto_turn_on_delay_secs: default_auto_turn_on_delay_secs(),
            auto_turn_off_delay_secs: default_auto_turn_off_delay_secs(),
            auto_overcool_hold_secs: default_auto_overcool_hold_secs(),
            auto_temp_ema_alpha: default_auto_temp_ema_alpha(),
            auto_rpm_slew_up_per_sec: default_auto_rpm_slew_up_per_sec(),
            auto_rpm_slew_down_per_sec: default_auto_rpm_slew_down_per_sec(),
            auto_follow_temp_margin_c: default_auto_follow_temp_margin_c(),
            auto_temp_hysteresis_c: default_auto_temp_hysteresis_c(),
            lighting_mode: default_cooling_pad_lighting_mode(),
            color: default_cooling_pad_color(),
            brightness_step: default_cooling_pad_brightness_step(),
        }
    }
}

/// Runtime cooling-pad settings mirrored in the GUI app.
#[derive(Debug, Clone)]
pub struct CoolingPadRuntime {
    pub fan_mode: String,
    pub manual_rpm: u16,
    pub auto_min_rpm: u16,
    pub auto_max_rpm: u16,
    pub auto_off_below_c: f32,
    pub auto_full_above_c: f32,
    pub auto_turn_on_delay_secs: f32,
    pub auto_turn_off_delay_secs: f32,
    pub auto_overcool_hold_secs: f32,
    pub auto_temp_ema_alpha: f32,
    pub auto_rpm_slew_up_per_sec: u16,
    pub auto_rpm_slew_down_per_sec: u16,
    pub auto_follow_temp_margin_c: f32,
    pub auto_temp_hysteresis_c: f32,
    pub lighting_mode: String,
    pub color: [u8; 3],
    pub brightness_step: usize,
}

impl From<&CoolingPadConfig> for CoolingPadRuntime {
    fn from(cfg: &CoolingPadConfig) -> Self {
        Self {
            fan_mode: cfg.fan_mode.clone(),
            manual_rpm: cfg.manual_rpm,
            auto_min_rpm: cfg.auto_min_rpm,
            auto_max_rpm: cfg.auto_max_rpm,
            auto_off_below_c: cfg.auto_off_below_c,
            auto_full_above_c: cfg.auto_full_above_c,
            auto_turn_on_delay_secs: cfg.auto_turn_on_delay_secs,
            auto_turn_off_delay_secs: cfg.auto_turn_off_delay_secs,
            auto_overcool_hold_secs: cfg.auto_overcool_hold_secs,
            auto_temp_ema_alpha: cfg.auto_temp_ema_alpha,
            auto_rpm_slew_up_per_sec: cfg.auto_rpm_slew_up_per_sec,
            auto_rpm_slew_down_per_sec: cfg.auto_rpm_slew_down_per_sec,
            auto_follow_temp_margin_c: cfg.auto_follow_temp_margin_c,
            auto_temp_hysteresis_c: cfg.auto_temp_hysteresis_c,
            lighting_mode: cfg.lighting_mode.clone(),
            color: cfg.color,
            brightness_step: cfg.brightness_step,
        }
    }
}

impl From<CoolingPadRuntime> for CoolingPadConfig {
    fn from(runtime: CoolingPadRuntime) -> Self {
        Self {
            fan_mode: runtime.fan_mode,
            fan_on: false,
            manual_rpm: runtime.manual_rpm,
            auto_min_rpm: runtime.auto_min_rpm,
            auto_max_rpm: runtime.auto_max_rpm,
            auto_off_below_c: runtime.auto_off_below_c,
            auto_full_above_c: runtime.auto_full_above_c,
            auto_turn_on_delay_secs: runtime.auto_turn_on_delay_secs,
            auto_turn_off_delay_secs: runtime.auto_turn_off_delay_secs,
            auto_overcool_hold_secs: runtime.auto_overcool_hold_secs,
            auto_temp_ema_alpha: runtime.auto_temp_ema_alpha,
            auto_rpm_slew_up_per_sec: runtime.auto_rpm_slew_up_per_sec,
            auto_rpm_slew_down_per_sec: runtime.auto_rpm_slew_down_per_sec,
            auto_follow_temp_margin_c: runtime.auto_follow_temp_margin_c,
            auto_temp_hysteresis_c: runtime.auto_temp_hysteresis_c,
            lighting_mode: runtime.lighting_mode,
            color: runtime.color,
            brightness_step: runtime.brightness_step,
        }
    }
}

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
    #[serde(default)]
    pub debug_enabled: bool,
    #[serde(default = "default_minimize_to_tray")]
    pub minimize_to_tray: bool,
    #[serde(default = "default_run_at_startup")]
    pub run_at_startup: bool,
    #[serde(default)]
    pub cooling_pad: CoolingPadConfig,
}

fn default_auto_fan_max_rpm() -> u16 {
    4500
}

fn default_minimize_to_tray() -> bool {
    true
}

fn default_run_at_startup() -> bool {
    startup::is_startup_enabled()
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
            debug_enabled: false,
            minimize_to_tray: default_minimize_to_tray(),
            run_at_startup: default_run_at_startup(),
            cooling_pad: CoolingPadConfig::default(),
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
        let raw: Value =
            serde_json::from_str(&contents).context("Failed to parse config JSON")?;
        let mut config: AppConfig =
            serde_json::from_value(raw.clone()).context("Failed to parse config JSON")?;
        migrate_cooling_pad_config(&mut config.cooling_pad, &raw);
        if config.version == 0 {
            config.version = CONFIG_VERSION;
        }
        Ok(config)
    }
}

fn migrate_cooling_pad_config(cooling_pad: &mut CoolingPadConfig, raw: &Value) {
    let had_fan_mode = raw
        .get("cooling_pad")
        .and_then(|cp| cp.get("fan_mode"))
        .is_some();
    if !had_fan_mode {
        cooling_pad.fan_mode = if cooling_pad.fan_on {
            "manual".to_string()
        } else {
            "off".to_string()
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_fan_on_to_fan_mode() {
        let raw: Value = serde_json::json!({
            "cooling_pad": { "fan_on": true, "manual_rpm": 2000 }
        });
        let mut cfg: CoolingPadConfig = serde_json::from_value(raw["cooling_pad"].clone()).unwrap();
        migrate_cooling_pad_config(&mut cfg, &raw);
        assert_eq!(cfg.fan_mode, "manual");
    }

    #[test]
    fn cooling_pad_auto_fields_default_when_missing() {
        let raw: Value = serde_json::json!({
            "cooling_pad": { "fan_mode": "auto" }
        });
        let cfg: CoolingPadConfig = serde_json::from_value(raw["cooling_pad"].clone()).unwrap();
        assert_eq!(cfg.auto_turn_on_delay_secs, DEFAULT_TURN_ON_DELAY_SECS);
        assert_eq!(cfg.auto_rpm_slew_up_per_sec, DEFAULT_RPM_SLEW_UP_PER_SEC);
        assert_eq!(cfg.auto_temp_hysteresis_c, DEFAULT_TEMP_HYSTERESIS_C);
    }
}
