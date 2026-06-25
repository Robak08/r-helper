use eframe::egui::{self, Color32, RichText};
use librazer::cooling_pad::{MAX_RPM, MIN_RPM, RPM_STEP};

const RPM_STEP_F64: f64 = RPM_STEP as f64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoolingPadFanMode {
    Off,
    Manual,
    Auto,
}

impl CoolingPadFanMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Manual => "manual",
            Self::Auto => "auto",
        }
    }

    pub fn from_config(s: &str) -> Self {
        match s {
            "manual" => Self::Manual,
            "auto" => Self::Auto,
            _ => Self::Off,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoolingPadFanAction {
    None,
    SetMode(CoolingPadFanMode),
    SetManualRpm(u16),
    ManualRpmDragging(u16),
    SetAutoMinRpm(u16),
    SetAutoMaxRpm(u16),
    SetAutoOffBelowC(f32),
    SetAutoFullAboveC(f32),
    ToggleFollowLaptopFan(bool),
}

pub fn render_cooling_pad_fan_section(
    ui: &mut egui::Ui,
    fan_mode: CoolingPadFanMode,
    display_rpm: Option<u16>,
    manual_rpm: &mut u16,
    auto_min_rpm: &mut u16,
    auto_max_rpm: &mut u16,
    auto_off_below_c: &mut f32,
    auto_full_above_c: &mut f32,
    follow_laptop_fan: &mut bool,
) -> CoolingPadFanAction {
    let mut action = CoolingPadFanAction::None;

    ui.group(|ui| {
        ui.add(egui::Label::new("🌀 Cooling Pad Fan").selectable(false));
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Set RPM:");
            match fan_mode {
                CoolingPadFanMode::Off => {
                    ui.label(RichText::new("Off").weak());
                }
                CoolingPadFanMode::Manual => {
                    if let Some(rpm) = display_rpm {
                        ui.label(RichText::new(format!("{rpm}")).color(Color32::from_rgb(0, 180, 0)));
                    } else {
                        ui.label(RichText::new("Off").weak());
                    }
                }
                CoolingPadFanMode::Auto => {
                    if let Some(rpm) = display_rpm {
                        ui.label(RichText::new(format!("{rpm}")).color(Color32::from_rgb(0, 180, 0)));
                    } else {
                        ui.label(
                            RichText::new(format!("Off · {}–{}", *auto_min_rpm, *auto_max_rpm))
                                .weak(),
                        );
                    }
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("Mode:");
            for (label, mode) in [
                ("Manual", CoolingPadFanMode::Manual),
                ("Auto", CoolingPadFanMode::Auto),
                ("Off", CoolingPadFanMode::Off),
            ] {
                let selected = fan_mode == mode;
                if ui.selectable_label(selected, label).clicked() && !selected {
                    action = CoolingPadFanAction::SetMode(mode);
                }
            }
        });

        if fan_mode == CoolingPadFanMode::Manual {
            ui.horizontal(|ui| {
                ui.label("RPM:");
                let mut rpm_f = *manual_rpm as f64;
                let response = ui.add(
                    egui::Slider::new(&mut rpm_f, MIN_RPM as f64..=MAX_RPM as f64)
                        .step_by(RPM_STEP_F64)
                        .custom_formatter(|val, _| format!("{}", val as u16)),
                );
                let rpm = ((rpm_f / RPM_STEP_F64).round() * RPM_STEP_F64) as u16;
                if rpm != *manual_rpm {
                    *manual_rpm = rpm;
                    if response.dragged() || response.has_focus() {
                        action = CoolingPadFanAction::ManualRpmDragging(rpm);
                    } else {
                        action = CoolingPadFanAction::SetManualRpm(rpm);
                    }
                } else if response.drag_stopped() || response.lost_focus() {
                    action = CoolingPadFanAction::SetManualRpm(rpm);
                }
            });
        }

        if fan_mode == CoolingPadFanMode::Auto {
            ui.horizontal(|ui| {
                let mut follow = *follow_laptop_fan;
                if ui.checkbox(&mut follow, "Follow laptop fan").changed() {
                    *follow_laptop_fan = follow;
                    action = CoolingPadFanAction::ToggleFollowLaptopFan(follow);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Off below:");
                let mut off = *auto_off_below_c;
                if ui
                    .add(egui::Slider::new(&mut off, 40.0..=75.0).suffix("°C"))
                    .changed()
                {
                    *auto_off_below_c = off;
                    action = CoolingPadFanAction::SetAutoOffBelowC(off);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Min RPM:");
                let mut min = *auto_min_rpm;
                if ui
                    .add(
                        egui::Slider::new(&mut min, MIN_RPM..=MAX_RPM)
                            .step_by(RPM_STEP_F64)
                            .custom_formatter(|val, _| format!("{}", val as u16)),
                    )
                    .changed()
                {
                    min = ((min as f32 / RPM_STEP as f32).round() as u16 * RPM_STEP as u16)
                        .clamp(MIN_RPM, *auto_max_rpm);
                    *auto_min_rpm = min;
                    action = CoolingPadFanAction::SetAutoMinRpm(min);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Max RPM:");
                let mut max = *auto_max_rpm;
                if ui
                    .add(
                        egui::Slider::new(&mut max, MIN_RPM..=MAX_RPM)
                            .step_by(RPM_STEP_F64)
                            .custom_formatter(|val, _| format!("{}", val as u16)),
                    )
                    .changed()
                {
                    max = ((max as f32 / RPM_STEP as f32).round() as u16 * RPM_STEP as u16)
                        .clamp(*auto_min_rpm, MAX_RPM);
                    *auto_max_rpm = max;
                    action = CoolingPadFanAction::SetAutoMaxRpm(max);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Full speed above:");
                let mut full = *auto_full_above_c;
                let min_full = *auto_off_below_c + 5.0;
                if ui
                    .add(egui::Slider::new(&mut full, min_full..=95.0).suffix("°C"))
                    .changed()
                {
                    *auto_full_above_c = full.max(min_full);
                    action = CoolingPadFanAction::SetAutoFullAboveC(*auto_full_above_c);
                }
            });
        }
    });

    action
}
