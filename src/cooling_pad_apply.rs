use anyhow::Result;
use librazer::{
    chroma::Rgb,
    cooling_pad::{CoolingPadDevice, PadLightingMode},
};

pub fn apply_pad_lighting(
    pad: &CoolingPadDevice,
    mode: PadLightingMode,
    rgb: Rgb,
    brightness: u8,
    clear_first: bool,
) -> Result<()> {
    pad.apply_lighting(mode, rgb, brightness, clear_first)
}

pub fn apply_pad_color_change(
    pad: &CoolingPadDevice,
    mode: PadLightingMode,
    rgb: Rgb,
    brightness: u8,
) -> Result<()> {
    pad.apply_color_change(mode, rgb, brightness)
}

pub fn apply_pad_config_lighting(
    pad: &CoolingPadDevice,
    mode: PadLightingMode,
    rgb: Rgb,
    brightness: u8,
) -> Result<()> {
    if mode == PadLightingMode::Off {
        pad.apply_lighting(mode, rgb, brightness, false)
    } else {
        pad.apply_color_change(mode, rgb, brightness)
    }
}

pub fn set_pad_brightness(pad: &CoolingPadDevice, brightness: u8) -> Result<()> {
    pad.set_brightness(brightness)
}
