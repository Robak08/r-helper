use eframe::egui;

/// Shared keyboard/cooling-pad brightness step slider.
pub fn brightness_step_slider(
    ui: &mut egui::Ui,
    label: &str,
    step: &mut usize,
    level_count: usize,
) -> (bool, bool) {
    let mut changed = false;
    let mut active = false;

    ui.horizontal(|ui| {
        ui.label(label);
        let mut step_f = *step as f64;
        let response = ui.add(
            egui::Slider::new(&mut step_f, 0.0..=(level_count.saturating_sub(1)) as f64)
                .step_by(1.0)
                .integer(),
        );
        active = response.dragged() || response.has_focus();
        let new_step = step_f.round() as usize;
        if new_step != *step {
            *step = new_step;
            changed = true;
        }
    });

    (changed, active)
}
