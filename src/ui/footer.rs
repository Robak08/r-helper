use eframe::egui::{self, Align, Layout, RichText};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
use std::process::Command;

/// Renders the application footer with version info and controls
pub fn render_footer(
    ui: &mut egui::Ui,
    status_messages: &mut bool,
    minimize_to_tray: &mut bool,
    run_at_startup: &mut bool,
) {
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        render_version_info(ui);
        ui.separator();
        render_status_toggle(ui, status_messages);
        ui.separator();
        ui.checkbox(minimize_to_tray, "Minimize to tray");
        ui.checkbox(run_at_startup, "Run at startup");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.button("🌐 GitHub").clicked() {
                let _ = Command::new("cmd")
                    .args(&["/c", "start", "https://github.com/Fatalution/r-helper"])
                    .spawn();
            }
        });
    });

    ui.add_space(8.0);
}

/// Renders the application version information
fn render_version_info(ui: &mut egui::Ui) {
    let text = format!("{} • Made with ♥ by Fatalution", APP_VERSION);
    let label = egui::Label::new(RichText::new(text)).selectable(false).sense(egui::Sense::click());
    if ui.add(label).clicked() {
        let _ =
            Command::new("cmd").args(&["/c", "start", "https://paypal.me/fatalutionDE"]).spawn();
    }
}

/// Renders the status messages toggle
fn render_status_toggle(ui: &mut egui::Ui, status_messages: &mut bool) {
    ui.checkbox(status_messages, "🐛 Debug");
}
