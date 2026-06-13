use super::packet::{response_ok, REPORT_LEN};

use anyhow::{anyhow, Context, Result};
use std::{thread, time::Duration};

pub fn send_feature_report(device: &hidapi::HidDevice, report: &[u8; REPORT_LEN]) -> Result<Vec<u8>> {
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

    let response = recv_buf[1..].to_vec();
    if !response_ok(&response) {
        return Err(anyhow!("Chroma device returned error status {}", response[0]));
    }

    Ok(response)
}
