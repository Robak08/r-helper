use crate::chroma::packet::{build_request, response_ok, xor_crc, REPORT_LEN};
use crate::enumerate::RazerHidEntry;

use anyhow::{anyhow, Context, Result};
use std::{thread, time::Duration};

const CMD_CLASS_POWER: u8 = 0x07;
const CMD_BATTERY: u8 = 0x80;
const CMD_CHARGING: u8 = 0x84;

const DEFAULT_TRANSACTION_IDS: &[u8] = &[0x1F, 0x3F, 0xFF];

/// Battery reading for a Razer peripheral HID interface.
pub struct ChromaHandle {
    device: hidapi::HidDevice,
    transaction_id: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeripheralBattery {
    pub percent: u8,
    pub charging: bool,
}

impl ChromaHandle {
    /// Open a HID path and detect a working transaction ID via battery probe.
    pub fn try_open(entry: &RazerHidEntry) -> Result<Self> {
        let api = hidapi::HidApi::new().context("Failed to create hid api")?;
        let device = api
            .open_path(entry.path.as_c_str())
            .with_context(|| format!("Failed to open {:?}", entry.path))?;

        for &transaction_id in transaction_ids_for_pid(entry.pid) {
            if let Ok(Some(_)) = Self::query_battery_inner(&device, transaction_id) {
                return Ok(Self { device, transaction_id });
            }
        }

        anyhow::bail!(
            "No working Chroma transaction ID for PID 0x{:04x} on {}",
            entry.pid,
            entry.path.to_string_lossy()
        )
    }

    pub fn read_battery(&self) -> Result<PeripheralBattery> {
        let level = Self::query_battery_inner(&self.device, self.transaction_id)?
            .ok_or_else(|| anyhow!("Battery query returned no data"))?;
        Ok(level)
    }

    fn query_battery_inner(
        device: &hidapi::HidDevice,
        transaction_id: u8,
    ) -> Result<Option<PeripheralBattery>> {
        let battery_req = build_power_request(transaction_id, CMD_BATTERY);
        let battery_resp = match send_feature(device, &battery_req) {
            Ok(resp) => resp,
            Err(_) => return Ok(None),
        };

        if !response_ok(&battery_resp) {
            return Ok(None);
        }

        let raw = battery_resp[9];
        if raw == 0 {
            return Ok(None);
        }

        let percent = ((raw as u16 * 100) / 255).min(100) as u8;

        let charging_req = build_power_request(transaction_id, CMD_CHARGING);
        let charging = match send_feature(device, &charging_req) {
            Ok(resp) if response_ok(&resp) => resp.get(11).copied() == Some(0x01),
            _ => false,
        };

        Ok(Some(PeripheralBattery { percent, charging }))
    }
}

fn build_power_request(transaction_id: u8, command_id: u8) -> [u8; REPORT_LEN] {
    let mut report = build_request(transaction_id, CMD_CLASS_POWER, command_id, &[]);
    report[5] = 0x02;
    report[88] = xor_crc(&report);
    report
}

fn send_feature(device: &hidapi::HidDevice, report: &[u8; REPORT_LEN]) -> Result<Vec<u8>> {
    let mut send_buf = vec![0u8; 1 + REPORT_LEN];
    send_buf[1..].copy_from_slice(report);

    device
        .send_feature_report(&send_buf)
        .context("Failed to send Chroma feature report")?;

    thread::sleep(Duration::from_millis(50));

    let mut recv_buf = vec![0u8; 1 + REPORT_LEN];
    let size = device
        .get_feature_report(&mut recv_buf)
        .context("Failed to read Chroma feature report")?;

    if size <= 1 {
        return Err(anyhow!("Chroma response too short: {size}"));
    }

    Ok(recv_buf[1..].to_vec())
}

fn transaction_ids_for_pid(pid: u16) -> &'static [u8] {
    // OpenRazer groupings — expand as hardware is validated.
    match pid {
        // DeathAdder V2 Pro, Mamba, Lancehead wired/wireless groups use 0x3F
        0x007a | 0x007b | 0x007c | 0x007d | 0x007e | 0x007f => &[0x3F, 0x1F, 0xFF],
        _ => DEFAULT_TRANSACTION_IDS,
    }
}

/// Best-effort battery read without keeping the handle open.
pub fn probe_peripheral_battery(entry: &RazerHidEntry) -> Option<PeripheralBattery> {
    ChromaHandle::try_open(entry).ok().and_then(|h| h.read_battery().ok())
}
