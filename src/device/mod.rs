// Device domain types and helpers
use std::time::Duration;

use anyhow::Result;
use librazer::types::{
    BatteryCare, CpuBoost, FanMode, GpuBoost, LightsAlwaysOn, LogoMode, MaxFanSpeedMode, PerfMode,
};
use librazer::{command, device};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompleteDeviceState {
    pub perf_mode: PerfMode,
    pub fan_mode: FanMode,
    pub fan_rpm: Option<u16>,
    pub logo_mode: LogoMode,
    pub keyboard_brightness: u8,
    pub lights_always_on: LightsAlwaysOn,
    pub battery_care: BatteryCare,
    #[serde(default)]
    pub cpu_boost: Option<CpuBoost>,
    #[serde(default)]
    pub gpu_boost: Option<GpuBoost>,
    #[serde(default)]
    pub max_fan_speed: Option<MaxFanSpeedMode>,
}

impl Default for CompleteDeviceState {
    fn default() -> Self {
        Self {
            perf_mode: PerfMode::Performance,
            fan_mode: FanMode::Auto,
            fan_rpm: None,
            logo_mode: LogoMode::Off,
            keyboard_brightness: 50,
            lights_always_on: LightsAlwaysOn::Disable,
            battery_care: BatteryCare::Percent80,
            cpu_boost: None,
            gpu_boost: None,
            max_fan_speed: None,
        }
    }
}

impl CompleteDeviceState {
    pub fn read_from_device(device: &device::Device) -> Result<Self> {
        let (perf_mode, fan_mode) = command::get_perf_mode(device)?;
        let fan_rpm = match fan_mode {
            FanMode::Manual => Some(command::get_fan_rpm(device, librazer::types::FanZone::Zone1)?),
            FanMode::Auto => None,
        };
        let logo_mode = command::get_logo_mode(device)?;
        let keyboard_brightness = command::get_keyboard_brightness(device)?;
        let lights_always_on = command::get_lights_always_on(device)?;
        let battery_care = command::get_battery_care(device)?;

        let (cpu_boost, gpu_boost, max_fan_speed) = if perf_mode == PerfMode::Custom {
            (
                command::get_cpu_boost(device).ok(),
                command::get_gpu_boost(device).ok(),
                command::get_max_fan_speed_mode(device).ok(),
            )
        } else {
            (None, None, None)
        };

        Ok(Self {
            perf_mode,
            fan_mode,
            fan_rpm,
            logo_mode,
            keyboard_brightness,
            lights_always_on,
            battery_care,
            cpu_boost,
            gpu_boost,
            max_fan_speed,
        })
    }

    pub fn apply_to_device(&self, device: &device::Device) -> Result<()> {
        command::set_perf_mode(device, self.perf_mode)?;

        if self.perf_mode == PerfMode::Custom {
            if let Some(cpu) = self.cpu_boost {
                command::set_cpu_boost(device, cpu)?;
            }
            if let Some(gpu) = self.gpu_boost {
                command::set_gpu_boost(device, gpu)?;
            }
            if let Some(max_fan) = self.max_fan_speed {
                command::set_max_fan_speed_mode(device, max_fan)?;
            }
        }

        match self.fan_mode {
            FanMode::Auto => {}
            FanMode::Manual => {
                command::set_fan_mode(device, FanMode::Manual)?;
                std::thread::sleep(Duration::from_millis(50));
                if let Some(rpm) = self.fan_rpm {
                    command::set_fan_rpm(device, rpm, true)?;
                }
            }
        }

        command::set_logo_mode(device, self.logo_mode)?;

        if let Ok(current_brightness) = command::get_keyboard_brightness(device) {
            if current_brightness != self.keyboard_brightness {
                command::set_keyboard_brightness(device, self.keyboard_brightness)?;
            }
        } else {
            command::set_keyboard_brightness(device, self.keyboard_brightness)?;
        }

        command::set_lights_always_on(device, self.lights_always_on)?;
        command::set_battery_care(device, self.battery_care)?;

        Ok(())
    }
}
