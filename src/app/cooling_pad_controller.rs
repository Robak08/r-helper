use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::Result;
use librazer::{
    chroma::Rgb,
    cooling_pad::{MAX_RPM, MIN_RPM, PadLightingMode},
};

use crate::config::CoolingPadRuntime;
use crate::cooling_pad_apply;
use crate::cooling_pad_handle::SharedCoolingPad;
use crate::ui::cooling_pad_fan::CoolingPadFanMode;
use crate::ui::lighting::BRIGHTNESS_LEVELS;

pub const CONFIG_SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
const MAX_APPLY_ATTEMPTS: u8 = 5;
const RETRY_BASE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApplyDomain {
    Fan,
    Lighting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    Ok,
    Busy,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyReason {
    Startup,
    Attach,
    User,
    LifecycleRestore,
}

#[derive(Debug)]
pub struct CoolingPadController {
    runtime: CoolingPadRuntime,
    persist_deadline: Option<Instant>,
    pending_domains: HashSet<ApplyDomain>,
    next_retry: Option<Instant>,
    retry_attempts: u8,
}

impl CoolingPadController {
    pub fn from_runtime(runtime: CoolingPadRuntime) -> Self {
        Self {
            runtime: normalize_runtime(runtime),
            persist_deadline: None,
            pending_domains: HashSet::new(),
            next_retry: None,
            retry_attempts: 0,
        }
    }

    pub fn runtime(&self) -> &CoolingPadRuntime {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut CoolingPadRuntime {
        &mut self.runtime
    }

    pub fn set_runtime(&mut self, runtime: CoolingPadRuntime) {
        self.runtime = normalize_runtime(runtime);
    }

    pub fn schedule_persist(&mut self) {
        self.persist_deadline = Some(Instant::now() + CONFIG_SAVE_DEBOUNCE);
    }

    pub fn persist_deadline(&self) -> Option<Instant> {
        self.persist_deadline
    }

    pub fn clear_persist_deadline(&mut self) {
        self.persist_deadline = None;
    }

    pub fn mark_persist_failed(&mut self, now: Instant) {
        self.persist_deadline = Some(now + CONFIG_SAVE_DEBOUNCE);
    }

    pub fn request_apply_all(&mut self, _reason: ApplyReason) {
        self.pending_domains.insert(ApplyDomain::Fan);
        self.pending_domains.insert(ApplyDomain::Lighting);
        self.next_retry = Some(Instant::now());
        self.retry_attempts = 0;
    }

    pub fn request_apply_domain(&mut self, domain: ApplyDomain, _reason: ApplyReason) {
        self.pending_domains.insert(domain);
        if self.next_retry.is_none() {
            self.next_retry = Some(Instant::now());
        }
    }

    pub fn note_apply_failure(&mut self, domain: ApplyDomain) {
        self.pending_domains.insert(domain);
        self.retry_attempts = self.retry_attempts.saturating_add(1);
        if self.retry_attempts < MAX_APPLY_ATTEMPTS {
            let backoff = RETRY_BASE * self.retry_attempts.max(1) as u32;
            self.next_retry = Some(Instant::now() + backoff);
        } else {
            self.pending_domains.remove(&domain);
            self.next_retry = None;
        }
    }

    pub fn clear_domain(&mut self, domain: ApplyDomain) {
        self.pending_domains.remove(&domain);
        if self.pending_domains.is_empty() {
            self.next_retry = None;
            self.retry_attempts = 0;
        }
    }

    pub fn has_pending_apply(&self) -> bool {
        !self.pending_domains.is_empty()
    }

    pub fn should_run_apply(&self, now: Instant) -> bool {
        if self.pending_domains.is_empty() {
            return false;
        }
        self.next_retry.is_none_or(|deadline| now >= deadline)
    }

    pub fn fan_mode(&self) -> CoolingPadFanMode {
        CoolingPadFanMode::from_config(&self.runtime.fan_mode)
    }

    pub fn apply_pending(
        &mut self,
        pad: Option<&SharedCoolingPad>,
        chroma_available: bool,
        reset_auto_on_fan_apply: &mut bool,
        now: Instant,
    ) -> (Vec<(ApplyDomain, ApplyOutcome)>, bool) {
        if !self.should_run_apply(now) {
            return (Vec::new(), false);
        }
        let Some(pad) = pad else {
            return (Vec::new(), false);
        };

        let domains: Vec<ApplyDomain> = self.pending_domains.iter().copied().collect();
        let mut outcomes = Vec::new();
        let mut any_failed = false;

        for domain in domains {
            let outcome = match domain {
                ApplyDomain::Fan => {
                    if *reset_auto_on_fan_apply {
                        *reset_auto_on_fan_apply = false;
                    }
                    self.apply_fan(pad)
                }
                ApplyDomain::Lighting => self.apply_lighting(pad, chroma_available),
            };
            outcomes.push((domain, outcome));
            match outcome {
                ApplyOutcome::Ok | ApplyOutcome::Skipped => {
                    self.clear_domain(domain);
                }
                ApplyOutcome::Busy | ApplyOutcome::Failed => {
                    any_failed = true;
                    self.note_apply_failure(domain);
                }
            }
        }
        (outcomes, any_failed)
    }

    fn apply_fan(&self, pad: &SharedCoolingPad) -> ApplyOutcome {
        let mode = self.fan_mode();
        match mode {
            CoolingPadFanMode::Auto => ApplyOutcome::Skipped,
            CoolingPadFanMode::Off => map_pad_result(pad.with(|p| p.fan_off())),
            CoolingPadFanMode::Manual => {
                let rpm = self.runtime.manual_rpm;
                map_pad_result(pad.with(|p| p.set_fan_rpm(rpm)))
            }
        }
    }

    fn apply_lighting(&self, pad: &SharedCoolingPad, chroma_available: bool) -> ApplyOutcome {
        if !chroma_available {
            return ApplyOutcome::Skipped;
        }
        let Some(pad_mode) = PadLightingMode::from_str(&self.runtime.lighting_mode) else {
            return ApplyOutcome::Failed;
        };
        let rgb =
            Rgb { r: self.runtime.color[0], g: self.runtime.color[1], b: self.runtime.color[2] };
        let step = self.runtime.brightness_step.min(BRIGHTNESS_LEVELS.len() - 1);
        let brightness = BRIGHTNESS_LEVELS[step];
        let result = pad
            .with(|p| cooling_pad_apply::apply_pad_config_lighting(p, pad_mode, rgb, brightness));
        map_pad_result(result)
    }
}

fn map_pad_result<T>(result: Option<Result<T>>) -> ApplyOutcome {
    match result {
        None => ApplyOutcome::Busy,
        Some(Ok(_)) => ApplyOutcome::Ok,
        Some(Err(_)) => ApplyOutcome::Failed,
    }
}

pub fn normalize_runtime(runtime: CoolingPadRuntime) -> CoolingPadRuntime {
    let mut runtime = runtime;
    runtime.fan_mode = match runtime.fan_mode.to_ascii_lowercase().as_str() {
        "manual" => "manual".to_string(),
        "auto" => "auto".to_string(),
        _ => "off".to_string(),
    };
    runtime.manual_rpm = runtime.manual_rpm.clamp(MIN_RPM, MAX_RPM);
    runtime.auto_min_rpm = runtime.auto_min_rpm.clamp(MIN_RPM, MAX_RPM);
    runtime.auto_max_rpm = runtime.auto_max_rpm.clamp(runtime.auto_min_rpm, MAX_RPM);
    if runtime.auto_full_above_c < runtime.auto_off_below_c + 5.0 {
        runtime.auto_full_above_c = runtime.auto_off_below_c + 5.0;
    }
    if PadLightingMode::from_str(&runtime.lighting_mode).is_none() {
        runtime.lighting_mode = "Static".to_string();
    }
    let max_step = BRIGHTNESS_LEVELS.len().saturating_sub(1);
    runtime.brightness_step = runtime.brightness_step.min(max_step);
    runtime
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_clamps_fan_mode_and_rpm() {
        let runtime = CoolingPadRuntime {
            fan_mode: "AUTO".to_string(),
            manual_rpm: 10,
            auto_min_rpm: 10,
            auto_max_rpm: 10_000,
            auto_off_below_c: 60.0,
            auto_full_above_c: 50.0,
            auto_turn_on_delay_secs: 5.0,
            auto_turn_off_delay_secs: 5.0,
            auto_overcool_hold_secs: 5.0,
            auto_temp_ema_alpha: 0.2,
            auto_rpm_slew_up_per_sec: 500,
            auto_rpm_slew_down_per_sec: 500,
            auto_follow_temp_margin_c: 2.0,
            auto_temp_hysteresis_c: 1.0,
            follow_laptop_fan: true,
            lighting_mode: "Static".to_string(),
            color: [0, 255, 0],
            brightness_step: 999,
        };
        let normalized = normalize_runtime(runtime);
        assert_eq!(normalized.fan_mode, "auto");
        assert_eq!(normalized.manual_rpm, MIN_RPM);
        assert_eq!(normalized.auto_max_rpm, MAX_RPM);
        assert!(normalized.auto_full_above_c >= normalized.auto_off_below_c + 5.0);
        assert!(normalized.brightness_step < BRIGHTNESS_LEVELS.len());
    }

    #[test]
    fn bounded_retry_stops_after_max_attempts() {
        let mut ctrl = CoolingPadController::from_runtime(CoolingPadRuntime {
            fan_mode: "manual".into(),
            manual_rpm: 1500,
            auto_min_rpm: 500,
            auto_max_rpm: 3200,
            auto_off_below_c: 60.0,
            auto_full_above_c: 86.0,
            auto_turn_on_delay_secs: 5.0,
            auto_turn_off_delay_secs: 5.0,
            auto_overcool_hold_secs: 5.0,
            auto_temp_ema_alpha: 0.2,
            auto_rpm_slew_up_per_sec: 500,
            auto_rpm_slew_down_per_sec: 500,
            auto_follow_temp_margin_c: 2.0,
            auto_temp_hysteresis_c: 1.0,
            follow_laptop_fan: true,
            lighting_mode: "Static".into(),
            color: [0, 255, 0],
            brightness_step: 8,
        });
        ctrl.request_apply_domain(ApplyDomain::Fan, ApplyReason::Startup);
        for _ in 0..MAX_APPLY_ATTEMPTS {
            ctrl.note_apply_failure(ApplyDomain::Fan);
        }
        assert!(!ctrl.has_pending_apply());
    }
}
