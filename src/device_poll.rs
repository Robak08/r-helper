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

use crate::device::CompleteDeviceState;
use crate::power::get_power_state;

const FAST_POLL_INTERVAL: Duration = Duration::from_millis(500);
const SLOW_POLL_INTERVAL: Duration = Duration::from_secs(3);
const HIDDEN_POLL_INTERVAL: Duration = Duration::from_millis(2500);

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

pub fn spawn_device_poller(
    device: Arc<Mutex<Device>>,
    tx: Sender<DevicePollSnapshot>,
    brightness_slider_active: Arc<AtomicBool>,
    poll_slow: Arc<AtomicBool>,
) {
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
