use eframe::egui::{self, RichText};
use librazer::types::BatteryCare;

#[derive(Debug, Clone, PartialEq)]
pub enum BatteryAction {
    None,
    SetBatteryCare(BatteryCare),
}

pub fn render_battery_section(ui: &mut egui::Ui, battery_care: &mut BatteryCare) -> BatteryAction {
    let mut action = BatteryAction::None;

    ui.group(|ui| {
        ui.add(egui::Label::new("🔋 Battery").selectable(false));
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Battery Health Optimizer:");
            let selected = *battery_care;
            egui::ComboBox::from_id_salt("battery_care_level")
                .selected_text(selected.label())
                .show_ui(ui, |ui| {
                    for level in BatteryCare::LEVELS {
                        if ui.selectable_value(battery_care, *level, level.label()).clicked() {
                            action = BatteryAction::SetBatteryCare(*level);
                        }
                    }
                });
        });

        render_battery_status(ui, *battery_care);
    });

    action
}

fn render_battery_status(ui: &mut egui::Ui, battery_care: BatteryCare) {
    ui.horizontal(|ui| {
        let status_text = if battery_care == BatteryCare::Disable {
            "Disabled — charges to 100%".to_string()
        } else {
            format!("Active — charge limit {}%", battery_care.to_percent())
        };
        ui.add(egui::Label::new(RichText::new(status_text)).selectable(false));
    });
}
