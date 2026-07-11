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
        Self { mode: None, brightness: None, apply_color: false, slider_active: None }
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
                let (changed, active) = super::brightness_slider::brightness_step_slider(
                    ui,
                    "Brightness:",
                    temp_brightness_step,
                    BRIGHTNESS_LEVELS.len(),
                );
                if active {
                    action.slider_active = Some(true);
                } else if changed {
                    action.slider_active = Some(false);
                    action.brightness = Some(BRIGHTNESS_LEVELS[*temp_brightness_step]);
                }
            });
        }
    });

    action
}
