use librazer::{command, device::Device, types::FanMode};

use crate::system::thermal::ThermalSnapshot;

pub const LAPTOP_FAN_CAP_HYSTERESIS: u16 = 100;
pub const LAPTOP_FAN_CAP_RELEASE_TEMP_C: f32 = 65.0;
pub const LAPTOP_FAN_CAP_ENGAGE_DWELL_SECS: f32 = 1.5;
pub const LAPTOP_FAN_CAP_PROBE_INTERVAL_SECS: f32 = 5.0;

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
    /// Auto actual >= max, accumulating before first engage.
    engage_dwell_secs: f32,
    /// Time since cap engaged (for periodic Auto probe).
    capped_secs: f32,
    /// After releasing to Auto, wait one sample to recap or drop the cap.
    probe_pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeDecision {
    StayAuto,
    Recap,
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

fn reset_cap_timers(state: &mut CapEnforcerState) {
    state.engage_dwell_secs = 0.0;
    state.capped_secs = 0.0;
    state.probe_pending = false;
}

/// Returns true when peak is cool enough to probe/release immediately (no 3s dwell while capped).
pub(crate) fn peak_cool_immediate_release(thermal: &ThermalSnapshot) -> bool {
    peak_temp_c(thermal).is_some_and(|peak| peak < LAPTOP_FAN_CAP_RELEASE_TEMP_C)
}

/// Accumulate engage dwell; returns true when the cap should engage.
pub(crate) fn tick_engage_dwell(
    state: &mut CapEnforcerState,
    actual: u16,
    max: u16,
    dt_secs: f32,
) -> bool {
    if actual < max {
        state.engage_dwell_secs = 0.0;
        return false;
    }
    state.engage_dwell_secs += dt_secs;
    state.engage_dwell_secs >= LAPTOP_FAN_CAP_ENGAGE_DWELL_SECS
}

/// After an Auto probe sample, decide whether to recap or leave Auto.
pub(crate) fn probe_decision(actual: u16, max: u16) -> ProbeDecision {
    if actual >= max {
        ProbeDecision::Recap
    } else if actual.saturating_add(LAPTOP_FAN_CAP_HYSTERESIS) < max {
        ProbeDecision::StayAuto
    } else {
        // In the hysteresis band below max — treat as safe to stay on Auto.
        ProbeDecision::StayAuto
    }
}

/// Whether to release Manual and probe Auto while capped.
pub(crate) fn should_probe_auto(
    state: &CapEnforcerState,
    thermal: &ThermalSnapshot,
    _dt_secs: f32,
) -> bool {
    if state.probe_pending {
        return false;
    }
    if peak_cool_immediate_release(thermal) {
        return true;
    }
    state.capped_secs >= LAPTOP_FAN_CAP_PROBE_INTERVAL_SECS
}

fn try_release_to_auto(device: &Device) -> bool {
    if command::set_fan_mode(device, FanMode::Auto).is_ok() {
        true
    } else {
        eprintln!("laptop fan cap: failed to restore Auto mode for probe");
        false
    }
}

fn engage_cap(device: &Device, cap: &mut LaptopFanCapShared, state: &mut CapEnforcerState) -> bool {
    if command::set_fan_mode(device, FanMode::Manual).is_ok()
        && command::set_fan_rpm(device, cap.max_rpm, true).is_ok()
    {
        cap.cap_active = true;
        state.engage_dwell_secs = 0.0;
        state.capped_secs = 0.0;
        state.probe_pending = false;
        true
    } else {
        false
    }
}

fn start_auto_probe(device: &Device, state: &mut CapEnforcerState) {
    if try_release_to_auto(device) {
        state.probe_pending = true;
        state.capped_secs = 0.0;
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
        reset_cap_timers(state);
        return;
    }

    let max = cap.max_rpm;

    // User took over Manual at a different RPM — drop our cap state.
    if fan.fan_mode == FanMode::Manual {
        if cap.cap_active && fan.fan_set_rpm != Some(max) {
            cap.cap_active = false;
            reset_cap_timers(state);
        }
        return;
    }

    if state.probe_pending {
        if fan.fan_mode != FanMode::Auto {
            return;
        }
        let Some(actual) = fan.fan_actual_rpm else {
            return;
        };
        match probe_decision(actual, max) {
            ProbeDecision::Recap => {
                if engage_cap(device, cap, state) {
                    state.probe_pending = false;
                }
            }
            ProbeDecision::StayAuto => {
                cap.cap_active = false;
                state.probe_pending = false;
                reset_cap_timers(state);
            }
        }
        return;
    }

    if cap.cap_active {
        state.capped_secs += dt_secs;

        if fan.fan_mode == FanMode::Auto {
            // Cap flag set but hardware already Auto — decide from this sample.
            let Some(actual) = fan.fan_actual_rpm else {
                return;
            };
            match probe_decision(actual, max) {
                ProbeDecision::Recap => {
                    let _ = engage_cap(device, cap, state);
                }
                ProbeDecision::StayAuto => {
                    cap.cap_active = false;
                    reset_cap_timers(state);
                }
            }
            return;
        }

        // Capped on Manual@max: probe periodically instead of rewriting RPM every poll.
        if should_probe_auto(state, thermal, dt_secs) {
            start_auto_probe(device, state);
            return;
        }

        // Only correct hardware if it left Manual@max while still capped and still hot.
        let needs_reclip = fan.fan_set_rpm != Some(max)
            || fan.fan_actual_rpm.is_some_and(|actual| actual > max.saturating_add(LAPTOP_FAN_CAP_HYSTERESIS));
        if needs_reclip {
            let _ = engage_cap(device, cap, state);
        }
        return;
    }

    if fan.fan_mode != FanMode::Auto {
        state.engage_dwell_secs = 0.0;
        return;
    }

    let Some(actual) = fan.fan_actual_rpm else {
        return;
    };

    if tick_engage_dwell(state, actual, max, dt_secs) {
        let _ = engage_cap(device, cap, state);
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
    fn single_sample_at_max_does_not_engage() {
        let mut state = CapEnforcerState::default();
        assert!(!tick_engage_dwell(&mut state, 3200, 3200, 0.8));
        assert!(state.engage_dwell_secs > 0.0);
    }

    #[test]
    fn dwell_then_engage() {
        let mut state = CapEnforcerState::default();
        assert!(!tick_engage_dwell(&mut state, 3300, 3200, 0.5));
        assert!(!tick_engage_dwell(&mut state, 3300, 3200, 0.5));
        assert!(tick_engage_dwell(&mut state, 3300, 3200, 0.5));
    }

    #[test]
    fn engage_dwell_resets_when_auto_drops_below_max() {
        let mut state = CapEnforcerState::default();
        tick_engage_dwell(&mut state, 3300, 3200, 1.0);
        assert!(!tick_engage_dwell(&mut state, 3000, 3200, 0.1));
        assert_eq!(state.engage_dwell_secs, 0.0);
    }

    #[test]
    fn probe_stays_auto_when_below_max_with_hysteresis() {
        assert_eq!(probe_decision(3000, 3200), ProbeDecision::StayAuto);
        assert_eq!(probe_decision(3099, 3200), ProbeDecision::StayAuto);
    }

    #[test]
    fn probe_recaps_when_at_or_above_max() {
        assert_eq!(probe_decision(3200, 3200), ProbeDecision::Recap);
        assert_eq!(probe_decision(3300, 3200), ProbeDecision::Recap);
    }

    #[test]
    fn should_probe_after_interval() {
        let state = CapEnforcerState { capped_secs: 5.0, ..Default::default() };
        let hot = ThermalSnapshot { cpu_avg_c: Some(76.0), gpu_avg_c: Some(60.0) };
        assert!(should_probe_auto(&state, &hot, 0.8));
    }

    #[test]
    fn should_probe_immediately_when_cool() {
        let state = CapEnforcerState { capped_secs: 0.5, ..Default::default() };
        let cool = ThermalSnapshot { cpu_avg_c: Some(57.0), gpu_avg_c: None };
        assert!(peak_cool_immediate_release(&cool));
        assert!(should_probe_auto(&state, &cool, 0.8));
    }

    #[test]
    fn should_not_probe_when_pending() {
        let state = CapEnforcerState { probe_pending: true, capped_secs: 10.0, ..Default::default() };
        let cool = ThermalSnapshot { cpu_avg_c: Some(50.0), gpu_avg_c: None };
        assert!(!should_probe_auto(&state, &cool, 0.8));
    }
}
