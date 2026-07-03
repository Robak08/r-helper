use super::packet::{
    build_report, CMD_CLASS_BATTERY, CMD_CLASS_CHARGING, CMD_CLASS_INIT, RF_WAKE_REPORT,
};
use super::pids::HeadsetProfile;
use super::transport::{write_and_read, write_report};

use crate::chroma::PeripheralBattery;

use anyhow::Result;
use std::{thread, time::Duration};

const ARMING_DELAY: Duration = Duration::from_millis(50);
const RF_WAKE_BURST_GAP: Duration = Duration::from_millis(5);

/// Run the full arming handshake (STEP1, STEP2, RF_WAKE burst, status + battery queries).
pub fn arm_device(device: &hidapi::HidDevice, profile: HeadsetProfile) -> Result<()> {
    let step1 = build_report(false, CMD_CLASS_INIT);
    write_report(device, &step1)?;
    thread::sleep(ARMING_DELAY);

    let step2 = build_report(false, CMD_CLASS_CHARGING);
    write_report(device, &step2)?;
    thread::sleep(ARMING_DELAY);

    if profile == HeadsetProfile::WirelessDongle {
        send_rf_wake_burst(device)?;
    }

    let _ = write_and_read(device, &build_report(true, CMD_CLASS_CHARGING))?;
    let _ = write_and_read(device, &build_report(true, CMD_CLASS_BATTERY))?;
    Ok(())
}

pub fn send_rf_wake(device: &hidapi::HidDevice) -> Result<()> {
    write_report(device, &RF_WAKE_REPORT)
}

fn send_rf_wake_burst(device: &hidapi::HidDevice) -> Result<()> {
    for _ in 0..3 {
        send_rf_wake(device)?;
        thread::sleep(RF_WAKE_BURST_GAP);
    }
    Ok(())
}

/// Query battery level and charging state from an armed headset session.
pub fn query_battery(device: &hidapi::HidDevice, profile: HeadsetProfile) -> Result<Option<PeripheralBattery>> {
    if profile == HeadsetProfile::WirelessDongle {
        send_rf_wake(device)?;
    }

    let battery_resp = match write_and_read(device, &build_report(true, CMD_CLASS_BATTERY))? {
        Some(resp) => resp,
        None => return Ok(None),
    };

    let percent = match parse_value_byte(&battery_resp) {
        Some(p) => p,
        None => return Ok(None),
    };
    if percent == 0 {
        return Ok(None);
    }

    let charging_resp = write_and_read(device, &build_report(true, CMD_CLASS_CHARGING))?;
    let charging = charging_resp
        .as_deref()
        .and_then(parse_value_byte)
        .map(|v| v != 0)
        .unwrap_or(false);

    Ok(Some(PeripheralBattery {
        percent: percent.min(100),
        charging,
    }))
}

/// Best-effort one-shot probe: try a direct query, then fall back to full arming.
pub fn probe_battery_once(
    device: &hidapi::HidDevice,
    profile: HeadsetProfile,
) -> Result<Option<PeripheralBattery>> {
    if let Some(battery) = query_battery(device, profile)? {
        return Ok(Some(battery));
    }

    arm_device(device, profile)?;
    query_battery(device, profile)
}

fn parse_value_byte(response: &[u8]) -> Option<u8> {
    if response.len() > 13 {
        let value = response[13];
        if value > 0 {
            return Some(value);
        }
    }
    // Some responses omit the report ID in the read buffer.
    if response.len() > 12 && response.get(0) != Some(&0x02) {
        let value = response[12];
        if value > 0 {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::parse_value_byte;

    #[test]
    fn parse_value_from_offset_13() {
        let mut resp = vec![0u8; 20];
        resp[13] = 72;
        assert_eq!(parse_value_byte(&resp), Some(72));
    }

    #[test]
    fn parse_value_zero_returns_none() {
        let resp = vec![0u8; 20];
        assert_eq!(parse_value_byte(&resp), None);
    }
}
