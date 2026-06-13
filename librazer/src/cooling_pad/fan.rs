use anyhow::{ensure, Context, Result};

pub const REPORT_LEN: usize = 91;
pub const MIN_RPM: u16 = 500;
pub const MAX_RPM: u16 = 3200;
pub const RPM_STEP: u16 = 50;

const REPORT_ID: u8 = 0x00;
const REPORT_CODE: usize = 8;
const SUB_VER: usize = 9;
const CURVE_ID: usize = 10;
const RPM_L: usize = 11;
const RPM_H: usize = 12;
const CHK_L: usize = 89;
const CHK_H: usize = 90;

/// Fixed payload template (bytes 1..=90, excluding report ID).
const HEADER_TEMPLATE: [u8; 90] = {
    let mut h = [0u8; 90];
    h[1] = 0x02;
    h[5] = 0x03;
    h[6] = 0x0D;
    h[7] = 0x10;
    h[8] = 0x01;
    h[9] = 0x02;
    h[10] = 0x36;
    h
};

fn build_buffer(report_code: u8, sub_ver: u8, curve_id: u8, rpm: u16, chk_l: u8) -> [u8; REPORT_LEN] {
    let mut buf = [0u8; REPORT_LEN];
    buf[0] = REPORT_ID;
    buf[1..].copy_from_slice(&HEADER_TEMPLATE);
    buf[REPORT_CODE] = report_code;
    buf[SUB_VER] = sub_ver;
    buf[CURVE_ID] = curve_id;

    let raw = (rpm / RPM_STEP) as u16;
    buf[RPM_L] = (raw & 0xFF) as u8;
    buf[RPM_H] = (raw >> 8) as u8;
    buf[CHK_L] = chk_l;
    buf[CHK_H] = 0x00;
    buf
}

pub fn probe_fan(device: &hidapi::HidDevice) -> bool {
    let mut buf = [0u8; REPORT_LEN];
    buf[0] = REPORT_ID;
    if get_feature(device, &mut buf).is_ok() {
        return true;
    }
    // Some interfaces accept writes before reads succeed (e.g. right after plug-in).
    device.send_feature_report(&buf).is_ok()
}

pub fn set_rpm(device: &hidapi::HidDevice, rpm: u16) -> Result<()> {
    ensure!((MIN_RPM..=MAX_RPM).contains(&rpm), "RPM must be between {} and {}", MIN_RPM, MAX_RPM);
    let rounded = ((rpm as f32 / RPM_STEP as f32).round() as u16) * RPM_STEP;
    let raw = rounded / RPM_STEP;
    let buf = build_buffer(0x01, 0x01, 0x05, rounded, (raw & 0xFF) as u8 ^ 0x0B);
    send_feature(device, &buf)?;
    Ok(())
}

pub fn fan_off(device: &hidapi::HidDevice) -> Result<()> {
    let buf = build_buffer(0x10, 0x00, 0x06, 0, 0x18);
    send_feature(device, &buf)?;
    Ok(())
}

pub fn get_commanded_rpm(device: &hidapi::HidDevice) -> Result<u16> {
    let mut buf = [0u8; REPORT_LEN];
    buf[0] = REPORT_ID;
    get_feature(device, &mut buf)?;
    parse_rpm_from_buffer(&buf).ok_or_else(|| {
        anyhow::anyhow!("Cooling pad fan report does not contain a valid RPM value")
    })
}

/// Parse RPM only from an active manual-curve report (avoids garbage reads after off/transitions).
pub fn parse_rpm_from_buffer(buf: &[u8; REPORT_LEN]) -> Option<u16> {
    if buf[REPORT_CODE] != 0x01 || buf[CURVE_ID] != 0x05 {
        return None;
    }
    let raw = buf[RPM_L] as u16 | ((buf[RPM_H] as u16) << 8);
    if raw == 0 {
        return Some(0);
    }
    let rpm = raw * RPM_STEP;
    ((MIN_RPM..=MAX_RPM).contains(&rpm)).then_some(rpm)
}

fn send_feature(device: &hidapi::HidDevice, buf: &[u8; REPORT_LEN]) -> Result<()> {
    device
        .send_feature_report(buf)
        .context("Failed to send cooling pad fan feature report")?;
    Ok(())
}

fn get_feature(device: &hidapi::HidDevice, buf: &mut [u8; REPORT_LEN]) -> Result<()> {
    let size = device
        .get_feature_report(buf)
        .context("Failed to read cooling pad fan feature report")?;
    ensure!(size >= REPORT_LEN, "Cooling pad fan response too short: {size}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rpm_rejects_off_report() {
        let mut buf = [0u8; REPORT_LEN];
        buf[REPORT_CODE] = 0x10;
        buf[CURVE_ID] = 0x06;
        buf[RPM_L] = 105;
        assert_eq!(parse_rpm_from_buffer(&buf), None);
    }

    #[test]
    fn parse_rpm_accepts_valid_curve() {
        let mut buf = [0u8; REPORT_LEN];
        buf[REPORT_CODE] = 0x01;
        buf[CURVE_ID] = 0x05;
        buf[RPM_L] = 33; // 1650 RPM
        assert_eq!(parse_rpm_from_buffer(&buf), Some(1650));
    }
}
