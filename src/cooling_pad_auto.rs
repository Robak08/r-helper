use librazer::cooling_pad::{MAX_RPM, MIN_RPM, RPM_STEP};

const LAPTOP_FAN_MAX_RPM: f32 = 5500.0;

/// Default seconds smoothed temp must stay above threshold before the fan turns on.
pub const DEFAULT_TURN_ON_DELAY_SECS: f32 = 5.0;
/// Default seconds smoothed temp must stay below threshold before the fan turns off.
pub const DEFAULT_TURN_OFF_DELAY_SECS: f32 = 10.0;
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
    pub min_rpm: u16,
    pub max_rpm: u16,
    pub off_below_c: f32,
    pub full_above_c: f32,
    pub temp_hysteresis_c: f32,
    /// Seconds since the last auto update (used for dwell timers and slew).
    pub dt_secs: f32,
    pub turn_on_delay_secs: f32,
    pub turn_off_delay_secs: f32,
    pub temp_ema_alpha: f32,
    pub rpm_slew_up_per_sec: u16,
    pub rpm_slew_down_per_sec: u16,
    pub follow_temp_margin_c: f32,
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
}

/// Combined auto: debounced temperature curve + gated laptop fan follow, with smooth RPM ramp.
pub fn compute_combined_auto(
    inputs: &CoolingPadAutoInputs,
    state: &mut CoolingPadAutoState,
) -> CoolingPadAutoOutput {
    let dt = inputs.dt_secs.max(0.05);
    let alpha = inputs.temp_ema_alpha.clamp(0.05, 1.0);

    state.smoothed_cpu_c = ema_update(state.smoothed_cpu_c, inputs.cpu_temp_c, alpha);
    state.smoothed_gpu_c = ema_update(state.smoothed_gpu_c, inputs.gpu_temp_c, alpha);
    state.smoothed_laptop_rpm = ema_update(
        state.smoothed_laptop_rpm,
        inputs.laptop_fan_actual_rpm.map(|r| r as f32),
        alpha,
    );

    let min_rpm = inputs.min_rpm.clamp(MIN_RPM, MAX_RPM);
    let max_rpm = inputs.max_rpm.clamp(min_rpm, MAX_RPM);
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
            state.fan_running = false;
            state.last_rpm = None;
            state.warm_dwell_secs = 0.0;
            state.cool_dwell_secs = 0.0;
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
        scale_laptop_fan_rpm(rpm.round() as u16, min_rpm, max_rpm)
    });

    let raw_target = if peak < off_below + follow_margin {
        temp_rpm
    } else {
        match follow_rpm {
            Some(f) => temp_rpm.max(f),
            None => temp_rpm,
        }
    };

    let target = round_rpm(raw_target.clamp(min_rpm, max_rpm));
    let up_step = ((inputs.rpm_slew_up_per_sec as f32) * dt).round() as u16;
    let down_step = ((inputs.rpm_slew_down_per_sec as f32) * dt).round() as u16;

    let applied = match state.last_rpm {
        Some(current) => slew_toward(current, target, up_step.max(1), down_step.max(1)),
        None => min_rpm.min(target),
    };

    state.fan_running = true;
    state.last_rpm = Some(applied);
    CoolingPadAutoOutput::Rpm(applied)
}

fn ema_update(prev: Option<f32>, sample: Option<f32>, alpha: f32) -> Option<f32> {
    match (prev, sample) {
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

/// Quadratic ease-in: stays near min RPM when only slightly above the off threshold.
fn temp_to_rpm(temp: f32, off_below: f32, full_above: f32, min_rpm: u16, max_rpm: u16) -> u16 {
    if temp <= off_below {
        return min_rpm;
    }
    if temp >= full_above {
        return max_rpm;
    }
    let t = ((temp - off_below) / (full_above - off_below)).clamp(0.0, 1.0);
    let eased = t * t;
    let rpm = min_rpm as f32 + eased * (max_rpm - min_rpm) as f32;
    round_rpm(rpm as u16)
}

fn scale_laptop_fan_rpm(laptop_rpm: u16, min_rpm: u16, max_rpm: u16) -> u16 {
    let scaled = (laptop_rpm as f32 / LAPTOP_FAN_MAX_RPM) * (max_rpm - min_rpm) as f32
        + min_rpm as f32;
    round_rpm(scaled.clamp(min_rpm as f32, max_rpm as f32) as u16)
}

fn slew_toward(current: u16, target: u16, up_step: u16, down_step: u16) -> u16 {
    if target > current {
        current.saturating_add(up_step).min(target)
    } else {
        current.saturating_sub(down_step).max(target)
    }
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
            min_rpm: 500,
            max_rpm: 3200,
            off_below_c: 55.0,
            full_above_c: 85.0,
            temp_hysteresis_c: 3.0,
            dt_secs: 1.0,
            turn_on_delay_secs: 0.0,
            turn_off_delay_secs: 0.0,
            temp_ema_alpha: 1.0,
            rpm_slew_up_per_sec: 10_000,
            rpm_slew_down_per_sec: 10_000,
            follow_temp_margin_c: 0.0,
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
    fn slight_warmth_stays_near_min_rpm() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.cpu_temp_c = Some(57.0);
        inp.gpu_temp_c = Some(56.0);
        inp.laptop_fan_actual_rpm = Some(5000);
        inp.follow_temp_margin_c = 10.0;
        inp.turn_on_delay_secs = 0.0;
        inp.temp_ema_alpha = 1.0;
        inp.rpm_slew_up_per_sec = 10_000;

        let CoolingPadAutoOutput::Rpm(rpm) = tick(&inp, &mut state) else {
            panic!("expected rpm");
        };
        assert!(rpm <= 700, "expected low rpm for slight warmth, got {rpm}");
    }

    #[test]
    fn slew_limits_ramp_up() {
        let mut state = CoolingPadAutoState::default();
        let mut inp = inputs();
        inp.turn_on_delay_secs = 0.0;
        inp.temp_ema_alpha = 1.0;
        inp.rpm_slew_up_per_sec = 100;
        inp.rpm_slew_down_per_sec = 10_000;
        inp.laptop_fan_actual_rpm = None;

        let _ = tick(&inp, &mut state);
        let first = state.last_rpm.unwrap();

        inp.cpu_temp_c = Some(85.0);
        let CoolingPadAutoOutput::Rpm(second) = tick(&inp, &mut state) else {
            panic!("expected rpm");
        };
        assert!(second <= first + 100);
        assert!(second < 3200);
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
}
