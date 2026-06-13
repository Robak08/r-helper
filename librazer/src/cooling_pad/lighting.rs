use crate::chroma::effects::{
    build_extended_breathing_single, build_extended_brightness, build_extended_get_brightness,
    build_extended_none, build_extended_static, Rgb, TRANSACTION_ID_COOLING_PAD, VARSTORE, ZERO_LED,
};
use crate::chroma::send_feature_report;

use anyhow::{Context, Result};
use std::{thread, time::Duration};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadLightingMode {
    Off,
    Static,
    Breathing,
}

impl PadLightingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Static => "Static",
            Self::Breathing => "Breathing",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Off" => Some(Self::Off),
            "Static" => Some(Self::Static),
            "Breathing" => Some(Self::Breathing),
            _ => None,
        }
    }
}

pub fn probe_chroma(device: &hidapi::HidDevice) -> bool {
    let report = build_extended_get_brightness(TRANSACTION_ID_COOLING_PAD, VARSTORE, ZERO_LED);
    send_feature_report(device, &report).is_ok()
}

pub fn set_mode(device: &hidapi::HidDevice, mode: PadLightingMode, rgb: Rgb) -> Result<()> {
    let report = match mode {
        PadLightingMode::Off => build_extended_none(TRANSACTION_ID_COOLING_PAD, VARSTORE, ZERO_LED),
        PadLightingMode::Static => {
            build_extended_static(TRANSACTION_ID_COOLING_PAD, VARSTORE, ZERO_LED, rgb)
        }
        PadLightingMode::Breathing => {
            build_extended_breathing_single(TRANSACTION_ID_COOLING_PAD, VARSTORE, ZERO_LED, rgb)
        }
    };
    send_feature_report(device, &report).context("Failed to set cooling pad lighting mode")?;
    Ok(())
}

/// Apply mode, color, and brightness. Clears the current effect first when requested —
/// required for reliable color/mode updates on the pad.
pub fn apply_lighting(
    device: &hidapi::HidDevice,
    mode: PadLightingMode,
    rgb: Rgb,
    brightness: u8,
    clear_first: bool,
) -> Result<()> {
    if mode == PadLightingMode::Off {
        set_mode(device, mode, rgb)?;
        return Ok(());
    }

    if clear_first {
        let none = build_extended_none(TRANSACTION_ID_COOLING_PAD, VARSTORE, ZERO_LED);
        send_feature_report(device, &none).context("Failed to clear cooling pad lighting")?;
        thread::sleep(Duration::from_millis(50));
    }

    set_mode(device, mode, rgb)?;
    thread::sleep(Duration::from_millis(50));
    set_brightness(device, brightness)?;
    Ok(())
}
/// Re-apply color by cycling off → alternate mode → target mode (pad firmware ignores
/// color-only updates within the same effect).
pub fn apply_color_change(
    device: &hidapi::HidDevice,
    mode: PadLightingMode,
    rgb: Rgb,
    brightness: u8,
) -> Result<()> {
    let alternate = match mode {
        PadLightingMode::Off => return Ok(()),
        PadLightingMode::Static => PadLightingMode::Breathing,
        PadLightingMode::Breathing => PadLightingMode::Static,
    };

    set_mode(device, PadLightingMode::Off, rgb)?;
    thread::sleep(Duration::from_millis(100));
    set_mode(device, alternate, rgb)?;
    thread::sleep(Duration::from_millis(100));
    set_mode(device, mode, rgb)?;
    thread::sleep(Duration::from_millis(50));
    set_brightness(device, brightness)
}

pub fn set_brightness(device: &hidapi::HidDevice, brightness: u8) -> Result<()> {
    let report =
        build_extended_brightness(TRANSACTION_ID_COOLING_PAD, VARSTORE, ZERO_LED, brightness);
    send_feature_report(device, &report).context("Failed to set cooling pad brightness")?;
    Ok(())
}

pub fn get_brightness(device: &hidapi::HidDevice) -> Result<u8> {
    let report = build_extended_get_brightness(TRANSACTION_ID_COOLING_PAD, VARSTORE, ZERO_LED);
    let response =
        send_feature_report(device, &report).context("Failed to read cooling pad brightness")?;
    // Response args: [varstore, led_id, brightness] at bytes 8..=10
    Ok(response.get(10).copied().unwrap_or(0))
}
