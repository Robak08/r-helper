use super::pids::{headset_profile, HeadsetProfile};
use super::protocol::{arm_device, probe_battery_once, query_battery, send_rf_wake};
use super::transport::drain_input;

use crate::chroma::PeripheralBattery;
use crate::enumerate::RazerHidEntry;

use anyhow::{Context, Result};
use std::{
    collections::HashMap,
    ffi::CString,
    time::{Duration, Instant},
};

const RF_WAKE_INTERVAL: Duration = Duration::from_millis(3500);
const BATTERY_POLL_INTERVAL: Duration = Duration::from_secs(8);

pub fn hid_debug(message: impl AsRef<str>) {
    if std::env::var_os("R_HELPER_HID_DEBUG").is_some() {
        eprintln!("[hid] {}", message.as_ref());
    }
}

/// Open a headset HID session on the given interface.
pub struct HeadsetSession {
    device: hidapi::HidDevice,
    profile: HeadsetProfile,
    path: CString,
    last_rf_wake: Instant,
    last_battery_poll: Instant,
    armed: bool,
    cached: Option<PeripheralBattery>,
}

impl HeadsetSession {
    pub fn open(entry: &RazerHidEntry) -> Result<Self> {
        let profile = headset_profile(entry.pid)
            .ok_or_else(|| anyhow::anyhow!("PID 0x{:04x} is not a known headset", entry.pid))?;

        let api = hidapi::HidApi::new().context("Failed to create hid api")?;
        let device = api
            .open_path(entry.path.as_c_str())
            .with_context(|| format!("Failed to open headset {:?}", entry.path))?;

        let _ = device.set_blocking_mode(true);

        let mut session = Self {
            device,
            profile,
            path: entry.path.clone(),
            last_rf_wake: Instant::now(),
            last_battery_poll: Instant::now()
                .checked_sub(BATTERY_POLL_INTERVAL)
                .unwrap_or_else(Instant::now),
            armed: false,
            cached: None,
        };

        session.bootstrap()?;
        Ok(session)
    }

    fn bootstrap(&mut self) -> Result<()> {
        drain_input(&self.device);
        if let Some(battery) = probe_battery_once(&self.device, self.profile)? {
            self.cached = Some(battery);
            self.armed = true;
            self.last_battery_poll = Instant::now();
            return Ok(());
        }

        arm_device(&self.device, self.profile)?;
        self.armed = true;
        self.poll_battery()?;
        Ok(())
    }

    pub fn tick(&mut self) {
        let now = Instant::now();

        if self.profile == HeadsetProfile::WirelessDongle
            && now.duration_since(self.last_rf_wake) >= RF_WAKE_INTERVAL
        {
            if send_rf_wake(&self.device).is_ok() {
                self.last_rf_wake = now;
            }
        }

        if now.duration_since(self.last_battery_poll) >= BATTERY_POLL_INTERVAL {
            let _ = self.poll_battery();
        }
    }

    fn poll_battery(&mut self) -> Result<()> {
        if !self.armed {
            arm_device(&self.device, self.profile)?;
            self.armed = true;
        }

        if let Some(battery) = query_battery(&self.device, self.profile)? {
            self.cached = Some(battery);
        }
        self.last_battery_poll = Instant::now();
        Ok(())
    }

    pub fn battery(&self) -> Option<PeripheralBattery> {
        self.cached
    }

    pub fn path(&self) -> &CString {
        &self.path
    }
}

/// Best-effort one-shot headset battery read (opens and closes the handle).
pub fn probe_headset_battery(entry: &RazerHidEntry) -> Option<PeripheralBattery> {
    HeadsetSession::open(entry)
        .ok()
        .and_then(|session| session.battery())
}

/// Maintains persistent headset sessions for keepalive and periodic battery polling.
pub struct HeadsetBatteryManager {
    sessions: HashMap<u16, HeadsetSession>,
}

impl HeadsetBatteryManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Reconcile connected headsets and advance active sessions.
    pub fn tick(&mut self, entries: &[RazerHidEntry]) {
        let headset_entries = select_headset_entries(entries);

        let connected_pids: std::collections::HashSet<u16> =
            headset_entries.iter().map(|(pid, _)| *pid).collect();

        self.sessions
            .retain(|pid, _| connected_pids.contains(pid));

        for (pid, entry) in &headset_entries {
            if self.sessions.contains_key(pid) {
                continue;
            }

            match HeadsetSession::open(entry) {
                Ok(session) => {
                    hid_debug(format!(
                        "Headset session opened PID 0x{pid:04x} on {} (usage_page 0x{:04x})",
                        entry.path.to_string_lossy(),
                        entry.usage_page
                    ));
                    self.sessions.insert(*pid, session);
                }
                Err(err) => {
                    hid_debug(format!(
                        "Headset session open failed PID 0x{pid:04x} on {}: {err}",
                        entry.path.to_string_lossy()
                    ));
                }
            }
        }

        for session in self.sessions.values_mut() {
            session.tick();
        }
    }

    pub fn battery_for_pid(&self, pid: u16) -> Option<PeripheralBattery> {
        self.sessions.get(&pid).and_then(|s| s.battery())
    }
}

impl Default for HeadsetBatteryManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Pick the best HID control interface per headset PID.
pub fn select_headset_entries(entries: &[RazerHidEntry]) -> Vec<(u16, RazerHidEntry)> {
    let mut by_pid: HashMap<u16, RazerHidEntry> = HashMap::new();

    for entry in entries {
        if headset_profile(entry.pid).is_none() {
            continue;
        }
        if entry.usage_page == 0x0C {
            continue;
        }

        match by_pid.get(&entry.pid) {
            None => {
                by_pid.insert(entry.pid, entry.clone());
            }
            Some(existing) if interface_priority(entry) < interface_priority(existing) => {
                by_pid.insert(entry.pid, entry.clone());
            }
            _ => {}
        }
    }

    by_pid.into_iter().collect()
}

/// Ranked list of HID interfaces to try for battery on a given PID.
pub fn ranked_entries_for_pid<'a>(
    entries: &'a [RazerHidEntry],
    pid: u16,
) -> Vec<&'a RazerHidEntry> {
    let mut candidates: Vec<&RazerHidEntry> = entries
        .iter()
        .filter(|e| e.pid == pid && e.usage_page != 0x0C)
        .collect();

    candidates.sort_by_key(|e| interface_priority(e));
    candidates
}

fn interface_priority(entry: &RazerHidEntry) -> u8 {
    match entry.usage_page {
        0xFF14 => 0,
        0xFF13 => 1,
        page if page >= 0xFF00 => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn entry(pid: u16, usage_page: u16, path: &str) -> RazerHidEntry {
        RazerHidEntry {
            vid: 0x1532,
            pid,
            product_string: Some("Test".into()),
            manufacturer_string: None,
            path: CString::new(path).unwrap(),
            interface_number: 0,
            usage_page,
            usage: 0,
        }
    }

    #[test]
    fn prefers_ff14_interface() {
        let entries = vec![
            entry(0x057a, 0x0C, "/audio"),
            entry(0x057a, 0xFF13, "/vendor13"),
            entry(0x057a, 0xFF14, "/vendor14"),
        ];

        let selected = select_headset_entries(&entries);
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].1.usage_page, 0xFF14);
    }

    #[test]
    fn ranked_entries_skips_consumer_audio() {
        let entries = vec![
            entry(0x057a, 0x0C, "/audio"),
            entry(0x057a, 0xFF14, "/vendor"),
        ];

        let ranked = ranked_entries_for_pid(&entries, 0x057a);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].usage_page, 0xFF14);
    }

    /// Connect a BlackShark V3 via USB, then run:
    /// `cargo test -p librazer live_probe_connected_headsets -- --ignored --nocapture`
    #[test]
    #[ignore = "requires a supported Razer headset connected via USB"]
    fn live_probe_connected_headsets() {
        use crate::enumerate::{enrich_peripheral_batteries, list_razer_hid_devices, summarize_peripheral_devices};

        let entries = list_razer_hid_devices().expect("hid enumeration");
        let mut summaries = summarize_peripheral_devices(&entries);
        let mut manager = HeadsetBatteryManager::new();
        enrich_peripheral_batteries(&entries, &mut summaries, &mut manager);

        for device in &summaries {
            if device.kind != crate::enumerate::RazerDeviceKind::Headset {
                continue;
            }
            println!(
                "{} (PID 0x{:04x}): battery={:?} charging={:?} available={}",
                device.name, device.pid, device.battery_percent, device.battery_charging, device.battery_available
            );
            assert!(
                device.battery_available,
                "expected battery data for connected headset PID 0x{:04x}",
                device.pid
            );
        }
    }
}
