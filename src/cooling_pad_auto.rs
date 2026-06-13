use librazer::cooling_pad::{MAX_RPM, MIN_RPM, RPM_STEP};

const LAPTOP_FAN_MAX_RPM: f32 = 5500.0;
/// Hold min RPM through the lower half of the off→full range.
const RAMP_PLATEAU_FRACTION: f32 = 0.5;
/// At most this many °C above off-below before the RPM ramp begins (whichever is sooner).
pub const DEFAULT_PLATEAU_MAX_ABOVE_C: f32 = 10.0;

/// Default seconds smoothed temp must stay above threshold before the fan turns on.
pub const DEFAULT_TURN_ON_DELAY_SECS: f32 = 5.0;
/// Default seconds smoothed temp must stay below threshold before the fan turns off.
pub const DEFAULT_TURN_OFF_DELAY_SECS: f32 = 10.0;
/// After RPM increases, hold that speed this long before allowing a decrease.
pub const DEFAULT_OVERCOOL_HOLD_SECS: f32 = 10.0;
/// EMA weight for new temperature samples (0–1). Lower = smoother, slower to react.
pub const DEFAULT_TEMP_EMA_ALPHA: f32 = 0.25;
/// Max RPM increase per second when ramping up.
pub const DEFAULT_RPM_SLEW_UP_PER_SEC: u16 = 150;
/// Max RPM decrease per second when ramping down.
pub const DEFAULT_RPM_SLEW_DOWN_PER_SEC: u16 = 250;
/// Laptop fan follow only applies once temp is this many °C above the off-below point.
pub const DEFAULT_FOLLOW_TEMP_MARGIN_C: f32 = 10.0;
/// Temperature hysteresis when deciding whether the fan should turn off.
pub const DEFAULT_TEMP_HYSTERESIS_C: f32 = 3.0;
/// Reject upward EMA steps larger than this (°C) — blocks false hot spikes from driving RPM.
pub const DEFAULT_TEMP_SPIKE_REJECT_C: f32 = 12.0;
/// When the filtered reading is this much below smoothed, trust it immediately (stuck-high recovery).
pub const DEFAULT_TEMP_FAST_COOL_C: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoolingPadAutoOutput {
    Off,
    Rpm(u16),
}

#[derive(Debug, Clone, Copy)]
pub struct CoolingPadAutoInputs {
    pub cpu_temp_c: Option<f32>,
    pub gpu_temp_c: Option<f32>,
    pub laptop_fan_actual_rpm: Option<u16>,
    /// When set, laptop fan follow uses `min(actual, cap)` so pad auto respects laptop auto max.
    pub laptop_fan_cap_rpm: Option<u16>,
    pub min_rpm: u16,
    pub max_rpm: u16,
    pub off_below_c: f32,
    pub full_above_c: f32,
    pub temp_hysteresis_c: f32,
    /// Seconds since the last auto update (used for dwell timers and slew).
    pub dt_secs: f32,
    pub turn_on_delay_secs: f32,
    pub turn_off_delay_secs: f32,
    pub overcool_hold_secs: f32,
    pub temp_ema_alpha: f32,
    pub rpm_slew_up_per_sec: u16,
    pub rpm_slew_down_per_sec: u16,
    pub follow_temp_margin_c: f32,
    /// When false, pad auto ignores laptop fan speed (user manual laptop fan, etc.).
    pub laptop_fan_follow_enabled: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CoolingPadAutoState {
    pub fan_running: bool,
    pub last_rpm: Option<u16>,
    smoothed_cpu_c: Option<f32>,
    smoothed_gpu_c: Option<f32>,
    smoothed_laptop_rpm: Option<f32>,
    warm_dwell_secs: f32,
    cool_dwell_secs: f32,
    overcool_hold_rpm: Option<u16>,
    overcool_secs_left: f32,
}

/// Combined auto: debounced temperature curve + gated laptop fan follow, with smooth RPM ramp.
pub fn compute_combined_auto(
    inputs: &CoolingPadAutoInputs,
    state: &mut CoolingPadAutoState,
) -> CoolingPadAutoOutput {
    let dt = inputs.dt_secs.max(0.05);
    let alpha = inputs.temp_ema_alpha.clamp(0.05, 1.0);

    let spike_reject = DEFAULT_TEMP_SPIKE_REJECT_C;
    let cpu_sample = reconcile_cooler_sample(state.smoothed_cpu_c, inputs.cpu_temp_c);
    let gpu_sample = reconcile_cooler_sample(state.smoothed_gpu_c, inputs.gpu_temp_c);
    state.smoothed_cpu_c =
        ema_update_temp(state.smoothed_cpu_c, cpu_sample, alpha, spike_reject);
    state.smoothed_gpu_c =
        ema_update_temp(state.smoothed_gpu_c, gpu_sample, alpha, spike_reject);
    let laptop_sample = if inputs.laptop_fan_follow_enabled {
        inputs.laptop_fan_actual_rpm.map(|rpm| match inputs.laptop_fan_cap_rpm {
            Some(cap) => rpm.min(cap),
            None => rpm,
        })
    } else {
        None
    };
    state.smoothed_laptop_rpm = ema_update(
        state.smoothed_laptop_rpm,
        laptop_sample.map(|r| r as f32),
        alpha,
    );

    let min_rpm = round_rpm(inputs.min_rpm.clamp(MIN_RPM, MAX_RPM));
    let max_rpm = round_rpm(inputs.max_rpm.clamp(min_rpm, MAX_RPM));
    clamp_auto_state_to_limits(state, min_rpm, max_rpm);
    let off_below = inputs.off_below_c;
    let full_above = inputs.full_above_c.max(off_below + 1.0);
    let temp_hyst = inputs.temp_hysteresis_c.max(0.0);
    let follow_margin = inputs.follow_temp_margin_c.max(0.0);

    let peak = peak_temp(state.smoothed_cpu_c, state.smoothed_gpu_c);

    if let Some(temp) = peak {
        let off_threshold = if state.fan_running {
            off_below - temp_hyst
        } else {
            off_below
        };

        if temp >= off_threshold {
            state.warm_dwell_secs += dt;
            state.cool_dwell_secs = 0.0;
        } else {
            state.cool_dwell_secs += dt;
            state.warm_dwell_secs = 0.0;
        }

        if !state.fan_running {
            if temp < off_threshold || state.warm_dwell_secs < inputs.turn_on_delay_secs {
                return CoolingPadAutoOutput::Off;
            }
        } else if temp < off_threshold
            && state.cool_dwell_secs >= inputs.turn_off_delay_secs
        {
            reset_running_state(state);
            return CoolingPadAutoOutput::Off;
        }
    } else if !state.fan_running {
        state.warm_dwell_secs = 0.0;
        state.cool_dwell_secs = 0.0;
        return CoolingPadAutoOutput::Off;
    }

    let peak = peak.unwrap_or(off_below);
    let temp_rpm = temp_to_rpm(peak, off_below, full_above, min_rpm, max_rpm);

    let follow_rpm = state.smoothed_laptop_rpm.map(|rpm| {
        scale_laptop_fan_rpm(round_rpm(rpm.round() as u16), min_rpm, max_rpm)
    });

    let mut target = if peak < off_below + follow_margin {
        temp_rpm
    } else {
        match follow_rpm {
            Some(f) => temp_rpm.max(f),
            None => temp_rpm,
        }
    };
    target = round_rpm(target.clamp(min_rpm, max_rpm));

    if target > state.overcool_hold_rpm.unwrap_or(0) {
        state.overcool_hold_rpm = Some(target);
        state.overcool_secs_left = inputs.overcool_hold_secs.max(0.0);
    }

    target = apply_overcool_hold(state, target, dt, min_rpm, max_rpm);

    let up_step = slew_step(inputs.rpm_slew_up_per_sec, dt);
    let down_step = slew_step(inputs.rpm_slew_down_per_sec, dt);

    let applied = match state.last_rpm {
        Some(current) => round_rpm(slew_toward(current, target, up_step, down_step)),
        None => min_rpm.min(target),
    };

    let applied = round_rpm(applied.clamp(min_rpm, max_rpm));

    state.fan_running = true;
    state.last_rpm = Some(applied);
    CoolingPadAutoOutput::Rpm(applied)
}

fn reset_running_state(state: &mut CoolingPadAutoState) {
    state.fan_running = false;
    state.last_rpm = None;
    state.warm_dwell_secs = 0.0;
    state.cool_dwell_secs = 0.0;
    state.overcool_hold_rpm = None;
    state.overcool_secs_left = 0.0;
    state.smoothed_laptop_rpm = None;
}

/// Clear laptop-follow smoothing after the user takes manual control of the laptop fan.
pub fn clear_laptop_follow_smoothing(state: &mut CoolingPadAutoState) {
    state.smoothed_laptop_rpm = None;
}

fn apply_overcool_hold(
    state: &mut CoolingPadAutoState,
    target: u16,
    dt: f32,
    min_rpm: u16,
    max_rpm: u16,
) -> u16 {
    let target = round_rpm(target.clamp(min_rpm, max_rpm));
    let Some(mut hold) = state.overcool_hold_rpm else {
        return target;
    };
    hold = round_rpm(hold.clamp(min_rpm, max_rpm));
    state.overcool_hold_rpm = Some(hold);

    if target >= hold {
        return target;
    }

    state.overcool_secs_left = (state.overcool_secs_left - dt).max(0.0);
    if state.overcool_secs_left > 0.0 {
        return hold;
    }

    state.overcool_hold_rpm = None;
    target
}

/// Re-clamp persisted RPM when min/max limits change (e.g. user lowers max in the UI).
pub fn clamp_auto_state_to_limits(state: &mut CoolingPadAutoState, min_rpm: u16, max_rpm: u16) {
    if let Some(hold) = state.overcool_hold_rpm {
        let clamped = round_rpm(hold.clamp(min_rpm, max_rpm));
        if clamped < hold {
            state.overcool_hold_rpm = Some(clamped);
            state.overcool_secs_left = 0.0;
        } else {
            state.overcool_hold_rpm = Some(clamped);
        }
    }
    if let Some(last) = state.last_rpm {
        state.last_rpm = Some(round_rpm(last.clamp(min_rpm, max_rpm)));
    }
}

fn ema_update(prev: Option<f32>, sample: Option<f32>, alpha: f32) -> Option<f32> {
    match (prev, sample) {
        (Some(p), Some(s)) => Some(p + alpha * (s - p)),
        (_, Some(s)) => Some(s),
        (p, None) => p,
    }
}

fn reconcile_cooler_sample(smoothed: Option<f32>, sample: Option<f32>) -> Option<f32> {
    match (smoothed, sample) {
        (Some(p), Some(s)) if p > s + DEFAULT_TEMP_FAST_COOL_C => Some(s),
        (_, sample) => sample,
    }
}

fn ema_update_temp(
    prev: Option<f32>,
    sample: Option<f32>,
    alpha: f32,
    spike_reject_c: f32,
) -> Option<f32> {
    match (prev, sample) {
        (Some(p), Some(s)) if s > p + spike_reject_c => Some(p),
        (Some(p), Some(s)) => Some(p + alpha * (s - p)),
        (_, Some(s)) => Some(s),
        (p, None) => p,
    }
}

fn peak_temp(cpu: Option<f32>, gpu: Option<f32>) -> Option<f32> {
    match (cpu, gpu) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// First half of the off→full range (capped at [`plateau_end_temp`]) stays at min RPM.
fn temp_to_rpm(temp: f32, off_below: f32, full_above: f32, min_rpm: u16, max_rpm: u16) -> u16 {
    if temp <= off_below {
        return min_rpm;
    }
    if temp >= full_above {
        return max_rpm;
    }

    let plateau_end = plateau_end_temp(off_below, full_above);
    if temp <= plateau_end {
        return min_rpm;
    }

    let t = ((temp - plateau_end) / (full_above - plateau_end)).clamp(0.0, 1.0);
    let eased = smoothstep(t);
    let rpm = min_rpm as f32 + eased * (max_rpm - min_rpm) as f32;
    round_rpm(rpm.round() as u16)
}

/// Min-RPM plateau ends at the sooner of half the temp range or 10 °C above off-below.
fn plateau_end_temp(off_below: f32, full_above: f32) -> f32 {
    let span = (full_above - off_below).max(0.0);
    let half_span = RAMP_PLATEAU_FRACTION * span;
    off_below + half_span.min(DEFAULT_PLATEAU_MAX_ABOVE_C)
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn scale_laptop_fan_rpm(laptop_rpm: u16, min_rpm: u16, max_rpm: u16) -> u16 {
    let scaled = (laptop_rpm as f32 / LAPTOP_FAN_MAX_RPM) * (max_rpm - min_rpm) as f32
        + min_rpm as f32;
    round_rpm(scaled.round() as u16)
}

fn slew_step(per_sec: u16, dt: f32) -> u16 {
    let raw = (per_sec as f32 * dt).round() as u16;
    round_rpm_step(raw).max(RPM_STEP)
}

fn slew_toward(current: u16, target: u16, up_step: u16, down_step: u16) -> u16 {
    if target > current {
        current.saturating_add(up_step).min(target)
    } else {
        current.saturating_sub(down_step).max(target)
    }
}

fn round_rpm_step(step: u16) -> u16 {
    ((step as f32 / RPM_STEP as f32).round() as u16 * RPM_STEP).max(RPM_STEP)
}

fn round_rpm(rpm: u16) -> u16 {
    ((rpm as f32 / RPM_STEP as f32).round() as u16 * RPM_STEP).clamp(MIN_RPM, MAX_RPM)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> CoolingPadAutoInputs {
        CoolingPadAutoInputs {
            cpu_temp_c: Some(70.0),
            gpu_temp_c: Some(65.0),
            laptop_fan_actual_rpm: Some(2000),
            laptop_fan_cap_rpm: None,
            min_rpm: 600,
            max_rpm: 3200,
            off_below_c: 55.0,
            full_above_c: 85.0,
            temp_hysteresis_c: 3.0,
            dt_secs: 1.0,
            turn_on_delay_secs: 0.0,
            turn_off_delay_secs: 0.0,
            overcool_hold_secs: 0.0,
            temp_ema_alpha: 1.0,
            rpm_slew_up_per_sec: 10_000,
            rpm_slew_down_per_sec: 10_000,
            follow_temp_margin_c: 0.0,
            laptop_fan_follow_enabled: true,
        }
    }

    fn tick(inp: &CoolingPadAutoInputs, state: &mut CoolingPadAutoState) -> CoolingPadAutoOutput {
        compute_combined_auto(inp, state)
    }

    #[test]
    fn stays_off_below_threshold() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.cpu_temp_c = Some(50.0);
        inp.gpu_temp_c = Some(48.0);
        assert_eq!(tick(&inp, &mut state), CoolingPadAutoOutput::Off);
    }

    #[test]
    fn turn_on_requires_dwell() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.cpu_temp_c = Some(60.0);
        inp.gpu_temp_c = Some(58.0);
        inp.turn_on_delay_secs = 5.0;
        inp.temp_ema_alpha = 1.0;

        for _ in 0..4 {
            assert_eq!(tick(&inp, &mut state), CoolingPadAutoOutput::Off);
        }
        assert!(matches!(tick(&inp, &mut state), CoolingPadAutoOutput::Rpm(_)));
    }

    #[test]
    fn plateau_holds_min_rpm_in_first_half_of_range() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.off_below_c = 50.0;
        inp.full_above_c = 60.0;
        inp.min_rpm = 600;
        inp.cpu_temp_c = Some(55.0);
        inp.gpu_temp_c = Some(54.0);
        inp.laptop_fan_actual_rpm = None;
        inp.turn_on_delay_secs = 0.0;
        inp.follow_temp_margin_c = 100.0;

        let CoolingPadAutoOutput::Rpm(rpm) = tick(&inp, &mut state) else {
            panic!("expected rpm");
        };
        assert_eq!(rpm, 600);
    }

    #[test]
    fn plateau_ramps_in_upper_half() {
        assert_eq!(temp_to_rpm(57.5, 50.0, 60.0, 600, 3200), 1900);
        assert_eq!(temp_to_rpm(60.0, 50.0, 60.0, 600, 3200), 3200);
    }

    #[test]
    fn plateau_end_is_half_range_or_ten_deg_above_off() {
        assert_eq!(plateau_end_temp(50.0, 60.0), 55.0);
        assert_eq!(plateau_end_temp(55.0, 85.0), 65.0);
        assert_eq!(temp_to_rpm(64.0, 55.0, 85.0, 600, 3200), 600);
        assert!(temp_to_rpm(70.0, 55.0, 85.0, 600, 3200) > 600);
    }

    #[test]
    fn all_outputs_are_rpm_step_multiples() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.off_below_c = 50.0;
        inp.full_above_c = 60.0;
        inp.min_rpm = 600;
        inp.cpu_temp_c = Some(58.0);
        inp.turn_on_delay_secs = 0.0;
        inp.rpm_slew_up_per_sec = 500;

        for _ in 0..20 {
            if let CoolingPadAutoOutput::Rpm(rpm) = tick(&inp, &mut state) {
                assert_eq!(rpm % RPM_STEP, 0);
            }
        }
    }

    #[test]
    fn overcool_holds_rpm_after_increase() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.off_below_c = 50.0;
        inp.full_above_c = 60.0;
        inp.min_rpm = 600;
        inp.cpu_temp_c = Some(59.0);
        inp.gpu_temp_c = Some(59.0);
        inp.turn_on_delay_secs = 0.0;
        inp.overcool_hold_secs = 10.0;
        inp.rpm_slew_up_per_sec = 10_000;
        inp.laptop_fan_actual_rpm = None;
        inp.follow_temp_margin_c = 100.0;

        for _ in 0..30 {
            tick(&inp, &mut state);
        }
        let held = state.last_rpm.unwrap();
        assert!(held > 600);

        inp.cpu_temp_c = Some(52.0);
        inp.gpu_temp_c = Some(52.0);
        for _ in 0..9 {
            tick(&inp, &mut state);
        }
        assert_eq!(state.last_rpm.unwrap(), held);

        for _ in 0..40 {
            tick(&inp, &mut state);
        }
        assert_eq!(state.last_rpm.unwrap(), 600);
    }

    #[test]
    fn lowered_max_clamps_overcool_hold() {
        let mut state = CoolingPadAutoState::default();
        state.overcool_hold_rpm = Some(3200);
        state.last_rpm = Some(3200);
        state.overcool_secs_left = 30.0;
        clamp_auto_state_to_limits(&mut state, 600, 1500);
        assert_eq!(state.overcool_hold_rpm, Some(1500));
        assert_eq!(state.last_rpm, Some(1500));
        assert_eq!(state.overcool_secs_left, 0.0);
    }

    #[test]
    fn laptop_cap_limits_follow_rpm() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.off_below_c = 50.0;
        inp.full_above_c = 60.0;
        inp.min_rpm = 600;
        inp.max_rpm = 3200;
        inp.cpu_temp_c = Some(75.0);
        inp.laptop_fan_actual_rpm = Some(5500);
        inp.laptop_fan_cap_rpm = Some(2000);
        inp.follow_temp_margin_c = 0.0;
        inp.turn_on_delay_secs = 0.0;
        inp.overcool_hold_secs = 0.0;
        inp.rpm_slew_up_per_sec = 10_000;

        let CoolingPadAutoOutput::Rpm(rpm) = tick(&inp, &mut state) else {
            panic!("expected rpm");
        };
        let uncapped_follow = scale_laptop_fan_rpm(5500, 600, 3200);
        let capped_follow = scale_laptop_fan_rpm(2000, 600, 3200);
        assert!(rpm <= capped_follow + 200);
        assert!(rpm < uncapped_follow);
    }

    #[test]
    fn slew_step_and_curve_helpers() {
        assert_eq!(slew_step(150, 1.0), 150);
        assert_eq!(slew_toward(600, 3200, 150, 2500), 750);
        assert_eq!(temp_to_rpm(60.0, 50.0, 60.0, 600, 3200), 3200);
    }

    #[test]
    fn slew_limits_ramp_up() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.off_below_c = 50.0;
        inp.full_above_c = 60.0;
        inp.min_rpm = 600;
        inp.cpu_temp_c = Some(55.0);
        inp.gpu_temp_c = Some(55.0);
        inp.turn_on_delay_secs = 0.0;
        inp.overcool_hold_secs = 0.0;
        inp.temp_ema_alpha = 1.0;
        inp.rpm_slew_up_per_sec = 150;
        inp.rpm_slew_down_per_sec = 10_000;
        inp.laptop_fan_actual_rpm = None;
        inp.follow_temp_margin_c = 100.0;

        let _ = tick(&inp, &mut state);
        assert_eq!(state.last_rpm.unwrap(), 600);

        inp.cpu_temp_c = Some(60.0);
        inp.gpu_temp_c = Some(60.0);
        let CoolingPadAutoOutput::Rpm(second) = tick(&inp, &mut state) else {
            panic!("expected rpm");
        };
        assert_eq!(second, 750);
    }

    #[test]
    fn hysteresis_keeps_fan_on_until_cooler() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.turn_off_delay_secs = 0.0;
        inp.temp_ema_alpha = 1.0;
        let _ = tick(&inp, &mut state);

        inp.cpu_temp_c = Some(54.0);
        inp.gpu_temp_c = Some(52.0);
        assert!(matches!(tick(&inp, &mut state), CoolingPadAutoOutput::Rpm(_)));

        inp.cpu_temp_c = Some(50.0);
        inp.gpu_temp_c = Some(48.0);
        assert_eq!(tick(&inp, &mut state), CoolingPadAutoOutput::Off);
    }

    #[test]
    fn combined_uses_higher_of_temp_and_follow_when_hot() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.cpu_temp_c = Some(75.0);
        inp.laptop_fan_actual_rpm = Some(5000);
        inp.follow_temp_margin_c = 5.0;
        inp.turn_on_delay_secs = 0.0;
        inp.temp_ema_alpha = 1.0;
        inp.rpm_slew_up_per_sec = 10_000;

        let mut out = CoolingPadAutoOutput::Off;
        for _ in 0..30 {
            out = tick(&inp, &mut state);
        }
        let CoolingPadAutoOutput::Rpm(rpm) = out else {
            panic!("expected rpm");
        };
        assert!(rpm >= 2000);
    }

    #[test]
    fn temp_spike_does_not_force_max_rpm() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.off_below_c = 60.0;
        inp.cpu_temp_c = Some(62.0);
        inp.gpu_temp_c = Some(62.0);
        inp.laptop_fan_actual_rpm = None;
        inp.follow_temp_margin_c = 100.0;
        inp.turn_on_delay_secs = 0.0;
        inp.overcool_hold_secs = 0.0;
        inp.temp_ema_alpha = 1.0;
        for _ in 0..2 {
            let _ = tick(&inp, &mut state);
        }
        let CoolingPadAutoOutput::Rpm(baseline) = tick(&inp, &mut state) else {
            panic!("expected rpm");
        };
        assert_eq!(baseline, 600);

        inp.cpu_temp_c = Some(92.0);
        let CoolingPadAutoOutput::Rpm(spike) = tick(&inp, &mut state) else {
            panic!("expected rpm");
        };
        assert_eq!(spike, baseline);
    }

    #[test]
    fn follow_disabled_ignores_laptop_rpm() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.cpu_temp_c = Some(55.0);
        inp.gpu_temp_c = Some(55.0);
        inp.laptop_fan_actual_rpm = Some(5500);
        inp.laptop_fan_follow_enabled = false;
        inp.follow_temp_margin_c = 0.0;
        inp.turn_on_delay_secs = 0.0;
        inp.overcool_hold_secs = 0.0;
        inp.temp_ema_alpha = 1.0;
        inp.rpm_slew_up_per_sec = 10_000;

        let CoolingPadAutoOutput::Rpm(rpm) = tick(&inp, &mut state) else {
            panic!("expected rpm");
        };
        let with_follow = {
            let mut s = CoolingPadAutoState::default();
            let mut i = inp;
            i.laptop_fan_follow_enabled = true;
            for _ in 0..5 {
                tick(&i, &mut s);
            }
            let CoolingPadAutoOutput::Rpm(r) = tick(&i, &mut s) else {
                panic!("expected rpm");
            };
            r
        };
        assert!(with_follow > rpm);
    }

    #[test]
    fn fast_cool_snaps_stuck_high_smoothed() {
        let mut state = CoolingPadAutoState::default();
        state.smoothed_cpu_c = Some(85.0);
        state.smoothed_gpu_c = Some(85.0);
        state.fan_running = true;
        state.last_rpm = Some(3200);
        state.overcool_hold_rpm = Some(3200);

        let mut inp = inputs();
        inp.off_below_c = 60.0;
        inp.full_above_c = 86.0;
        inp.cpu_temp_c = Some(65.0);
        inp.gpu_temp_c = Some(65.0);
        inp.laptop_fan_actual_rpm = None;
        inp.follow_temp_margin_c = 100.0;
        inp.turn_on_delay_secs = 0.0;
        inp.overcool_hold_secs = 0.0;
        inp.temp_ema_alpha = 0.25;
        inp.rpm_slew_down_per_sec = 10_000;

        let CoolingPadAutoOutput::Rpm(rpm) = tick(&inp, &mut state) else {
            panic!("expected rpm");
        };
        assert!(rpm < 3200);
    }
}
