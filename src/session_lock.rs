use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use librazer::{command, device::Device, types::FanMode};

use crate::cooling_pad_enforce::CoolingPadEnforceShared;
use crate::device_handle::SharedDevice;
use crate::laptop_fan_cap::LaptopFanCapShared;
use crate::ui::cooling_pad_fan::CoolingPadFanMode;

const MONITOR_INTERVAL: Duration = Duration::from_secs(2);

pub struct SessionState {
    pub locked: AtomicBool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            locked: AtomicBool::new(false),
        }
    }
}

#[derive(Debug, Default)]
struct LaptopCapSnapshot {
    cap_active: bool,
    limit_enabled: bool,
    max_rpm: u16,
}

#[derive(Debug, Default)]
struct SessionSnapshots {
    laptop_manual_rpm: Option<u16>,
    laptop_cap: Option<LaptopCapSnapshot>,
    cooling_pad_manual_rpm: Option<u16>,
}

/// Lazily populated when the Razer laptop connects.
pub type DeviceSlot = Arc<Mutex<Option<SharedDevice>>>;

pub fn new_device_slot() -> DeviceSlot {
    Arc::new(Mutex::new(None))
}

#[cfg(windows)]
pub fn is_session_locked() -> bool {
    use windows::Win32::Foundation::{GetLastError, HANDLE};
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, DESKTOP_CONTROL_FLAGS,
        DESKTOP_READOBJECTS, UOI_NAME,
    };

    unsafe {
        let desktop = match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS)
        {
            Ok(d) => d,
            Err(_) => return false,
        };

        let mut name_buf = [0u16; 256];
        let mut needed = 0u32;
        let ok = GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name_buf.as_mut_ptr().cast()),
            (name_buf.len() * 2) as u32,
            Some(&mut needed),
        )
        .is_ok();
        let _ = CloseDesktop(desktop);

        if !ok {
            let _ = GetLastError();
            return false;
        }

        let len = needed as usize / 2;
        let name = String::from_utf16_lossy(&name_buf[..len.min(name_buf.len())]);
        name.trim_end_matches('\0') == "Winlogon"
    }
}

#[cfg(not(windows))]
pub fn is_session_locked() -> bool {
    false
}

pub fn spawn_session_lock_monitor(
    session: Arc<SessionState>,
    ctx: eframe::egui::Context,
    device_slot: DeviceSlot,
    laptop_fan_cap: Arc<Mutex<LaptopFanCapShared>>,
    cooling_pad_settings: Arc<Mutex<CoolingPadEnforceShared>>,
    pending_cooling_pad_restore: Arc<AtomicBool>,
) {
    std::thread::Builder::new()
        .name("session-lock-monitor".into())
        .spawn(move || {
            let mut snapshots = SessionSnapshots::default();
            let mut was_locked = is_session_locked();
            session.locked.store(was_locked, Ordering::Relaxed);

            loop {
                std::thread::sleep(MONITOR_INTERVAL);

                let locked = is_session_locked();
                if locked == was_locked {
                    continue;
                }

                was_locked = locked;
                session.locked.store(locked, Ordering::Relaxed);

                let device = device_slot
                    .lock()
                    .ok()
                    .and_then(|guard| guard.clone());

                if locked {
                    if let Some(ref dev) = device {
                        apply_lock_mitigations(
                            dev,
                            &laptop_fan_cap,
                            &cooling_pad_settings,
                            &mut snapshots,
                        );
                    }
                } else {
                    if let Some(ref dev) = device {
                        restore_unlock_state(
                            dev,
                            &laptop_fan_cap,
                            &cooling_pad_settings,
                            &pending_cooling_pad_restore,
                            &mut snapshots,
                        );
                    } else {
                        snapshots = SessionSnapshots::default();
                    }
                    ctx.request_repaint();
                }
            }
        })
        .expect("session lock monitor thread");
}

fn read_laptop_fan_mode(device: &Device) -> Option<(FanMode, Option<u16>)> {
    use librazer::types::FanZone;

    let (_, fan_mode) = command::get_perf_mode(device).ok()?;
    let rpm = if fan_mode == FanMode::Manual {
        command::get_fan_rpm(device, FanZone::Zone1).ok()
    } else {
        None
    };
    Some((fan_mode, rpm))
}

fn apply_lock_mitigations(
    device: &SharedDevice,
    laptop_fan_cap: &Arc<Mutex<LaptopFanCapShared>>,
    cooling_pad_settings: &Arc<Mutex<CoolingPadEnforceShared>>,
    snapshots: &mut SessionSnapshots,
) {
    if let Ok(mut cap) = laptop_fan_cap.lock() {
        if cap.cap_active {
            snapshots.laptop_cap = Some(LaptopCapSnapshot {
                cap_active: cap.cap_active,
                limit_enabled: cap.limit_enabled,
                max_rpm: cap.max_rpm,
            });
            cap.skip = true;
            cap.cap_active = false;
            if let Some(d) = device.with_mut(|d| command::set_fan_mode(d, FanMode::Auto)) {
                if let Err(e) = d {
                    eprintln!("session lock: failed to release laptop fan cap to Auto: {e}");
                }
            }
        }
    }

    if snapshots.laptop_cap.is_none() {
        if let Some((fan_mode, rpm)) = device.with(read_laptop_fan_mode).flatten() {
            if fan_mode == FanMode::Manual {
                if let Some(rpm) = rpm {
                    snapshots.laptop_manual_rpm = Some(rpm);
                }
                if let Some(result) =
                    device.with_mut(|d| command::set_fan_mode(d, FanMode::Auto))
                {
                    if let Err(e) = result {
                        eprintln!("session lock: failed to set laptop fan Auto: {e}");
                    }
                }
            }
        }
    }

    if let Ok(mut settings) = cooling_pad_settings.lock() {
        if settings.active
            && settings.fully_initialized
            && settings.fan_mode == CoolingPadFanMode::Manual
        {
            snapshots.cooling_pad_manual_rpm = Some(settings.manual_rpm);
            settings.fan_mode = CoolingPadFanMode::Auto;
        }
    }
}

fn restore_unlock_state(
    device: &SharedDevice,
    laptop_fan_cap: &Arc<Mutex<LaptopFanCapShared>>,
    cooling_pad_settings: &Arc<Mutex<CoolingPadEnforceShared>>,
    pending_cooling_pad_restore: &Arc<AtomicBool>,
    snapshots: &mut SessionSnapshots,
) {
    if let Some(rpm) = snapshots.cooling_pad_manual_rpm.take() {
        if let Ok(mut settings) = cooling_pad_settings.lock() {
            settings.fan_mode = CoolingPadFanMode::Manual;
            settings.manual_rpm = rpm;
        }
        pending_cooling_pad_restore.store(true, Ordering::Relaxed);
    }

    if let Some(rpm) = snapshots.laptop_manual_rpm.take() {
        if let Some(result) = device.with_mut(|d| {
            command::set_fan_mode(d, FanMode::Manual)?;
            command::set_fan_rpm(d, rpm, true)
        }) {
            if let Err(e) = result {
                eprintln!("session lock: failed to restore laptop manual fan: {e}");
            }
        }
    }

    if let Some(cap_snap) = snapshots.laptop_cap.take() {
        if let Ok(mut cap) = laptop_fan_cap.lock() {
            cap.skip = false;
            cap.limit_enabled = cap_snap.limit_enabled;
            cap.max_rpm = cap_snap.max_rpm;
            cap.cap_active = cap_snap.cap_active;
        }
    }
}
