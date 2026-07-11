use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cooling_pad_auto::{
    DEFAULT_FOLLOW_TEMP_MARGIN_C, DEFAULT_OVERCOOL_HOLD_SECS, DEFAULT_RPM_SLEW_DOWN_PER_SEC,
    DEFAULT_RPM_SLEW_UP_PER_SEC, DEFAULT_TEMP_EMA_ALPHA, DEFAULT_TEMP_HYSTERESIS_C,
    DEFAULT_TURN_OFF_DELAY_SECS, DEFAULT_TURN_ON_DELAY_SECS,
};
use crate::device::CompleteDeviceState;
use crate::startup;

pub(crate) const CONFIG_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolingPadConfig {
    #[serde(default = "default_fan_mode")]
    pub fan_mode: String,
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
    #[serde(default = "default_follow_laptop_fan")]
    pub follow_laptop_fan: bool,
    #[serde(default = "default_cooling_pad_lighting_mode")]
    pub lighting_mode: String,
    #[serde(default = "default_cooling_pad_color")]
    pub color: [u8; 3],
    #[serde(default = "default_cooling_pad_brightness_step")]
    pub brightness_step: usize,
}

fn default_follow_laptop_fan() -> bool {
    true
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
            follow_laptop_fan: default_follow_laptop_fan(),
            lighting_mode: default_cooling_pad_lighting_mode(),
            color: default_cooling_pad_color(),
            brightness_step: default_cooling_pad_brightness_step(),
        }
    }
}

/// Runtime cooling-pad settings mirrored in the GUI app.
#[derive(Debug, Clone, PartialEq)]
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
    pub follow_laptop_fan: bool,
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
            follow_laptop_fan: cfg.follow_laptop_fan,
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
            follow_laptop_fan: runtime.follow_laptop_fan,
            lighting_mode: runtime.lighting_mode,
            color: runtime.color,
            brightness_step: runtime.brightness_step,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
            Ok((config, migrated)) => {
                if migrated && let Err(e) = config.save() {
                    eprintln!("Failed to rewrite migrated config: {e}");
                }
                config
            }
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
        write_config_atomically(&path, json.as_bytes())?;
        Ok(())
    }

    fn load_from_disk() -> Result<(Self, bool)> {
        let path = config_file_path();
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {:?}", path))?;
        Self::parse(&contents)
    }

    fn parse(contents: &str) -> Result<(Self, bool)> {
        let raw: Value = serde_json::from_str(contents).context("Failed to parse config JSON")?;
        let mut config: AppConfig =
            serde_json::from_value(raw.clone()).context("Failed to parse config JSON")?;
        let mut migrated = false;
        if config.version < CONFIG_VERSION {
            migrate_config_to_v2(&mut config, &raw);
            config.version = CONFIG_VERSION;
            migrated = true;
        }
        Ok((config, migrated))
    }
}

fn write_config_atomically(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    let temp_path = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temp_path)
        .with_context(|| format!("Failed to create temporary config {:?}", temp_path))?;
    use std::io::Write;
    file.write_all(contents)
        .with_context(|| format!("Failed to write temporary config {:?}", temp_path))?;
    file.sync_all().with_context(|| format!("Failed to flush temporary config {:?}", temp_path))?;
    drop(file);

    replace_file(&temp_path, path)
        .with_context(|| format!("Failed to replace config {:?} with {:?}", path, temp_path))
}

#[cfg(windows)]
fn replace_file(source: &std::path::Path, destination: &std::path::Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    use windows::core::PCWSTR;

    let source = source.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    let destination = destination.as_os_str().encode_wide().chain(Some(0)).collect::<Vec<_>>();
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .context("MoveFileExW failed")
    }
}

#[cfg(not(windows))]
fn replace_file(source: &std::path::Path, destination: &std::path::Path) -> Result<()> {
    fs::rename(source, destination).context("rename failed")
}

fn migrate_config_to_v2(config: &mut AppConfig, raw: &Value) {
    let had_fan_mode = raw.get("cooling_pad").and_then(|cp| cp.get("fan_mode")).is_some();
    if !had_fan_mode {
        let fan_on = raw
            .get("cooling_pad")
            .and_then(|cp| cp.get("fan_on"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        config.cooling_pad.fan_mode = if fan_on { "manual".to_string() } else { "off".to_string() };
    }

    let had_follow = raw.get("cooling_pad").and_then(|cp| cp.get("follow_laptop_fan")).is_some();
    if !had_follow {
        config.cooling_pad.follow_laptop_fan = default_follow_laptop_fan();
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
    use librazer::types::{
        BatteryCare, CpuBoost, FanMode, GpuBoost, LightsAlwaysOn, LogoMode, MaxFanSpeedMode,
        PerfMode,
    };

    #[test]
    fn migrates_v1_fan_on_and_sets_follow_default() {
        let raw: Value = serde_json::json!({
            "version": 1,
            "cooling_pad": { "fan_on": true, "manual_rpm": 2000 }
        });
        let mut config = AppConfig::default();
        config.version = 1;
        migrate_config_to_v2(&mut config, &raw);
        assert_eq!(config.cooling_pad.fan_mode, "manual");
        assert!(config.cooling_pad.follow_laptop_fan);
    }

    #[test]
    fn v2_save_omits_fan_on() {
        let config = AppConfig::default();
        let json = serde_json::to_value(&config).unwrap();
        assert!(json["cooling_pad"].get("fan_on").is_none());
        assert_eq!(json["version"], CONFIG_VERSION);
    }

    #[test]
    fn loads_v2_follow_laptop_fan() {
        let mut cfg = AppConfig::default();
        cfg.cooling_pad.follow_laptop_fan = false;
        let loaded: AppConfig =
            serde_json::from_value(serde_json::to_value(&cfg).unwrap()).unwrap();
        assert!(!loaded.cooling_pad.follow_laptop_fan);
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

    #[test]
    fn complete_non_default_config_round_trip_is_stable() {
        let ac_profile = CompleteDeviceState {
            perf_mode: PerfMode::Custom,
            fan_mode: FanMode::Manual,
            fan_rpm: Some(4100),
            logo_mode: LogoMode::Breathing,
            keyboard_brightness: 73,
            lights_always_on: LightsAlwaysOn::Enable,
            battery_care: BatteryCare::Percent50,
            cpu_boost: Some(CpuBoost::High),
            gpu_boost: Some(GpuBoost::Medium),
            max_fan_speed: Some(MaxFanSpeedMode::Enable),
        };
        let battery_profile = CompleteDeviceState {
            perf_mode: PerfMode::Silent,
            fan_mode: FanMode::Manual,
            fan_rpm: Some(2200),
            logo_mode: LogoMode::Static,
            keyboard_brightness: 17,
            lights_always_on: LightsAlwaysOn::Enable,
            battery_care: BatteryCare::Disable,
            cpu_boost: Some(CpuBoost::Low),
            gpu_boost: Some(GpuBoost::High),
            max_fan_speed: Some(MaxFanSpeedMode::Disable),
        };
        let cooling_pad = CoolingPadConfig {
            fan_mode: "auto".into(),
            manual_rpm: 1900,
            auto_min_rpm: 700,
            auto_max_rpm: 3100,
            auto_off_below_c: 51.5,
            auto_full_above_c: 82.5,
            auto_turn_on_delay_secs: 2.5,
            auto_turn_off_delay_secs: 7.5,
            auto_overcool_hold_secs: 11.5,
            auto_temp_ema_alpha: 0.42,
            auto_rpm_slew_up_per_sec: 333,
            auto_rpm_slew_down_per_sec: 222,
            auto_follow_temp_margin_c: 4.5,
            auto_temp_hysteresis_c: 1.75,
            follow_laptop_fan: false,
            lighting_mode: "Spectrum".into(),
            color: [12, 34, 56],
            brightness_step: 5,
        };
        let expected = AppConfig {
            version: CONFIG_VERSION,
            auto_switch_enabled: false,
            ac_profile,
            battery_profile,
            auto_fan_limit_enabled: true,
            auto_fan_max_rpm: 3875,
            debug_enabled: true,
            minimize_to_tray: false,
            run_at_startup: false,
            cooling_pad,
        };

        let json = serde_json::to_string_pretty(&expected).unwrap();
        let (actual, migrated) = AppConfig::parse(&json).unwrap();

        assert!(!migrated);
        assert_eq!(actual, expected);
    }

    #[test]
    fn migrated_v1_config_is_stable_after_rewrite() {
        let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
        raw["version"] = Value::from(1);
        let cooling_pad = raw["cooling_pad"].as_object_mut().unwrap();
        cooling_pad.remove("fan_mode");
        cooling_pad.remove("follow_laptop_fan");
        cooling_pad.insert("fan_on".into(), Value::from(true));

        let (migrated, did_migrate) = AppConfig::parse(&raw.to_string()).unwrap();
        assert!(did_migrate);
        assert_eq!(migrated.version, CONFIG_VERSION);
        assert_eq!(migrated.cooling_pad.fan_mode, "manual");
        assert!(migrated.cooling_pad.follow_laptop_fan);

        let (reloaded, did_migrate_again) =
            AppConfig::parse(&serde_json::to_string(&migrated).unwrap()).unwrap();
        assert!(!did_migrate_again);
        assert_eq!(reloaded, migrated);
    }

    #[test]
    fn cooling_pad_runtime_mapping_preserves_every_field() {
        let mut expected = CoolingPadConfig::default();
        expected.fan_mode = "manual".into();
        expected.manual_rpm = 1777;
        expected.auto_min_rpm = 611;
        expected.auto_max_rpm = 2999;
        expected.auto_off_below_c = 49.5;
        expected.auto_full_above_c = 79.5;
        expected.auto_turn_on_delay_secs = 3.25;
        expected.auto_turn_off_delay_secs = 8.25;
        expected.auto_overcool_hold_secs = 14.0;
        expected.auto_temp_ema_alpha = 0.55;
        expected.auto_rpm_slew_up_per_sec = 456;
        expected.auto_rpm_slew_down_per_sec = 321;
        expected.auto_follow_temp_margin_c = 6.5;
        expected.auto_temp_hysteresis_c = 2.25;
        expected.follow_laptop_fan = false;
        expected.lighting_mode = "Wave".into();
        expected.color = [9, 8, 7];
        expected.brightness_step = 3;

        let runtime = CoolingPadRuntime::from(&expected);
        assert_eq!(CoolingPadConfig::from(runtime), expected);
    }

    #[test]
    fn atomic_write_replaces_existing_config() {
        let unique = format!(
            "r-helper-config-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        );
        let directory = std::env::temp_dir().join(unique);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("config.json");
        fs::write(&path, b"old").unwrap();

        write_config_atomically(&path, b"new").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert!(!path.with_extension("json.tmp").exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
