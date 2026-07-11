use librazer::{command, device::Device, types::FanMode};

use crate::system::thermal::ThermalSnapshot;

pub const LAPTOP_FAN_CAP_HYSTERESIS: u16 = 100;
pub const LAPTOP_FAN_CAP_RELEASE_TEMP_C: f32 = 65.0;
pub const LAPTOP_FAN_CAP_COOL_DWELL_SECS: f32 = 3.0;

/// Laptop auto-max RPM cap enforced on a dedicated thread (works while gaming / in tray).
#[derive(Debug, Default)]
pub struct LaptopFanCapShared {
    pub limit_enabled: bool,
    pub max_rpm: u16,
    /// Skip enforcement while the user is dragging the max-RPM slider.
    pub skip: bool,
    pub cap_active: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LaptopFanState {
    fan_mode: FanMode,
    fan_set_rpm: Option<u16>,
    fan_actual_rpm: Option<u16>,
}

#[derive(Debug, Default)]
pub(crate) struct CapEnforcerState {
    cool_temp_dwell_secs: f32,
}

impl LaptopFanState {
    pub(crate) fn new(
        fan_mode: FanMode,
        fan_set_rpm: Option<u16>,
        fan_actual_rpm: Option<u16>,
    ) -> Self {
        Self { fan_mode, fan_set_rpm, fan_actual_rpm }
    }
}

fn peak_temp_c(thermal: &ThermalSnapshot) -> Option<f32> {
    match (thermal.cpu_avg_c, thermal.gpu_avg_c) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn should_release_cap(
    max: u16,
    actual: u16,
    thermal: &ThermalSnapshot,
    state: &mut CapEnforcerState,
    dt_secs: f32,
) -> bool {
    if actual <= max {
        return true;
    }

    if actual < max.saturating_sub(LAPTOP_FAN_CAP_HYSTERESIS) {
        return true;
    }

    if let Some(peak) = peak_temp_c(thermal) {
        if peak < LAPTOP_FAN_CAP_RELEASE_TEMP_C {
            state.cool_temp_dwell_secs += dt_secs;
            if state.cool_temp_dwell_secs >= LAPTOP_FAN_CAP_COOL_DWELL_SECS {
                return true;
            }
        } else {
            state.cool_temp_dwell_secs = 0.0;
        }
    }

    false
}

fn try_release_to_auto(device: &Device, cap: &mut LaptopFanCapShared) -> bool {
    if command::set_fan_mode(device, FanMode::Auto).is_ok() {
        cap.cap_active = false;
        true
    } else {
        eprintln!("laptop fan cap: failed to restore Auto mode");
        false
    }
}

/// Apply or release the auto-max cap while the user has laptop fan in Auto + limit enabled.
fn enforce_laptop_fan_cap(
    device: &Device,
    cap: &mut LaptopFanCapShared,
    fan: LaptopFanState,
    thermal: &ThermalSnapshot,
    state: &mut CapEnforcerState,
    dt_secs: f32,
) {
    if cap.skip {
        return;
    }

    if !cap.limit_enabled {
        cap.cap_active = false;
        state.cool_temp_dwell_secs = 0.0;
        return;
    }

    let max = cap.max_rpm;

    if fan.fan_mode == FanMode::Manual {
        if cap.cap_active && fan.fan_set_rpm == Some(max) {
            if let Err(e) = command::set_fan_rpm(device, max, true) {
                eprintln!("laptop fan cap: failed to maintain max RPM: {e}");
            }
            if let Some(actual) = fan.fan_actual_rpm {
                if should_release_cap(max, actual, thermal, state, dt_secs) {
                    let _ = try_release_to_auto(device, cap);
                }
            }
            return;
        }
        if cap.cap_active && fan.fan_set_rpm != Some(max) {
            cap.cap_active = false;
            state.cool_temp_dwell_secs = 0.0;
        }
        return;
    }

    let Some(actual) = fan.fan_actual_rpm else {
        if cap.cap_active {
            let _ = command::set_fan_rpm(device, max, true);
        }
        return;
    };

    if cap.cap_active {
        if fan.fan_mode != FanMode::Manual {
            if let Err(e) = command::set_fan_mode(device, FanMode::Manual) {
                eprintln!("laptop fan cap: failed to set Manual mode: {e}");
            }
        }
        if let Err(e) = command::set_fan_rpm(device, max, true) {
            eprintln!("laptop fan cap: failed to set max RPM: {e}");
        }

        if should_release_cap(max, actual, thermal, state, dt_secs) {
            let _ = try_release_to_auto(device, cap);
        }
        return;
    }

    state.cool_temp_dwell_secs = 0.0;

    if fan.fan_mode != FanMode::Auto {
        return;
    }

    if actual >= max
        && command::set_fan_mode(device, FanMode::Manual).is_ok()
        && command::set_fan_rpm(device, max, true).is_ok()
    {
        cap.cap_active = true;
    }
}

pub(crate) fn enforce_laptop_fan_cap_from_sample(
    device: &Device,
    cap: &mut LaptopFanCapShared,
    fan: LaptopFanState,
    thermal: &ThermalSnapshot,
    state: &mut CapEnforcerState,
    dt_secs: f32,
) {
    enforce_laptop_fan_cap(device, cap, fan, thermal, state, dt_secs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases_when_actual_at_or_below_max() {
        let thermal = ThermalSnapshot { cpu_avg_c: Some(57.0), gpu_avg_c: Some(55.0) };
        let mut state = CapEnforcerState::default();
        assert!(should_release_cap(4000, 4000, &thermal, &mut state, 0.8));
        assert!(should_release_cap(4000, 3900, &thermal, &mut state, 0.8));
    }

    #[test]
    fn releases_after_cool_temp_dwell() {
        let thermal = ThermalSnapshot { cpu_avg_c: Some(57.0), gpu_avg_c: None };
        let mut state = CapEnforcerState::default();
        assert!(!should_release_cap(4000, 4100, &thermal, &mut state, 1.0));
        assert!(!should_release_cap(4000, 4100, &thermal, &mut state, 1.0));
        assert!(should_release_cap(4000, 4100, &thermal, &mut state, 1.5));
    }

    #[test]
    fn no_release_when_hot_and_above_max() {
        let thermal = ThermalSnapshot { cpu_avg_c: Some(85.0), gpu_avg_c: Some(80.0) };
        let mut state = CapEnforcerState::default();
        assert!(!should_release_cap(4000, 4100, &thermal, &mut state, 5.0));
    }
}
