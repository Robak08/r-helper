use eframe::egui::{self, RichText, Vec2};

use crate::bluetooth::{BluetoothHeadsetSummary, normalized_device_name};
use crate::ui::temp::info_temp_row;
use librazer::enumerate::{RazerDeviceKind, RazerDeviceSummary};
use librazer::types::BatteryCare;

const COMPACT_PROGRESS_WIDTH: f32 = 48.0;
const COMPACT_PROGRESS_HEIGHT: f32 = 12.0;
const PROGRESS_BAR_RADIUS: u8 = 2;

#[derive(Debug, Clone)]
pub struct CoolingPadInfoView {
    pub fan_mode: String,
    pub commanded_rpm: Option<u16>,
    pub lighting_mode: String,
    pub chroma_available: bool,
}

/// Data assembled on the app side for the Info tab laptop card.
#[derive(Debug, Clone)]
pub struct LaptopInfoView {
    pub model: String,
    pub sku: Option<String>,
    pub pid: Option<String>,
    pub gpus: Vec<String>,
    pub cpu: String,
    pub ram_gb: Option<u32>,
    pub battery_percent: Option<u8>,
    pub battery_charging: bool,
    pub battery_time_mins: Option<u32>,
    pub charge_limit: Option<u8>,
    pub ac_power: bool,
    pub cpu_avg_temp_c: Option<f32>,
    pub gpu_avg_temp_c: Option<f32>,
}

impl Default for LaptopInfoView {
    fn default() -> Self {
        Self {
            model: "Unknown".to_string(),
            gpus: vec!["Unknown".to_string()],
            cpu: "Unknown".to_string(),
            sku: None,
            pid: None,
            ram_gb: None,
            battery_percent: None,
            battery_charging: false,
            battery_time_mins: None,
            charge_limit: None,
            ac_power: true,
            cpu_avg_temp_c: None,
            gpu_avg_temp_c: None,
        }
    }
}

pub fn render_info_tab(
    ui: &mut egui::Ui,
    info: &LaptopInfoView,
    cooling_pad: Option<&CoolingPadInfoView>,
    razer_devices: &[RazerDeviceSummary],
    bluetooth_headsets: &[BluetoothHeadsetSummary],
) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.group(|ui| {
            ui.label(RichText::new("💻 This laptop").strong());
            ui.separator();

            egui::ScrollArea::horizontal().id_salt("laptop_info_hscroll").show(ui, |ui| {
                ui.vertical(|ui| {
                    let content_width = laptop_content_width(info);
                    ui.set_min_width(content_width.max(ui.available_width()));
                    info_row(ui, "Model", &info.model);
                    if let Some(sku) = &info.sku {
                        info_row(ui, "SKU", sku);
                    }
                    if let Some(pid) = &info.pid {
                        info_row(ui, "USB PID", pid);
                    }
                    info_row(ui, "CPU", &info.cpu);
                    if let Some(ram) = info.ram_gb {
                        info_row(ui, "RAM", &format!("{ram} GB"));
                    }
                    if !info.gpus.is_empty() {
                        info_row(ui, "GPU", &info.gpus.join(", "));
                    }
                    info_temp_row(ui, "CPU temp", info.cpu_avg_temp_c);
                    info_temp_row(ui, "GPU temp", info.gpu_avg_temp_c);
                });
            });
        });

        ui.add_space(8.0);

        ui.group(|ui| {
            ui.label(RichText::new("🔋 Battery").strong());
            ui.separator();
            info_row(ui, "Power", if info.ac_power { "AC" } else { "Battery" });
            match info.battery_percent {
                Some(pct) => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(RichText::new("Charge:").weak()).selectable(false));
                        compact_progress_bar(ui, pct);
                    });
                }
                None => info_row(ui, "Charge", "Unknown"),
            }
            let charging = if info.battery_charging { "Yes" } else { "No" };
            info_row(ui, "Charging", charging);
            if let Some(mins) = info.battery_time_mins {
                info_row(ui, "Time remaining", &format_time_mins(mins));
            }
            match info.charge_limit {
                Some(limit) if limit < 100 => {
                    info_row(ui, "Charge limit", &format!("{limit}% (Battery Health Optimizer)"));
                }
                _ => info_row(ui, "Charge limit", "Disabled (100%)"),
            }
        });

        ui.add_space(8.0);

        if let Some(pad) = cooling_pad {
            render_cooling_pad_info(ui, pad);
            ui.add_space(8.0);
        }

        render_razer_devices(ui, razer_devices, bluetooth_headsets);
    });
}

fn render_cooling_pad_info(ui: &mut egui::Ui, pad: &CoolingPadInfoView) {
    ui.group(|ui| {
        ui.label(RichText::new("🌀 Cooling pad").strong());
        ui.separator();
        info_row(ui, "Model", "Razer Laptop Cooling Pad");
        info_row(ui, "USB PID", "0x0f43");
        let fan_label = match pad.fan_mode.as_str() {
            "manual" => "Manual",
            "auto" => "Auto",
            _ => "Off",
        };
        info_row(ui, "Fan", fan_label);
        match pad.commanded_rpm {
            Some(rpm) => info_row(ui, "Set RPM", &format!("{rpm}")),
            None if pad.fan_mode == "auto" => {
                info_row(ui, "Set RPM", "Off (auto)");
            }
            None => {}
        }
        if pad.chroma_available {
            info_row(ui, "Lighting", &pad.lighting_mode);
        } else {
            info_row(ui, "Lighting", "Unavailable");
        }
    });
}

fn render_razer_devices(
    ui: &mut egui::Ui,
    devices: &[RazerDeviceSummary],
    bluetooth_headsets: &[BluetoothHeadsetSummary],
) {
    ui.group(|ui| {
        ui.label(RichText::new("🎧 Connected peripherals").strong());
        ui.separator();

        let bluetooth_headsets = bluetooth_headsets
            .iter()
            .filter(|headset| !duplicates_usb_headset(&headset.name, devices))
            .collect::<Vec<_>>();

        if devices.is_empty() && bluetooth_headsets.is_empty() {
            ui.label(RichText::new("No peripherals detected.").weak());
            return;
        }

        egui::Grid::new("razer_device_grid").num_columns(3).spacing([10.0, 4.0]).show(ui, |ui| {
            ui.label(RichText::new("Device").strong());
            ui.label(RichText::new("Battery").strong());
            ui.label(RichText::new("Status").strong());
            ui.end_row();

            for device in devices {
                ui.label(&device.name);

                match device.battery_percent {
                    Some(pct) => {
                        compact_progress_bar(ui, pct);
                    }
                    None if device.battery_available => {
                        ui.label("—");
                    }
                    None => {
                        ui.label(RichText::new("N/A").weak());
                    }
                }

                let status = peripheral_status(device);
                ui.label(status);
                ui.end_row();
            }

            for headset in bluetooth_headsets {
                ui.label(&headset.name);
                match headset.battery_percent {
                    Some(pct) => compact_progress_bar(ui, pct),
                    None => {
                        ui.label(RichText::new("N/A").weak());
                    }
                }
                ui.label(bluetooth_status(headset));
                ui.end_row();
            }
        });
    });
}

fn duplicates_usb_headset(name: &str, devices: &[RazerDeviceSummary]) -> bool {
    devices.iter().any(|device| {
        device.kind == RazerDeviceKind::Headset && same_normalized_device_name(name, &device.name)
    })
}

fn same_normalized_device_name(left: &str, right: &str) -> bool {
    let left = normalized_device_name(left);
    let right = normalized_device_name(right);
    !left.is_empty()
        && !right.is_empty()
        && (left == right
            || (left.len() >= 6 && right.contains(&left))
            || (right.len() >= 6 && left.contains(&right)))
}

fn bluetooth_status(headset: &BluetoothHeadsetSummary) -> &'static str {
    match headset.battery_percent {
        Some(pct) if pct <= 20 => "Bluetooth · Low",
        Some(_) => "Bluetooth · OK",
        None => "Bluetooth · Connected",
    }
}

fn peripheral_status(device: &RazerDeviceSummary) -> &'static str {
    match (device.battery_charging, device.battery_percent) {
        (Some(true), _) => "Charging",
        (_, Some(pct)) if pct <= 20 => "Low",
        (_, Some(_)) => "OK",
        _ if device.kind == RazerDeviceKind::Headset => "Unavailable",
        _ => "Wired or unavailable",
    }
}

fn compact_progress_bar(ui: &mut egui::Ui, pct: u8) {
    ui.add_sized(
        Vec2::new(COMPACT_PROGRESS_WIDTH, COMPACT_PROGRESS_HEIGHT),
        egui::ProgressBar::new(pct as f32 / 100.0)
            .desired_width(COMPACT_PROGRESS_WIDTH)
            .desired_height(COMPACT_PROGRESS_HEIGHT)
            .corner_radius(egui::CornerRadius::same(PROGRESS_BAR_RADIUS))
            .text(format!("{pct}%")),
    );
}

fn laptop_content_width(info: &LaptopInfoView) -> f32 {
    let gpu = info.gpus.join(", ");
    let longest = [
        info.model.as_str(),
        info.sku.as_deref().unwrap_or(""),
        info.pid.as_deref().unwrap_or(""),
        info.cpu.as_str(),
        gpu.as_str(),
    ]
    .iter()
    .map(|s| s.len())
    .max()
    .unwrap_or(0);

    72.0 + longest as f32 * 6.5
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(RichText::new(format!("{label}:")).weak()).selectable(false));
        ui.add(
            egui::Label::new(RichText::new(value))
                .selectable(false)
                .wrap_mode(egui::TextWrapMode::Extend),
        );
    });
}

fn format_time_mins(mins: u32) -> String {
    if mins >= 60 { format!("{} h {} min", mins / 60, mins % 60) } else { format!("{mins} min") }
}

impl LaptopInfoView {
    pub fn charge_limit_from_care(care: BatteryCare) -> Option<u8> {
        let pct = care.to_percent();
        if pct >= 100 { None } else { Some(pct) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicates_usb_and_bluetooth_transport_names() {
        assert!(same_normalized_device_name(
            "Razer Barracuda X Bluetooth Stereo",
            "Razer Barracuda X"
        ));
        assert!(same_normalized_device_name("Razer BlackShark V3 BT", "BlackShark V3"));
        assert!(!same_normalized_device_name("Razer Barracuda X", "Razer Kraken Kitty"));
    }
}
