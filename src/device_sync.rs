use librazer::types::{
    BatteryCare, CpuBoost, FanMode, GpuBoost, LightsAlwaysOn, LogoMode, PerfMode,
};

use crate::device::CompleteDeviceState;
use crate::ui;

#[derive(Debug, Clone)]
pub struct DeviceStatus {
    pub perf_mode: Option<PerfMode>,
    pub fan_mode: Option<FanMode>,
    pub fan_rpm: Option<u16>,
    pub fan_actual_rpm: Option<u16>,
    pub logo_mode: Option<LogoMode>,
    pub keyboard_brightness: u8,
    pub lights_always_on: bool,
    pub battery_care: BatteryCare,
}

impl Default for DeviceStatus {
    fn default() -> Self {
        Self {
            perf_mode: None,
            fan_mode: None,
            fan_rpm: None,
            fan_actual_rpm: None,
            logo_mode: None,
            keyboard_brightness: 0,
            lights_always_on: false,
            battery_care: BatteryCare::Percent80,
        }
    }
}

impl DeviceStatus {
    pub fn performance_mode_label(&self) -> String {
        self.perf_mode
            .map(|m| perf_mode_to_string(m))
            .unwrap_or_else(|| "Reading...".to_string())
    }

    pub fn fan_speed_label(&self) -> String {
        match self.fan_mode {
            Some(FanMode::Auto) => "Auto".to_string(),
            Some(FanMode::Manual) => "Manual".to_string(),
            None => "Reading...".to_string(),
        }
    }

    pub fn logo_mode_label(&self) -> String {
        self.logo_mode
            .map(|m| logo_mode_to_string(m).to_string())
            .unwrap_or_else(|| "Reading...".to_string())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StatusApplyOptions {
    pub fan_only: bool,
    pub reset_auto_fan_cap_override: bool,
    pub respect_user_auto_mode: bool,
}

pub struct StatusApplyContext<'a> {
    pub status: &'a mut DeviceStatus,
    pub manual_fan_rpm: &'a mut u16,
    pub temp_brightness_step: &'a mut usize,
    pub cpu_boost: &'a mut CpuBoost,
    pub gpu_boost: &'a mut GpuBoost,
    pub auto_fan_cap_override: Option<&'a mut bool>,
}

pub fn perf_mode_to_string(mode: PerfMode) -> String {
    format!("{mode:?}")
}

pub fn string_to_perf_mode(mode: &str) -> Option<PerfMode> {
    match mode {
        "Silent" => Some(PerfMode::Silent),
        "Balanced" => Some(PerfMode::Balanced),
        "Performance" => Some(PerfMode::Performance),
        "Custom" => Some(PerfMode::Custom),
        "Battery" => Some(PerfMode::Battery),
        "Hyperboost" => Some(PerfMode::Hyperboost),
        _ => PerfMode::iter().find(|m| format!("{m:?}") == mode),
    }
}

pub fn logo_mode_to_string(mode: LogoMode) -> &'static str {
    match mode {
        LogoMode::Static => "Static",
        LogoMode::Breathing => "Breathing",
        LogoMode::Off => "Off",
    }
}

pub fn string_to_logo_mode(mode: &str) -> Option<LogoMode> {
    match mode {
        "Static" => Some(LogoMode::Static),
        "Breathing" => Some(LogoMode::Breathing),
        "Off" => Some(LogoMode::Off),
        _ => None,
    }
}


pub fn apply_fan_status(
    ctx: &mut StatusApplyContext<'_>,
    fan_mode: FanMode,
    set_rpm: Option<u16>,
    is_user_auto_mode: bool,
) -> bool {
    if is_user_auto_mode {
        return false;
    }
    let fan_rpm = match fan_mode {
        FanMode::Auto => None,
        FanMode::Manual => set_rpm,
    };
    let changed = ctx.status.fan_mode != Some(fan_mode) || ctx.status.fan_rpm != fan_rpm;
    ctx.status.fan_mode = Some(fan_mode);
    ctx.status.fan_rpm = fan_rpm;
    if let Some(rpm) = fan_rpm {
        *ctx.manual_fan_rpm = rpm;
    }
    changed
}

pub fn apply_state_to_status(
    ctx: &mut StatusApplyContext<'_>,
    state: &CompleteDeviceState,
    fan_actual_rpm: Option<u16>,
    options: StatusApplyOptions,
    is_user_auto_mode: bool,
) {
    if options.reset_auto_fan_cap_override {
        if let Some(override_flag) = ctx.auto_fan_cap_override.as_deref_mut() {
            *override_flag = false;
        }
    }

    if !options.fan_only {
        ctx.status.perf_mode = Some(state.perf_mode);
        ctx.status.logo_mode = Some(state.logo_mode);
        ctx.status.keyboard_brightness = state.keyboard_brightness;
        *ctx.temp_brightness_step =
            ui::lighting::raw_brightness_to_step_index(state.keyboard_brightness);
        ctx.status.lights_always_on = matches!(state.lights_always_on, LightsAlwaysOn::Enable);
        ctx.status.battery_care = state.battery_care;

        if let Some(cpu) = state.cpu_boost {
            *ctx.cpu_boost = cpu;
        }
        if let Some(gpu) = state.gpu_boost {
            *ctx.gpu_boost = gpu;
        }
    }

    if !options.respect_user_auto_mode || !is_user_auto_mode {
        ctx.status.fan_mode = Some(state.fan_mode);
        ctx.status.fan_rpm = match state.fan_mode {
            FanMode::Auto => None,
            FanMode::Manual => state.fan_rpm,
        };
        if let Some(rpm) = state.fan_rpm {
            *ctx.manual_fan_rpm = rpm;
        }
    }

    if let Some(rpm) = fan_actual_rpm {
        ctx.status.fan_actual_rpm = Some(rpm);
    }
}

use strum::IntoEnumIterator;
