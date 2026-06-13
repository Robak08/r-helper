use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use librazer::{
    command,
    device::Device,
    types::{BatteryCare, FanMode, FanZone, LightsAlwaysOn},
};

use librazer::cooling_pad::CoolingPadDevice;

use crate::device::CompleteDeviceState;
use crate::power::get_power_state;

const FAST_POLL_INTERVAL: Duration = Duration::from_millis(1000);
const SLOW_POLL_INTERVAL: Duration = Duration::from_secs(3);
const HIDDEN_POLL_INTERVAL: Duration = Duration::from_millis(2500);
const LAPTOP_FAN_CAP_HYSTERESIS: u16 = 300;
const LAPTOP_FAN_CAP_INTERVAL: Duration = Duration::from_millis(800);

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
struct LaptopFanState {
    fan_mode: FanMode,
    fan_set_rpm: Option<u16>,
    fan_actual_rpm: Option<u16>,
}

fn read_laptop_fan_state(device: &Device) -> Option<LaptopFanState> {
    let fan_actual_rpm = command::get_fan_actual_rpm(device, FanZone::Zone1).ok();
    let (fan_mode, fan_set_rpm) = match command::get_perf_mode(device) {
        Ok((_, fm)) => {
            let rpm = if fm == FanMode::Manual {
                command::get_fan_rpm(device, FanZone::Zone1).ok()
            } else {
                None
            };
            (fm, rpm)
        }
        Err(_) => return None,
    };
    Some(LaptopFanState {
        fan_mode,
        fan_set_rpm,
        fan_actual_rpm,
    })
}

/// Apply or release the auto-max cap. Returns `true` when `cap_active` changed.
fn enforce_laptop_fan_cap(device: &Device, cap: &mut LaptopFanCapShared, fan: LaptopFanState) {
    if cap.skip {
        return;
    }

    if !cap.limit_enabled {
        if cap.cap_active {
            let _ = command::set_fan_mode(device, FanMode::Auto);
            cap.cap_active = false;
        }
        return;
    }

    let max = cap.max_rpm;
    let Some(actual) = fan.fan_actual_rpm else {
        if cap.cap_active {
            let _ = command::set_fan_rpm(device, max, true);
        }
        return;
    };

    if cap.cap_active {
        if fan.fan_mode == FanMode::Manual
            && fan.fan_set_rpm != Some(max)
            && !cap.limit_enabled
        {
            cap.cap_active = false;
            return;
        }

        if fan.fan_mode != FanMode::Manual {
            let _ = command::set_fan_mode(device, FanMode::Manual);
        }
        let _ = command::set_fan_rpm(device, max, true);

        if actual < max.saturating_sub(LAPTOP_FAN_CAP_HYSTERESIS) {
            if command::set_fan_mode(device, FanMode::Auto).is_ok() {
                cap.cap_active = false;
            }
        }
        return;
    }

    if fan.fan_mode != FanMode::Auto {
        return;
    }

    if actual > max
        && command::set_fan_mode(device, FanMode::Manual).is_ok()
        && command::set_fan_rpm(device, max, true).is_ok()
    {
        cap.cap_active = true;
    }
}

fn laptop_fan_rpm_for_pad(snapshot: &DevicePollSnapshot, cap: &LaptopFanCapShared) -> Option<u16> {
    let rpm = snapshot.fan_actual_rpm?;
    if cap.limit_enabled {
        Some(rpm.min(cap.max_rpm))
    } else {
        Some(rpm)
    }
}

pub fn spawn_laptop_fan_cap_enforcer(
    device: Arc<Mutex<Device>>,
    laptop_fan_cap: Arc<Mutex<LaptopFanCapShared>>,
) {
    std::thread::Builder::new()
        .name("laptop-fan-cap".into())
        .spawn(move || {
            loop {
                std::thread::sleep(LAPTOP_FAN_CAP_INTERVAL);

                let Ok(mut cap) = laptop_fan_cap.lock() else {
                    continue;
                };
                if cap.skip {
                    continue;
                }

                let Ok(device) = device.try_lock() else {
                    continue;
                };
                let Some(fan) = read_laptop_fan_state(&device) else {
                    continue;
                };
                enforce_laptop_fan_cap(&device, &mut cap, fan);
            }
        })
        .expect("laptop fan cap enforcer thread");
}

#[derive(Debug, Clone)]
pub struct DevicePollSnapshot {
    pub ac_power: bool,
    pub fan_actual_rpm: Option<u16>,
    pub fan_mode: FanMode,
    pub fan_set_rpm: Option<u16>,
    pub keyboard_brightness: Option<u8>,
    pub lights_always_on: bool,
    pub battery_care: BatteryCare,
    pub full_state: Option<CompleteDeviceState>,
}

#[derive(Debug, Clone)]
pub struct CoolingPadPollSnapshot {
    pub brightness: Option<u8>,
}

pub fn spawn_cooling_pad_poller(
    device: Arc<Mutex<CoolingPadDevice>>,
    tx: Sender<CoolingPadPollSnapshot>,
    brightness_slider_active: Arc<AtomicBool>,
    poll_slow: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        loop {
            if !running.load(Ordering::Relaxed) {
                break;
            }
            let interval = if poll_slow.load(Ordering::Relaxed) {
                HIDDEN_POLL_INTERVAL
            } else {
                FAST_POLL_INTERVAL
            };
            std::thread::sleep(interval);

            let snapshot = {
                let device = match device.try_lock() {
                    Ok(guard) => guard,
                    Err(_) => continue,
                };
                read_cooling_pad_snapshot(&device, brightness_slider_active.load(Ordering::Relaxed))
            };

            if tx.send(snapshot).is_err() {
                break;
            }
        }
    });
}

fn read_cooling_pad_snapshot(
    device: &CoolingPadDevice,
    skip_brightness: bool,
) -> CoolingPadPollSnapshot {
    let brightness = if skip_brightness || !device.chroma_available() {
        None
    } else {
        device.brightness().ok()
    };

    CoolingPadPollSnapshot { brightness }
}

pub fn spawn_device_poller(
    device: Arc<Mutex<Device>>,
    tx: Sender<DevicePollSnapshot>,
    brightness_slider_active: Arc<AtomicBool>,
    poll_slow: Arc<AtomicBool>,
    laptop_fan_rpm: Arc<Mutex<Option<u16>>>,
    laptop_fan_cap: Arc<Mutex<LaptopFanCapShared>>,
) {
    spawn_laptop_fan_cap_enforcer(Arc::clone(&device), Arc::clone(&laptop_fan_cap));

    std::thread::spawn(move || {
        let mut last_slow = Instant::now()
            .checked_sub(SLOW_POLL_INTERVAL)
            .unwrap_or_else(Instant::now);

        loop {
            let interval = if poll_slow.load(Ordering::Relaxed) {
                HIDDEN_POLL_INTERVAL
            } else {
                FAST_POLL_INTERVAL
            };
            std::thread::sleep(interval);

            let include_full = last_slow.elapsed() >= SLOW_POLL_INTERVAL;
            let skip_brightness = brightness_slider_active.load(Ordering::Relaxed);

            let snapshot = {
                let device = match device.try_lock() {
                    Ok(guard) => guard,
                    Err(_) => continue,
                };
                match read_snapshot(&device, skip_brightness, include_full) {
                    Some(s) => s,
                    None => continue,
                }
            };

            if snapshot.full_state.is_some() {
                last_slow = Instant::now();
            }

            if let Ok(mut shared) = laptop_fan_rpm.lock() {
                if let Ok(cap) = laptop_fan_cap.lock() {
                    *shared = laptop_fan_rpm_for_pad(&snapshot, &cap);
                } else {
                    *shared = snapshot.fan_actual_rpm;
                }
            }

            if tx.send(snapshot).is_err() {
                break;
            }
        }
    });
}

fn read_snapshot(
    device: &Device,
    skip_brightness: bool,
    include_full_state: bool,
) -> Option<DevicePollSnapshot> {
    let ac_power = get_power_state().unwrap_or(true);
    let fan_actual_rpm = command::get_fan_actual_rpm(device, FanZone::Zone1).ok();
    let (fan_mode, fan_set_rpm) = match command::get_perf_mode(device) {
        Ok((_, fm)) => {
            let rpm = if fm == FanMode::Manual {
                command::get_fan_rpm(device, FanZone::Zone1).ok()
            } else {
                None
            };
            (fm, rpm)
        }
        Err(_) => return None,
    };

    let keyboard_brightness = if skip_brightness {
        None
    } else {
        command::get_keyboard_brightness(device).ok()
    };

    let lights_always_on = command::get_lights_always_on(device)
        .map(|v| matches!(v, LightsAlwaysOn::Enable))
        .unwrap_or(false);
    let battery_care = command::get_battery_care(device).ok()?;

    let full_state = if include_full_state {
        CompleteDeviceState::read_from_device(device).ok()
    } else {
        None
    };

    Some(DevicePollSnapshot {
        ac_power,
        fan_actual_rpm,
        fan_mode,
        fan_set_rpm,
        keyboard_brightness,
        lights_always_on,
        battery_care,
        full_state,
    })
}
