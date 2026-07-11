use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{SyncSender, TrySendError},
};
use std::time::{Duration, Instant};

use librazer::{
    command,
    device::Device,
    types::{BatteryCare, FanMode, FanZone, LightsAlwaysOn},
};

use librazer::cooling_pad::CoolingPadDevice;

use crate::device::CompleteDeviceState;
use crate::laptop_fan_cap::{
    CapEnforcerState, LaptopFanCapShared, LaptopFanState, enforce_laptop_fan_cap_from_sample,
};
use crate::power::get_power_state;
use crate::system::thermal::ThermalSnapshot;
use crate::worker::StopSignal;

const FAST_POLL_INTERVAL: Duration = Duration::from_millis(1000);
const FAN_CAP_POLL_INTERVAL: Duration = Duration::from_millis(800);
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

#[derive(Debug, Clone)]
pub struct CoolingPadPollSnapshot {
    pub brightness: Option<u8>,
    pub responsive: bool,
}

pub fn spawn_cooling_pad_poller(
    device: Arc<Mutex<CoolingPadDevice>>,
    tx: SyncSender<CoolingPadPollSnapshot>,
    brightness_slider_active: Arc<AtomicBool>,
    poll_slow: Arc<AtomicBool>,
    stop: Arc<StopSignal>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("cooling-pad-poll".into())
        .spawn(move || {
            while !stop.is_stopped() {
                let interval = if poll_slow.load(Ordering::Relaxed) {
                    HIDDEN_POLL_INTERVAL
                } else {
                    FAST_POLL_INTERVAL
                };
                if stop.wait(interval) {
                    break;
                }

                let snapshot = {
                    let device = match device.try_lock() {
                        Ok(guard) => guard,
                        Err(_) => continue,
                    };
                    read_cooling_pad_snapshot(
                        &device,
                        brightness_slider_active.load(Ordering::Relaxed),
                    )
                };

                if let Err(TrySendError::Disconnected(_)) = tx.try_send(snapshot) {
                    break;
                }
            }
        })
        .expect("cooling pad poller thread")
}

fn read_cooling_pad_snapshot(
    device: &CoolingPadDevice,
    skip_brightness: bool,
) -> CoolingPadPollSnapshot {
    let brightness =
        if skip_brightness || !device.chroma_available() { None } else { device.brightness().ok() };

    CoolingPadPollSnapshot { brightness, responsive: device.is_responsive() }
}

pub fn spawn_device_poller(
    device: Arc<Mutex<Device>>,
    tx: SyncSender<DevicePollSnapshot>,
    brightness_slider_active: Arc<AtomicBool>,
    poll_slow: Arc<AtomicBool>,
    laptop_fan_rpm: Arc<Mutex<Option<u16>>>,
    laptop_fan_cap: Arc<Mutex<LaptopFanCapShared>>,
    shared_thermal: Arc<Mutex<ThermalSnapshot>>,
    stop: Arc<StopSignal>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("device-poll".into())
        .spawn(move || {
            let mut last_slow =
                Instant::now().checked_sub(SLOW_POLL_INTERVAL).unwrap_or_else(Instant::now);
            let mut cap_state = CapEnforcerState::default();
            let mut last_cap_tick = Instant::now();

            while !stop.is_stopped() {
                let cap_needs_fast_poll = laptop_fan_cap
                    .lock()
                    .map(|cap| cap.limit_enabled && !cap.skip)
                    .unwrap_or(false);
                let interval = if cap_needs_fast_poll {
                    FAN_CAP_POLL_INTERVAL
                } else if poll_slow.load(Ordering::Relaxed) {
                    HIDDEN_POLL_INTERVAL
                } else {
                    FAST_POLL_INTERVAL
                };
                if stop.wait(interval) {
                    break;
                }

                let include_full = last_slow.elapsed() >= SLOW_POLL_INTERVAL;
                let skip_brightness = brightness_slider_active.load(Ordering::Relaxed);

                let snapshot = {
                    let device = match device.try_lock() {
                        Ok(guard) => guard,
                        Err(_) => continue,
                    };
                    let Some((fan_actual_rpm, fan_mode, fan_set_rpm)) = read_fan_state(&device)
                    else {
                        continue;
                    };

                    if let Ok(mut cap) = laptop_fan_cap.lock() {
                        let thermal = shared_thermal.lock().map(|value| *value).unwrap_or_default();
                        let dt_secs = last_cap_tick.elapsed().as_secs_f32().clamp(0.05, 3.0);
                        last_cap_tick = Instant::now();
                        enforce_laptop_fan_cap_from_sample(
                            &device,
                            &mut cap,
                            LaptopFanState::new(fan_mode, fan_set_rpm, fan_actual_rpm),
                            &thermal,
                            &mut cap_state,
                            dt_secs,
                        );
                    }

                    match read_snapshot(
                        &device,
                        skip_brightness,
                        include_full,
                        fan_actual_rpm,
                        fan_mode,
                        fan_set_rpm,
                    ) {
                        Some(s) => s,
                        None => continue,
                    }
                };

                if snapshot.full_state.is_some() {
                    last_slow = Instant::now();
                }

                if let Ok(mut shared) = laptop_fan_rpm.lock() {
                    *shared = snapshot.fan_actual_rpm;
                }

                if let Err(TrySendError::Disconnected(_)) = tx.try_send(snapshot) {
                    break;
                }
            }
        })
        .expect("device poller thread")
}

fn read_fan_state(device: &Device) -> Option<(Option<u16>, FanMode, Option<u16>)> {
    let fan_actual_rpm = command::get_fan_actual_rpm(device, FanZone::Zone1).ok();
    let (_, fan_mode) = command::get_perf_mode(device).ok()?;
    let fan_set_rpm = if fan_mode == FanMode::Manual {
        command::get_fan_rpm(device, FanZone::Zone1).ok()
    } else {
        None
    };
    Some((fan_actual_rpm, fan_mode, fan_set_rpm))
}

fn read_snapshot(
    device: &Device,
    skip_brightness: bool,
    include_full_state: bool,
    fan_actual_rpm: Option<u16>,
    fan_mode: FanMode,
    fan_set_rpm: Option<u16>,
) -> Option<DevicePollSnapshot> {
    let ac_power = get_power_state().unwrap_or(true);

    let keyboard_brightness =
        if skip_brightness { None } else { command::get_keyboard_brightness(device).ok() };

    let lights_always_on = command::get_lights_always_on(device)
        .map(|v| matches!(v, LightsAlwaysOn::Enable))
        .unwrap_or(false);
    let battery_care = command::get_battery_care(device).ok()?;

    let full_state =
        if include_full_state { CompleteDeviceState::read_from_device(device).ok() } else { None };

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
