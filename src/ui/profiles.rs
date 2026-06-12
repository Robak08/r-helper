use eframe::egui;
use librazer::types::PerfMode;

#[derive(Debug, Clone, PartialEq)]
pub enum ProfileAction {
    None,
    ToggleAutoSwitch(bool),
    SaveAcProfile,
    SaveBatteryProfile,
}

pub fn render_profiles_section(
    ui: &mut egui::Ui,
    auto_switch_enabled: bool,
    ac_perf_mode: PerfMode,
    battery_perf_mode: PerfMode,
    no_device: bool,
) -> ProfileAction {
    let mut action = ProfileAction::None;

    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label("💾 Saved Settings");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut enabled = auto_switch_enabled;
                if ui
                    .add_enabled(!no_device, egui::Checkbox::new(&mut enabled, "Auto-switch"))
                    .on_hover_text("Apply saved settings when AC is plugged or unplugged")
                    .changed()
                {
                    action = ProfileAction::ToggleAutoSwitch(enabled);
                }
            });
        });

        ui.label(
            egui::RichText::new(
                "Save performance, fan, lighting, and battery settings per power source.",
            )
            .small()
            .weak(),
        );

        ui.add_space(4.0);

        ui.horizontal(|ui| {
            let ac_label = format!("AC: {:?}", ac_perf_mode);
            let battery_label = format!("Battery: {:?}", battery_perf_mode);
            ui.label(egui::RichText::new(ac_label).small());
            ui.separator();
            ui.label(egui::RichText::new(battery_label).small());
        });

        ui.add_space(4.0);

        ui.horizontal(|ui| {
            if ui
                .add_enabled(!no_device, egui::Button::new("Save current as AC"))
                .on_hover_text(
                    "Store current device settings (perf, fan, lighting, battery) for AC power",
                )
                .clicked()
            {
                action = ProfileAction::SaveAcProfile;
            }
            if ui
                .add_enabled(!no_device, egui::Button::new("Save current as Battery"))
                .on_hover_text(
                    "Store current device settings (perf, fan, lighting, battery) for battery power",
                )
                .clicked()
            {
                action = ProfileAction::SaveBatteryProfile;
            }
        });

        ui.add_space(4.0);
    });

    action
}
