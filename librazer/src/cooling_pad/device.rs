use super::fan::{self, fan_off, set_rpm};
use super::lighting::{self, PadLightingMode};

use crate::chroma::Rgb;
use crate::enumerate::{list_razer_hid_devices, RAZER_VID};

use anyhow::{Context, Result};
use std::ffi::CString;

pub const COOLING_PAD_PID: u16 = 0x0F43;

pub struct CoolingPadDevice {
    fan: hidapi::HidDevice,
    chroma: Option<hidapi::HidDevice>,
    chroma_available: bool,
}

impl CoolingPadDevice {
    pub fn detect() -> Result<Self> {
        let api = hidapi::HidApi::new().context("Failed to create hid api")?;
        let entries: Vec<_> = list_razer_hid_devices()?
            .into_iter()
            .filter(|e| e.pid == COOLING_PAD_PID)
            .collect();

        if entries.is_empty() {
            anyhow::bail!("Razer Laptop Cooling Pad not found");
        }

        let fan_path = pick_fan_path(&api, &entries)?;
        let fan = api
            .open_path(fan_path.as_c_str())
            .with_context(|| format!("Failed to open cooling pad fan {:?}", fan_path))?;

        let chroma_path = pick_chroma_path(&api, &entries, &fan_path);
        let (chroma, chroma_available) = if let Some(path) = chroma_path {
            match api.open_path(path.as_c_str()) {
                Ok(device) if lighting::probe_chroma(&device) => (Some(device), true),
                _ => (None, false),
            }
        } else {
            (None, false)
        };

        Ok(Self { fan, chroma, chroma_available })
    }

    pub fn chroma_available(&self) -> bool {
        self.chroma_available
    }

    /// Returns false when the USB HID handle is stale (e.g. after system resume).
    pub fn is_responsive(&self) -> bool {
        fan::probe_fan(&self.fan)
    }

    pub fn set_fan_rpm(&self, rpm: u16) -> Result<()> {
        set_rpm(&self.fan, rpm)
    }

    pub fn fan_off(&self) -> Result<()> {
        fan_off(&self.fan)
    }

    pub fn apply_lighting(
        &self,
        mode: PadLightingMode,
        rgb: Rgb,
        brightness: u8,
        clear_first: bool,
    ) -> Result<()> {
        let Some(chroma) = self.chroma.as_ref() else {
            anyhow::bail!("Cooling pad lighting is not available");
        };
        lighting::apply_lighting(chroma, mode, rgb, brightness, clear_first)
    }

    pub fn apply_color_change(
        &self,
        mode: PadLightingMode,
        rgb: Rgb,
        brightness: u8,
    ) -> Result<()> {
        let Some(chroma) = self.chroma.as_ref() else {
            anyhow::bail!("Cooling pad lighting is not available");
        };
        lighting::apply_color_change(chroma, mode, rgb, brightness)
    }

    pub fn set_brightness(&self, brightness: u8) -> Result<()> {
        let Some(chroma) = self.chroma.as_ref() else {
            anyhow::bail!("Cooling pad lighting is not available");
        };
        lighting::set_brightness(chroma, brightness)
    }

    pub fn brightness(&self) -> Result<u8> {
        let Some(chroma) = self.chroma.as_ref() else {
            anyhow::bail!("Cooling pad lighting is not available");
        };
        lighting::get_brightness(chroma)
    }
}

fn pick_fan_path(api: &hidapi::HidApi, entries: &[crate::enumerate::RazerHidEntry]) -> Result<CString> {
    let mut candidates: Vec<_> = entries.iter().collect();
    candidates.sort_by_key(|e| std::cmp::Reverse(e.interface_number));

    for entry in candidates {
        if let Ok(device) = api.open_path(entry.path.as_c_str()) {
            if fan::probe_fan(&device) {
                return Ok(entry.path.clone());
            }
        }
    }

    anyhow::bail!("Failed to open cooling pad fan interface")
}

fn pick_chroma_path(
    api: &hidapi::HidApi,
    entries: &[crate::enumerate::RazerHidEntry],
    fan_path: &CString,
) -> Option<CString> {
    let mut candidates: Vec<_> = entries
        .iter()
        .filter(|e| e.path.as_bytes() != fan_path.as_bytes())
        .collect();
    candidates.sort_by_key(|e| std::cmp::Reverse(e.interface_number));

    for entry in candidates {
        if let Ok(device) = api.open_path(entry.path.as_c_str()) {
            if lighting::probe_chroma(&device) {
                return Some(entry.path.clone());
            }
        }
    }

    if let Ok(device) = api.open_path(fan_path.as_c_str()) {
        if lighting::probe_chroma(&device) {
            return Some(fan_path.clone());
        }
    }

    None
}

pub fn is_present() -> bool {
    list_razer_hid_devices()
        .map(|entries| entries.iter().any(|e| e.vid == RAZER_VID && e.pid == COOLING_PAD_PID))
        .unwrap_or(false)
}
