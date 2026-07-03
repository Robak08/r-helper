use crate::chroma::probe_peripheral_battery;
use crate::cooling_pad::COOLING_PAD_PID;
use crate::headset::{
    device_protocol, hid_debug, is_modern_headset_pid, ranked_entries_for_pid, DeviceProtocol,
    HeadsetBatteryManager,
};
use crate::profile::lookup_profile;

use anyhow::{Context, Result};
use std::ffi::CString;

pub const RAZER_VID: u16 = 0x1532;

#[derive(Debug, Clone)]
pub struct RazerHidEntry {
    pub vid: u16,
    pub pid: u16,
    pub product_string: Option<String>,
    pub manufacturer_string: Option<String>,
    pub path: CString,
    pub interface_number: i32,
    pub usage_page: u16,
    pub usage: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RazerDeviceKind {
    BladeLaptop,
    CoolingPad,
    Headset,
    Peripheral,
}

fn device_kind(pid: u16) -> RazerDeviceKind {
    if lookup_profile(pid).is_some() {
        RazerDeviceKind::BladeLaptop
    } else if pid == COOLING_PAD_PID {
        RazerDeviceKind::CoolingPad
    } else if is_modern_headset_pid(pid) {
        RazerDeviceKind::Headset
    } else {
        RazerDeviceKind::Peripheral
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RazerDeviceSummary {
    pub pid: u16,
    pub name: String,
    pub kind: RazerDeviceKind,
    pub interface_count: usize,
    pub battery_percent: Option<u8>,
    pub battery_charging: Option<bool>,
    /// `true` when a battery query succeeded at least once.
    pub battery_available: bool,
}

/// List every HID interface exposed by Razer USB devices (VID 0x1532).
pub fn list_razer_hid_devices() -> Result<Vec<RazerHidEntry>> {
    let api = hidapi::HidApi::new().context("Failed to create hid api")?;

    let mut entries: Vec<RazerHidEntry> = api
        .device_list()
        .filter(|info| info.vendor_id() == RAZER_VID)
        .map(|info| RazerHidEntry {
            vid: info.vendor_id(),
            pid: info.product_id(),
            product_string: info.product_string().map(str::to_string),
            manufacturer_string: info.manufacturer_string().map(str::to_string),
            path: info.path().to_owned(),
            interface_number: info.interface_number(),
            usage_page: info.usage_page(),
            usage: info.usage(),
        })
        .collect();

    entries.sort_by(|a, b| {
        a.pid
            .cmp(&b.pid)
            .then(a.interface_number.cmp(&b.interface_number))
            .then(a.path.to_bytes().cmp(b.path.to_bytes()))
    });

    Ok(entries)
}

/// Collapse HID interfaces into one row per USB product ID.
pub fn summarize_razer_devices(entries: &[RazerHidEntry]) -> Vec<RazerDeviceSummary> {
    let mut summaries: Vec<RazerDeviceSummary> = Vec::new();

    for entry in entries {
        if let Some(existing) = summaries.iter_mut().find(|s| s.pid == entry.pid) {
            existing.interface_count += 1;
            if is_better_name(&entry.product_string, &existing.name) {
                existing.name = best_name(entry);
            }
            continue;
        }

        summaries.push(RazerDeviceSummary {
            pid: entry.pid,
            name: best_name(entry),
            kind: device_kind(entry.pid),
            interface_count: 1,
            battery_percent: None,
            battery_charging: None,
            battery_available: false,
        });
    }

    summaries
}

/// Like [`summarize_razer_devices`], but excludes Blade laptop and cooling pad interfaces.
pub fn summarize_peripheral_devices(entries: &[RazerHidEntry]) -> Vec<RazerDeviceSummary> {
    summarize_razer_devices(entries)
        .into_iter()
        .filter(|s| matches!(s.kind, RazerDeviceKind::Peripheral | RazerDeviceKind::Headset))
        .collect()
}

/// Query peripheral battery levels (Chroma mice/keyboards + modern headsets).
pub fn enrich_peripheral_batteries(
    entries: &[RazerHidEntry],
    summaries: &mut [RazerDeviceSummary],
    headset_manager: &mut HeadsetBatteryManager,
) {
    headset_manager.tick(entries);

    for summary in summaries.iter_mut() {
        if matches!(
            summary.kind,
            RazerDeviceKind::BladeLaptop | RazerDeviceKind::CoolingPad
        ) {
            continue;
        }

        let battery = match device_protocol(summary.pid) {
            DeviceProtocol::HeadsetModern => headset_manager.battery_for_pid(summary.pid),
            DeviceProtocol::Chroma => probe_chroma_battery(entries, summary.pid),
        };

        if let Some(battery) = battery {
            summary.battery_percent = Some(battery.percent);
            summary.battery_charging = Some(battery.charging);
            summary.battery_available = true;
        } else {
            log_failed_battery_probe(entries, summary.pid);
        }
    }
}

fn probe_chroma_battery(
    entries: &[RazerHidEntry],
    pid: u16,
) -> Option<crate::chroma::PeripheralBattery> {
    for entry in ranked_entries_for_pid(entries, pid) {
        if let Some(battery) = probe_peripheral_battery(entry) {
            return Some(battery);
        }
    }
    None
}

fn log_failed_battery_probe(entries: &[RazerHidEntry], pid: u16) {
    let interfaces: Vec<String> = entries
        .iter()
        .filter(|e| e.pid == pid)
        .map(|e| {
            format!(
                "if{} usage_page=0x{:04x} usage=0x{:04x} path={}",
                e.interface_number,
                e.usage_page,
                e.usage,
                e.path.to_string_lossy()
            )
        })
        .collect();

    if interfaces.is_empty() {
        return;
    }

    hid_debug(format!(
        "Battery probe failed for PID 0x{pid:04x} ({})",
        interfaces.join("; ")
    ));
}

fn best_name(entry: &RazerHidEntry) -> String {
    entry
        .product_string
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("Razer device 0x{:04x}", entry.pid))
}

fn is_better_name(candidate: &Option<String>, current: &str) -> bool {
    match candidate {
        Some(name) if !name.is_empty() && (current.is_empty() || current.starts_with("Razer device")) => {
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_peripheral_devices_excludes_laptop() {
        let entries = vec![
            RazerHidEntry {
                vid: RAZER_VID,
                pid: 0x02c6,
                product_string: Some("Razer Blade".into()),
                manufacturer_string: None,
                path: CString::new("/a").unwrap(),
                interface_number: 0,
                usage_page: 0,
                usage: 0,
            },
            RazerHidEntry {
                vid: RAZER_VID,
                pid: 0x00a6,
                product_string: Some("Razer Viper".into()),
                manufacturer_string: None,
                path: CString::new("/c").unwrap(),
                interface_number: 0,
                usage_page: 0,
                usage: 0,
            },
        ];

        let summaries = summarize_peripheral_devices(&entries);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].pid, 0x00a6);
        assert_eq!(summaries[0].kind, RazerDeviceKind::Peripheral);
    }

    #[test]
    fn blackshark_v3_classified_as_headset() {
        let entries = vec![RazerHidEntry {
            vid: RAZER_VID,
            pid: 0x057a,
            product_string: Some("BlackShark V3".into()),
            manufacturer_string: None,
            path: CString::new("/hs").unwrap(),
            interface_number: 0,
            usage_page: 0xFF14,
            usage: 0,
        }];

        let summaries = summarize_peripheral_devices(&entries);
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].kind, RazerDeviceKind::Headset);
    }

    #[test]
    fn summarize_groups_by_pid() {
        let entries = vec![
            RazerHidEntry {
                vid: RAZER_VID,
                pid: 0x02c6,
                product_string: Some("Razer Blade".into()),
                manufacturer_string: None,
                path: CString::new("/a").unwrap(),
                interface_number: 0,
                usage_page: 0,
                usage: 0,
            },
            RazerHidEntry {
                vid: RAZER_VID,
                pid: 0x02c6,
                product_string: Some("Razer Blade".into()),
                manufacturer_string: None,
                path: CString::new("/b").unwrap(),
                interface_number: 1,
                usage_page: 0,
                usage: 0,
            },
            RazerHidEntry {
                vid: RAZER_VID,
                pid: 0x00a6,
                product_string: Some("Razer Viper".into()),
                manufacturer_string: None,
                path: CString::new("/c").unwrap(),
                interface_number: 0,
                usage_page: 0,
                usage: 0,
            },
        ];

        let summaries = summarize_razer_devices(&entries);
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].interface_count, 2);
        assert_eq!(summaries[0].kind, RazerDeviceKind::BladeLaptop);
        assert_eq!(summaries[1].kind, RazerDeviceKind::Peripheral);
    }
}
