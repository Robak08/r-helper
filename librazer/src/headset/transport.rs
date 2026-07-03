use super::packet::REPORT_LEN;

use anyhow::{anyhow, Context, Result};
use std::{thread, time::Duration};

const READ_TIMEOUT_MS: i32 = 200;
const POST_WRITE_DELAY: Duration = Duration::from_millis(50);

pub fn write_report(device: &hidapi::HidDevice, report: &[u8]) -> Result<()> {
    let written = device
        .write(report)
        .with_context(|| format!("Failed to write {}-byte HID report", report.len()))?;
    if written != report.len() {
        return Err(anyhow!(
            "Short HID write: expected {} bytes, wrote {written}",
            report.len()
        ));
    }
    thread::sleep(POST_WRITE_DELAY);
    Ok(())
}

/// Read an input report after a write; returns `None` on timeout or empty response.
pub fn try_read_response(device: &hidapi::HidDevice) -> Result<Option<Vec<u8>>> {
    let mut buf = [0u8; REPORT_LEN];
    match device.read_timeout(&mut buf, READ_TIMEOUT_MS) {
        Ok(0) => Ok(None),
        Ok(n) => Ok(Some(buf[..n].to_vec())),
        Err(e) => {
            // hidapi returns an error on timeout on some platforms.
            let msg = e.to_string().to_lowercase();
            if msg.contains("timeout") || msg.contains("timed out") {
                Ok(None)
            } else {
                Err(e).context("Failed to read headset HID response")
            }
        }
    }
}

pub fn write_and_read(device: &hidapi::HidDevice, report: &[u8]) -> Result<Option<Vec<u8>>> {
    write_report(device, report)?;
    try_read_response(device)
}

/// Drain any stale input reports before a fresh query.
pub fn drain_input(device: &hidapi::HidDevice) {
    let mut buf = [0u8; REPORT_LEN];
    while device.read_timeout(&mut buf, 1).unwrap_or(0) > 0 {}
}
