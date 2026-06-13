use eframe::egui::{self, Align, Color32, Layout, RichText};

use crate::ui::temp::temp_rich_text;

const MIN_RPM_FOR_COLOR: f32 = 1900.0;
const MAX_RPM_FOR_COLOR: f32 = 5000.0;
const MIN_MANUAL_RPM: u16 = 2000;
const MAX_MANUAL_RPM: u16 = 5500;
const RPM_STEP: f64 = 100.0;
const DARK_GREEN_MAX: u8 = 120;
const ORANGE_MAX: u8 = 100;

#[derive(Debug, Clone, PartialEq)]
pub enum FanAction {
    None,
    SetAutoMode,
    SetManualMode(u16),
    SetManualRpm(u16),
    SliderDragging(u16),
    ToggleAutoFanLimit(bool),
    AutoMaxRpmDragging(u16),
    SetAutoFanMaxRpm(u16),
}

pub fn render_fan_section(
    ui: &mut egui::Ui,
    fan_speed: &str,
    fan_actual_rpm: Option<u16>,
    fan_set_rpm: Option<u16>,
    manual_fan_rpm: &mut u16,
    auto_fan_limit_enabled: bool,
    auto_fan_max_rpm: &mut u16,
    show_status_messages: bool,
    custom_mode_active: bool,
    max_fan_speed_enabled: bool,
    cpu_avg_temp_c: Option<f32>,
    gpu_avg_temp_c: Option<f32>,
) -> (FanAction, bool) {
    let mut action = FanAction::None;
    let mut toggle_max = max_fan_speed_enabled;

    ui.group(|ui| {
        render_fan_header(
            ui,
            fan_actual_rpm,
            fan_set_rpm,
            auto_fan_limit_enabled,
            *auto_fan_max_rpm,
            show_status_messages,
        );
        ui.separator();
        // Fan Mode Selection row with Max on the right
        let available_width = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::Vec2::new(available_width, ui.spacing().interact_size.y),
            Layout::left_to_right(Align::Center),
            |ui| {
                // Use two columns for clean right alignment
                ui.columns(2, |cols| {
                    // Left column: Auto / Manual
                    cols[0].horizontal(|ui| {
                        let auto_selected = fan_speed.eq_ignore_ascii_case("auto");
                        if ui.selectable_label(auto_selected, "Auto").clicked() && !auto_selected {
                            action = FanAction::SetAutoMode;
                        }
                        let manual_selected = fan_speed.eq_ignore_ascii_case("manual");
                        if ui.selectable_label(manual_selected, "Manual").clicked()
                            && !manual_selected
                        {
                            action = FanAction::SetManualMode(*manual_fan_rpm);
                        }
                    });
                    // Right column: Max (toggle) - only when Custom mode AND in-app Debug are enabled
                    cols[1].with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if custom_mode_active && show_status_messages {
                            let max_selected = toggle_max;
                            let response = ui.selectable_label(max_selected, "Max");
                            if response.clicked() {
                                toggle_max = !toggle_max;
                            }
                        }
                    });
                });
            },
        );

        if fan_speed.eq_ignore_ascii_case("auto") {
            if let Some(auto_action) =
                render_auto_fan_limit_controls(ui, auto_fan_limit_enabled, auto_fan_max_rpm)
            {
                action = auto_action;
            }
        }

        // Manual RPM Slider (shown only in manual mode)
        if fan_speed.eq_ignore_ascii_case("manual") {
            if let Some(manual_action) = render_manual_fan_controls(ui, manual_fan_rpm) {
                action = manual_action;
            }
        }

        render_current_status(
            ui,
            fan_speed,
            auto_fan_limit_enabled,
            *auto_fan_max_rpm,
            cpu_avg_temp_c,
            gpu_avg_temp_c,
        );
    });

    (action, toggle_max)
}

fn render_fan_header(
    ui: &mut egui::Ui,
    fan_actual_rpm: Option<u16>,
    fan_set_rpm: Option<u16>,
    auto_fan_limit_enabled: bool,
    auto_fan_max_rpm: u16,
    show_status_messages: bool,
) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new("🌀 Fan Control").selectable(false));

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(actual_rpm) = fan_actual_rpm {
                let rpm_color = calculate_rpm_color(actual_rpm);
                ui.add(
                    egui::Label::new(RichText::new(format!("{} RPM", actual_rpm)).color(rpm_color))
                        .selectable(false),
                );
            } else {
                ui.add(egui::Label::new(RichText::new("N/A")).selectable(false));
            }

            if show_status_messages {
                if let Some(set_rpm) = fan_set_rpm {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("Set: {} |", set_rpm)).color(Color32::LIGHT_GRAY),
                        )
                        .selectable(false),
                    );
                } else if auto_fan_limit_enabled {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("Set: Auto max {} |", auto_fan_max_rpm))
                                .color(Color32::LIGHT_GRAY),
                        )
                        .selectable(false),
                    );
                } else {
                    ui.add(
                        egui::Label::new(RichText::new("Set: Auto |").color(Color32::LIGHT_GRAY))
                            .selectable(false),
                    );
                }
            }
        });
    });
}

fn render_auto_fan_limit_controls(
    ui: &mut egui::Ui,
    auto_fan_limit_enabled: bool,
    auto_fan_max_rpm: &mut u16,
) -> Option<FanAction> {
    let mut limit_enabled = auto_fan_limit_enabled;
    let mut action = None;

    ui.horizontal(|ui| {
        if ui
            .checkbox(&mut limit_enabled, "Limit max RPM")
            .changed()
        {
            action = Some(FanAction::ToggleAutoFanLimit(limit_enabled));
        }

        ui.add_enabled_ui(limit_enabled, |ui| {
            let slider_width = ui.available_width().max(ui.spacing().interact_size.y * 8.0);
            let response = ui.add_sized(
                [slider_width, ui.spacing().interact_size.y],
                egui::Slider::new(auto_fan_max_rpm, MIN_MANUAL_RPM..=MAX_MANUAL_RPM)
                    .step_by(RPM_STEP),
            );
            if response.dragged() || response.has_focus() {
                clamp_rpm(auto_fan_max_rpm);
                action = Some(FanAction::AutoMaxRpmDragging(*auto_fan_max_rpm));
            } else if response.drag_stopped() || response.lost_focus() {
                clamp_rpm(auto_fan_max_rpm);
                action = Some(FanAction::SetAutoFanMaxRpm(*auto_fan_max_rpm));
            }
        });
    });

    action
}

fn clamp_rpm(rpm: &mut u16) {
    *rpm = (*rpm).clamp(MIN_MANUAL_RPM, MAX_MANUAL_RPM);
    *rpm = (*rpm / RPM_STEP as u16) * RPM_STEP as u16;
    *rpm = (*rpm).clamp(MIN_MANUAL_RPM, MAX_MANUAL_RPM);
}

fn render_manual_fan_controls(ui: &mut egui::Ui, manual_fan_rpm: &mut u16) -> Option<FanAction> {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new("RPM:").selectable(false));
        let fan_response = ui.add(
            egui::Slider::new(manual_fan_rpm, MIN_MANUAL_RPM..=MAX_MANUAL_RPM).step_by(RPM_STEP),
        );

        if fan_response.dragged() || fan_response.has_focus() {
            Some(FanAction::SliderDragging(*manual_fan_rpm))
        } else if fan_response.drag_stopped() || fan_response.lost_focus() {
            Some(FanAction::SetManualRpm(*manual_fan_rpm))
        } else {
            None
        }
    })
    .inner
}

fn render_current_status(
    ui: &mut egui::Ui,
    fan_speed: &str,
    auto_fan_limit_enabled: bool,
    auto_fan_max_rpm: u16,
    cpu_avg_temp_c: Option<f32>,
    gpu_avg_temp_c: Option<f32>,
) {
    let status = if fan_speed.eq_ignore_ascii_case("auto") && auto_fan_limit_enabled {
        format!("Auto (max {} RPM)", auto_fan_max_rpm)
    } else {
        fan_speed.to_string()
    };

    ui.horizontal(|ui| {
        ui.add(
            egui::Label::new(format!("Current: {}", status)).selectable(false),
        );

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(
                egui::Label::new(temp_rich_text("GPU", gpu_avg_temp_c)).selectable(false),
            );
            ui.add(egui::Label::new(RichText::new("|").weak()).selectable(false));
            ui.add(
                egui::Label::new(temp_rich_text("CPU", cpu_avg_temp_c)).selectable(false),
            );
        });
    });
}

fn calculate_rpm_color(actual_rpm: u16) -> Color32 {
    let normalized_rpm = ((actual_rpm as f32 - MIN_RPM_FOR_COLOR)
        / (MAX_RPM_FOR_COLOR - MIN_RPM_FOR_COLOR))
        .clamp(0.0, 1.0);
    let green_component = ((1.0 - normalized_rpm) * DARK_GREEN_MAX as f32) as u8;
    let red_component = (normalized_rpm * 255.0) as u8;
    let orange_component = (normalized_rpm * 165.0) as u8;

    Color32::from_rgb(red_component, green_component, orange_component.min(ORANGE_MAX))
}
