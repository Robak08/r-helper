use eframe::egui;

use super::lighting::BRIGHTNESS_LEVELS;

#[derive(Debug, Clone, PartialEq)]
pub struct CoolingPadLightingAction {
    pub mode: Option<String>,
    pub brightness: Option<u8>,
    pub apply_color: bool,
    pub slider_active: Option<bool>,
}

impl Default for CoolingPadLightingAction {
    fn default() -> Self {
        Self {
            mode: None,
            brightness: None,
            apply_color: false,
            slider_active: None,
        }
    }
}

pub fn render_cooling_pad_lighting_section(
    ui: &mut egui::Ui,
    mode: &str,
    temp_brightness_step: &mut usize,
    pad_color: &mut [u8; 3],
) -> CoolingPadLightingAction {
    let mut action = CoolingPadLightingAction::default();

    ui.group(|ui| {
        ui.add(egui::Label::new("💡 Cooling Pad Lighting").selectable(false));
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Mode:");
            for option in ["Static", "Breathing", "Off"] {
                let selected = mode == option;
                if ui.selectable_label(selected, option).clicked() && !selected {
                    action.mode = Some(option.to_string());
                }
            }
        });

        if mode != "Off" {
            ui.horizontal(|ui| {
                ui.label("Color:");
                let mut rgba = [pad_color[0], pad_color[1], pad_color[2], 255];
                ui.color_edit_button_srgba_unmultiplied(&mut rgba);
                pad_color[0] = rgba[0];
                pad_color[1] = rgba[1];
                pad_color[2] = rgba[2];
                if ui.button("Apply").clicked() {
                    action.apply_color = true;
                }
            });

            ui.horizontal(|ui| {
                ui.label("Brightness:");
                *temp_brightness_step = (*temp_brightness_step).min(BRIGHTNESS_LEVELS.len() - 1);
                let mut step_index = *temp_brightness_step;
                let response = ui.add(
                    egui::Slider::new(&mut step_index, 0..=(BRIGHTNESS_LEVELS.len() - 1))
                        .custom_formatter(|val, _| format!("{}", val as usize))
                        .custom_parser(|s| s.parse::<f64>().ok()),
                );

                let value_changed = step_index != *temp_brightness_step;
                *temp_brightness_step = step_index;

                if brightness_changed(&response, value_changed) {
                    action.brightness = Some(BRIGHTNESS_LEVELS[*temp_brightness_step]);
                }

                if response.dragged() || response.has_focus() {
                    action.slider_active = Some(true);
                } else if response.drag_stopped() || response.lost_focus() {
                    action.slider_active = Some(false);
                }
            });
        }
    });

    action
}

fn brightness_changed(response: &egui::Response, value_changed: bool) -> bool {
    if value_changed {
        return true;
    }
    response.dragged() || response.has_focus() || response.drag_stopped() || response.lost_focus()
}
