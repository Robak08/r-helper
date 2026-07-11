use crate::messaging::{MessageManager, MessageType};
use crate::system::SystemSpecs;
use eframe::egui::{self, Align, Color32, Layout, RichText};

const FADE_START_TIME: f32 = 3.0;
const FADE_DURATION: f32 = 2.0;
const FULL_ALPHA: u8 = 255;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppTab {
    Laptop,
    CoolingPad,
    Info,
}

/// Tab bar action from the header row.
#[derive(Debug, Clone, Copy, Default)]
pub struct TabBarAction {
    pub selected_tab: Option<AppTab>,
}

/// Renders the application header with device name and tabs or init status.
pub fn render_header(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    system_specs: &SystemSpecs,
    device_connected: bool,
    message_manager: &MessageManager,
    detecting_device: bool,
    fully_initialized: bool,
    active_tab: AppTab,
    show_cooling_pad_tab: bool,
) -> TabBarAction {
    let mut tab_action = TabBarAction::default();

    ui.horizontal(|ui| {
        render_device_name(ui, device_connected, system_specs);

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if fully_initialized {
                tab_action = render_tab_bar(ui, active_tab, show_cooling_pad_tab);
            } else {
                render_init_status(ui, ctx, device_connected, detecting_device);
            }
        });
    });

    if fully_initialized {
        render_toast_message(ui, ctx, message_manager);
    }

    tab_action
}

fn render_tab_bar(
    ui: &mut egui::Ui,
    active_tab: AppTab,
    show_cooling_pad_tab: bool,
) -> TabBarAction {
    let mut action = TabBarAction::default();

    ui.horizontal(|ui| {
        if ui.selectable_label(active_tab == AppTab::Laptop, "Laptop").clicked() {
            action.selected_tab = Some(AppTab::Laptop);
        }
        if show_cooling_pad_tab
            && ui.selectable_label(active_tab == AppTab::CoolingPad, "Cooling Pad").clicked()
        {
            action.selected_tab = Some(AppTab::CoolingPad);
        }
        if ui.selectable_label(active_tab == AppTab::Info, "Info").clicked() {
            action.selected_tab = Some(AppTab::Info);
        }
    });

    action
}

fn render_init_status(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    device_connected: bool,
    detecting_device: bool,
) {
    if !device_connected {
        if detecting_device {
            ui.add(
                egui::Label::new(RichText::new("🔎 Detecting device…").color(Color32::LIGHT_BLUE))
                    .selectable(false),
            );
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        } else {
            ui.add(
                egui::Label::new(RichText::new("❌ No device detected").color(Color32::RED))
                    .selectable(false),
            );
        }
    } else {
        ui.add(
            egui::Label::new(RichText::new("Initializing…").color(Color32::LIGHT_BLUE))
                .selectable(false),
        );
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

fn render_toast_message(ui: &mut egui::Ui, ctx: &egui::Context, message_manager: &MessageManager) {
    let Some(current_message) = message_manager.get_current_message() else {
        return;
    };

    let elapsed = current_message.age_seconds();
    let (base_color, icon) = get_message_style_from_type(&current_message.message_type);
    let alpha = calculate_fade_alpha(elapsed);
    let faded_color = apply_alpha_to_color(base_color, alpha);

    ui.add(
        egui::Label::new(
            RichText::new(format!("{} {}", icon, current_message.content)).color(faded_color),
        )
        .selectable(false),
    );

    if current_message.should_fade() {
        ctx.request_repaint();
    }
}

/// Renders device name section
fn render_device_name(ui: &mut egui::Ui, device_connected: bool, system_specs: &SystemSpecs) {
    let device_text = if device_connected || system_specs.device_model != "Unknown" {
        if system_specs.device_model != "Unknown" {
            format!("💻 {}", system_specs.device_model)
        } else {
            "💻 Connected Device".to_string()
        }
    } else {
        "💻 No Razer Device".to_string()
    };

    ui.add(egui::Label::new(egui::RichText::new(device_text).heading()).selectable(false));
}

/// Message style based on type
fn get_message_style_from_type(message_type: &MessageType) -> (Color32, &'static str) {
    match message_type {
        MessageType::Info => (Color32::LIGHT_BLUE, "ℹ"),
        MessageType::Error => (Color32::RED, "⚠"),
    }
}

/// Calculates alpha value for fade animation
fn calculate_fade_alpha(elapsed: f32) -> f32 {
    if elapsed < FADE_START_TIME {
        1.0
    } else {
        let fade_progress = (elapsed - FADE_START_TIME) / FADE_DURATION;
        (1.0 - fade_progress).max(0.0)
    }
}

/// Applies alpha transparency to color
fn apply_alpha_to_color(base_color: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        base_color.r(),
        base_color.g(),
        base_color.b(),
        (FULL_ALPHA as f32 * alpha) as u8,
    )
}
