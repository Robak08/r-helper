use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use crate::cooling_pad_auto::{
    compute_combined_auto, CoolingPadAutoInputs, CoolingPadAutoOutput, CoolingPadAutoState,
};
use crate::cooling_pad_handle::SharedCoolingPad;
use crate::ui::cooling_pad_fan::CoolingPadFanMode;

const ENFORCE_INTERVAL: Duration = Duration::from_secs(5);

/// Settings and auto state shared between the UI thread and the background enforcer.
#[derive(Debug)]
pub struct CoolingPadEnforceShared {
    pub active: bool,
    pub fully_initialized: bool,
    pub fan_mode: CoolingPadFanMode,
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
    pub laptop_fan_follow_enabled: bool,
    pub auto_state: CoolingPadAutoState,
    pub last_enforce_time: Instant,
}

impl Default for CoolingPadEnforceShared {
    fn default() -> Self {
        Self {
            active: false,
            fully_initialized: false,
            fan_mode: CoolingPadFanMode::Off,
            manual_rpm: 500,
            auto_min_rpm: 500,
            auto_max_rpm: 3200,
            auto_off_below_c: 60.0,
            auto_full_above_c: 86.0,
            auto_turn_on_delay_secs: crate::cooling_pad_auto::DEFAULT_TURN_ON_DELAY_SECS,
            auto_turn_off_delay_secs: crate::cooling_pad_auto::DEFAULT_TURN_OFF_DELAY_SECS,
            auto_overcool_hold_secs: crate::cooling_pad_auto::DEFAULT_OVERCOOL_HOLD_SECS,
            auto_temp_ema_alpha: crate::cooling_pad_auto::DEFAULT_TEMP_EMA_ALPHA,
            auto_rpm_slew_up_per_sec: crate::cooling_pad_auto::DEFAULT_RPM_SLEW_UP_PER_SEC,
            auto_rpm_slew_down_per_sec: crate::cooling_pad_auto::DEFAULT_RPM_SLEW_DOWN_PER_SEC,
            auto_follow_temp_margin_c: crate::cooling_pad_auto::DEFAULT_FOLLOW_TEMP_MARGIN_C,
            auto_temp_hysteresis_c: crate::cooling_pad_auto::DEFAULT_TEMP_HYSTERESIS_C,
            laptop_fan_follow_enabled: true,
            auto_state: CoolingPadAutoState::default(),
            last_enforce_time: Instant::now(),
        }
    }
}

pub struct CoolingPadEnforceContext {
    pub settings: Arc<Mutex<CoolingPadEnforceShared>>,
    pub laptop_fan_rpm: Arc<Mutex<Option<u16>>>,
    pub pad: Arc<Mutex<Option<SharedCoolingPad>>>,
    pub pending_cooling_pad_restore: Arc<AtomicBool>,
    pub needs_redetect: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

impl CoolingPadEnforceContext {
    pub fn start(shared_thermal: Arc<Mutex<crate::system::thermal::ThermalSnapshot>>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(CoolingPadEnforceShared::default()));
        let laptop_fan_rpm = Arc::new(Mutex::new(None));
        let pad = Arc::new(Mutex::new(None));
        let pending_cooling_pad_restore = Arc::new(AtomicBool::new(false));
        let needs_redetect = Arc::new(AtomicBool::new(false));

        spawn_cooling_pad_enforcer(
            Arc::clone(&settings),
            Arc::clone(&laptop_fan_rpm),
            Arc::clone(&pad),
            Arc::clone(&needs_redetect),
            Arc::clone(&shared_thermal),
            Arc::clone(&running),
        );

        Self {
            settings,
            laptop_fan_rpm,
            pad,
            pending_cooling_pad_restore,
            needs_redetect,
            running,
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

fn spawn_cooling_pad_enforcer(
    settings: Arc<Mutex<CoolingPadEnforceShared>>,
    laptop_fan_rpm: Arc<Mutex<Option<u16>>>,
    pad_slot: Arc<Mutex<Option<SharedCoolingPad>>>,
    needs_redetect: Arc<AtomicBool>,
    shared_thermal: Arc<Mutex<crate::system::thermal::ThermalSnapshot>>,
    running: Arc<AtomicBool>,
) {
    std::thread::Builder::new()
        .name("cooling-pad-enforce".into())
        .spawn(move || {
            let mut last_tick = Instant::now();

            while running.load(Ordering::Relaxed) {
                std::thread::sleep(ENFORCE_INTERVAL);
                if !running.load(Ordering::Relaxed) {
                    break;
                }

                let pad = pad_slot
                    .lock()
                    .ok()
                    .and_then(|guard| guard.clone());
                let Some(pad) = pad else {
                    continue;
                };

                let mut settings_guard = match settings.lock() {
                    Ok(guard) => guard,
                    Err(_) => continue,
                };

                if !settings_guard.active
                    || !settings_guard.fully_initialized
                    || settings_guard.fan_mode == CoolingPadFanMode::Off
                {
                    continue;
                }

                let dt = last_tick.elapsed().as_secs_f32().clamp(0.05, 10.0);
                last_tick = Instant::now();

                let thermal = crate::thermal_poll::read_shared_thermal(&shared_thermal);

                let laptop_rpm = laptop_fan_rpm.lock().ok().and_then(|g| *g);

                match settings_guard.fan_mode {
                    CoolingPadFanMode::Manual => {
                        let rpm = settings_guard.manual_rpm;
                        drop(settings_guard);
                        let failed = pad
                            .with(|p| p.set_fan_rpm(rpm))
                            .is_none_or(|r| r.is_err());
                        if failed {
                            needs_redetect.store(true, Ordering::Relaxed);
                        }
                    }
                    CoolingPadFanMode::Auto => {
                        let inputs = CoolingPadAutoInputs {
                            cpu_temp_c: thermal.cpu_avg_c,
                            gpu_temp_c: thermal.gpu_avg_c,
                            laptop_fan_actual_rpm: if settings_guard.laptop_fan_follow_enabled {
                                laptop_rpm
                            } else {
                                None
                            },
                            min_rpm: settings_guard.auto_min_rpm,
                            max_rpm: settings_guard.auto_max_rpm,
                            off_below_c: settings_guard.auto_off_below_c,
                            full_above_c: settings_guard.auto_full_above_c,
                            temp_hysteresis_c: settings_guard.auto_temp_hysteresis_c,
                            dt_secs: dt,
                            turn_on_delay_secs: settings_guard.auto_turn_on_delay_secs,
                            turn_off_delay_secs: settings_guard.auto_turn_off_delay_secs,
                            overcool_hold_secs: settings_guard.auto_overcool_hold_secs,
                            temp_ema_alpha: settings_guard.auto_temp_ema_alpha,
                            rpm_slew_up_per_sec: settings_guard.auto_rpm_slew_up_per_sec,
                            rpm_slew_down_per_sec: settings_guard.auto_rpm_slew_down_per_sec,
                            follow_temp_margin_c: settings_guard.auto_follow_temp_margin_c,
                            laptop_fan_follow_enabled: settings_guard.laptop_fan_follow_enabled,
                        };
                        let output =
                            compute_combined_auto(&inputs, &mut settings_guard.auto_state);
                        settings_guard.last_enforce_time = Instant::now();
                        drop(settings_guard);

                        let failed = match output {
                            CoolingPadAutoOutput::Off => pad
                                .with(|p| p.fan_off())
                                .is_none_or(|r| r.is_err()),
                            CoolingPadAutoOutput::Rpm(rpm) => pad
                                .with(|p| p.set_fan_rpm(rpm))
                                .is_none_or(|r| r.is_err()),
                        };
                        if failed {
                            needs_redetect.store(true, Ordering::Relaxed);
                        }
                    }
                    CoolingPadFanMode::Off => {}
                }
            }
        })
        .expect("cooling pad enforcer thread");
}
