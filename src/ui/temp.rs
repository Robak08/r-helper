use eframe::egui::{self, Color32, RichText};

const COOL_MAX_C: f32 = 68.0;
const WARM_MAX_C: f32 = 82.0;

pub fn format_temp_c(value: Option<f32>) -> String {
    match value {
        Some(c) => format!("{:.0} °C", c),
        None => "N/A".to_string(),
    }
}

pub fn format_temp_label(prefix: &str, value: Option<f32>) -> String {
    match value {
        Some(c) => format!("{prefix} {:.0} °C", c),
        None => format!("{prefix} —"),
    }
}

pub fn temp_color(value: Option<f32>) -> Color32 {
    match value {
        Some(c) if c < COOL_MAX_C => Color32::from_rgb(40, DARK_GREEN_MAX, 40),
        Some(c) if c < WARM_MAX_C => Color32::from_rgb(220, 140, 20),
        Some(_) => Color32::from_rgb(220, 60, 40),
        None => Color32::GRAY,
    }
}

pub fn temp_rich_text(prefix: &str, value: Option<f32>) -> RichText {
    RichText::new(format_temp_label(prefix, value)).color(temp_color(value))
}

const DARK_GREEN_MAX: u8 = 120;

pub fn render_temp_pair(ui: &mut egui::Ui, cpu_temp_c: Option<f32>, gpu_temp_c: Option<f32>) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(temp_rich_text("CPU", cpu_temp_c)).selectable(false));
        ui.add_space(12.0);
        ui.add(egui::Label::new(temp_rich_text("GPU", gpu_temp_c)).selectable(false));
    });
}

pub fn info_temp_row(ui: &mut egui::Ui, label: &str, value: Option<f32>) {
    ui.horizontal(|ui| {
        ui.add(
            egui::Label::new(RichText::new(format!("{label}:")).weak()).selectable(false),
        );
        ui.add(
            egui::Label::new(
                RichText::new(format_temp_c(value)).color(temp_color(value)),
            )
            .selectable(false),
        );
    });
}
