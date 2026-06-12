#![windows_subsystem = "windows"]

mod config;
mod device;
mod messaging;
mod power;
mod startup;
mod system;
mod tray;
mod ui;
mod utils;

use eframe::egui;
use egui::IconData;

use anyhow::Result;
use std::sync::{mpsc, Arc};

use librazer::types::{
    BatteryCare, CpuBoost, FanMode, GpuBoost, LightsAlwaysOn, LogoMode, MaxFanSpeedMode, PerfMode,
};
use librazer::{command, device::Device};
use strum::IntoEnumIterator;

use device::CompleteDeviceState;
use messaging::{error_message, status_message, MessageManager};
use power::get_power_state;
use system::{get_system_specs, SystemSpecs};
use utils::{execute_device_command_simple, DeviceStateReader};

// Dynamic app metadata from Cargo
const APP_NAME: &str = "R-Helper";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTO_FAN_HYSTERESIS: u16 = 300;

#[derive(Debug, Clone)]
enum InitMessage {
    SystemSpecsComplete(SystemSpecs),
    PowerStateRead(bool),
    InitializationComplete,
    DeviceDetectionComplete(bool),
}

#[derive(Debug, Clone)]
struct DeviceStatus {
    performance_mode: String,
    fan_speed: String,
    fan_rpm: Option<u16>,
    fan_actual_rpm: Option<u16>,
    logo_mode: String,
    keyboard_brightness: u8,
    lights_always_on: bool,
    battery_care: BatteryCare,
}

impl Default for DeviceStatus {
    fn default() -> Self {
        Self {
            performance_mode: "Reading...".to_string(),
            fan_speed: "Reading...".to_string(),
            fan_rpm: None,
            fan_actual_rpm: None,
            logo_mode: "Reading...".to_string(),
            keyboard_brightness: 0,
            lights_always_on: false,
            battery_care: BatteryCare::Percent80,
        }
    }
}

struct RazerGuiApp {
    status: DeviceStatus,
    device: Option<Device>,
    device_state: Option<CompleteDeviceState>,
    system_specs: SystemSpecs,
    available_performance_modes: Vec<PerfMode>,
    base_performance_modes: Vec<PerfMode>,

    ac_power: bool,
    ac_profile: CompleteDeviceState,
    battery_profile: CompleteDeviceState,
    auto_switch_enabled: bool,

    loading: bool,
    fully_initialized: bool,
    init_receiver: Option<mpsc::Receiver<InitMessage>>,
    message_manager: MessageManager,
    last_refresh_time: std::time::Instant,
    last_state_check_time: std::time::Instant,
    last_fan_enforce_time: std::time::Instant,
    status_messages: bool,

    manual_fan_rpm: u16,
    auto_fan_limit_enabled: bool,
    auto_fan_max_rpm: u16,
    auto_fan_cap_override: bool,
    auto_fan_max_rpm_editing: bool,
    temp_brightness_step: usize,
    brightness_slider_active: bool,
    should_quit: bool,
    tray_state: Option<Arc<tray::TraySharedState>>,
    #[allow(dead_code)]
    _tray_guard: Option<tray::TrayHandle>,
    minimize_to_tray: bool,
    run_at_startup: bool,

    init_power_read: bool,
    init_specs_complete: bool,
    last_perf_poll_time: std::time::Instant,
    cpu_boost: CpuBoost,
    gpu_boost: GpuBoost,
    base_window_height: f32,
    expanded_window_height: Option<f32>,
    custom_controls_visible_last: bool,
    // Device detection state
    detecting_device: bool,
    device_detection_done: bool,
    min_detecting_until: std::time::Instant,
}

impl RazerGuiApp {
    fn perf_mode_to_string(mode: PerfMode) -> String {
        format!("{:?}", mode)
    }

    fn string_to_perf_mode(mode: &str) -> Option<PerfMode> {
        PerfMode::iter().find(|m| format!("{:?}", m) == mode)
    }

    fn logo_mode_to_string(mode: LogoMode) -> &'static str {
        match mode {
            LogoMode::Static => "Static",
            LogoMode::Breathing => "Breathing",
            LogoMode::Off => "Off",
        }
    }

    fn string_to_logo_mode(mode: &str) -> Option<LogoMode> {
        match mode {
            "Static" => Some(LogoMode::Static),
            "Breathing" => Some(LogoMode::Breathing),
            "Off" => Some(LogoMode::Off),
            _ => None,
        }
    }

    fn is_user_auto_mode(&self) -> bool {
        self.status.fan_speed.eq_ignore_ascii_case("auto")
    }

    fn apply_device_fan_status(&mut self, fan_mode: FanMode, set_rpm: Option<u16>) {
        if self.is_user_auto_mode() {
            return;
        }
        let (fan_speed, fan_rpm) = match fan_mode {
            FanMode::Auto => ("Auto".to_string(), None),
            FanMode::Manual => ("Manual".to_string(), set_rpm),
        };
        self.status.fan_speed = fan_speed;
        self.status.fan_rpm = fan_rpm;
        if let Some(rpm) = fan_rpm {
            self.manual_fan_rpm = rpm;
        }
    }

    fn read_current_fan_state(device: &Device) -> (FanMode, Option<u16>) {
        // Read the current fan mode from the combined perf/fan query.
        // (We intentionally avoid a second immediate retry; caller logic tolerates fallback to Auto.)
        let fan_mode = command::get_perf_mode(device).map(|(_, fm)| fm).unwrap_or_else(|_| {
            eprintln!("Warning: Failed to read device fan mode, assuming Auto");
            FanMode::Auto
        });
        let set_rpm = get_fan_rpm_set(device, librazer::types::FanZone::Zone1);
        (fan_mode, set_rpm)
    }

    fn get_fan_status_from_mode(fan_mode: FanMode, device: &Device) -> (String, Option<u16>) {
        match fan_mode {
            FanMode::Auto => ("Auto".to_string(), None),
            FanMode::Manual => {
                let set_rpm = get_fan_rpm_set(device, librazer::types::FanZone::Zone1);
                ("Manual".to_string(), set_rpm)
            }
        }
    }

    fn set_no_device_message(&mut self) {
        self.set_status_message("No device connected".to_string());
    }

    fn new() -> Self {
        let app_config = config::AppConfig::load();
        let ac_profile = app_config.ac_profile;
        let battery_profile = app_config.battery_profile;
        let auto_switch_enabled = app_config.auto_switch_enabled;
        let auto_fan_limit_enabled = app_config.auto_fan_limit_enabled;
        let auto_fan_max_rpm = app_config.auto_fan_max_rpm;

        let (init_sender, init_receiver) = mpsc::channel();

        let now = std::time::Instant::now();
        let mut app = Self {
            status: DeviceStatus::default(),
            device: None,
            device_state: None,
            system_specs: SystemSpecs::default(),
            available_performance_modes: Vec::new(),
            base_performance_modes: Vec::new(),
            ac_power: true,
            ac_profile,
            battery_profile,
            auto_switch_enabled,
            loading: true,
            fully_initialized: false,
            init_receiver: Some(init_receiver),
            message_manager: MessageManager::new(),
            last_refresh_time: std::time::Instant::now(),
            last_state_check_time: std::time::Instant::now(),
            last_fan_enforce_time: std::time::Instant::now(),
            status_messages: false,

            manual_fan_rpm: 2000,
            auto_fan_limit_enabled,
            auto_fan_max_rpm,
            auto_fan_cap_override: false,
            auto_fan_max_rpm_editing: false,
            temp_brightness_step: 0,
            brightness_slider_active: false,

            should_quit: false,
            tray_state: None,
            _tray_guard: None,
            minimize_to_tray: true,
            run_at_startup: startup::is_startup_enabled(),

            init_power_read: false,
            init_specs_complete: false,
            last_perf_poll_time: std::time::Instant::now(),
            cpu_boost: CpuBoost::Low,
            gpu_boost: GpuBoost::Low,
            base_window_height: 0.0,
            expanded_window_height: None,
            custom_controls_visible_last: false,
            detecting_device: true,
            device_detection_done: false,
            min_detecting_until: now + std::time::Duration::from_secs(1),
        };

        // Kick off async device detection so the UI can show a clear “Detecting device…” state.
        app.start_device_detection(init_sender.clone());

        // Start other background initialization (power state, system specs)
        app.start_background_initialization(init_sender);

        app
    }

    fn start_device_detection(&mut self, sender: mpsc::Sender<InitMessage>) {
        self.detecting_device = true;
        std::thread::spawn(move || {
            let present = match Device::detect() {
                Ok(_dev) => true,
                Err(e) => {
                    eprintln!("Failed to connect to Razer device: {}", e);
                    false
                }
            };
            let _ = sender.send(InitMessage::DeviceDetectionComplete(present));
        });
    }

    fn detect_available_performance_modes(&mut self) {
        // Prefer firmware-advertised list; fallback to full enum when unknown.
        if let Some(ref device) = self.device {
            if let Some(ref list) = device.info().perf_modes {
                self.available_performance_modes = list.clone();
                if self.base_performance_modes.is_empty() {
                    self.base_performance_modes = self.available_performance_modes.clone();
                }
                return;
            }
        }
        self.available_performance_modes = PerfMode::iter().collect();
        if self.base_performance_modes.is_empty() {
            self.base_performance_modes = self.available_performance_modes.clone();
        }
    }

    fn get_descriptor_allowed_boosts(
        &self,
    ) -> (Vec<CpuBoost>, Vec<GpuBoost>, Vec<(CpuBoost, GpuBoost)>) {
        if let Some(ref device) = self.device {
            let d = device.info();
            let cpus: Vec<CpuBoost> = d.cpu_boosts.clone().unwrap_or_else(|| {
                vec![CpuBoost::Low, CpuBoost::Medium, CpuBoost::High, CpuBoost::Boost]
            });
            let gpus: Vec<GpuBoost> = d
                .gpu_boosts
                .clone()
                .unwrap_or_else(|| vec![GpuBoost::Low, GpuBoost::Medium, GpuBoost::High]);
            let pairs: Vec<(CpuBoost, GpuBoost)> = d.disallowed_boost_pairs.clone();
            (cpus, gpus, pairs)
        } else {
            (
                vec![CpuBoost::Low, CpuBoost::Medium, CpuBoost::High, CpuBoost::Boost],
                vec![GpuBoost::Low, GpuBoost::Medium, GpuBoost::High],
                Vec::new(),
            )
        }
    }

    fn read_initial_device_state(&mut self) {
        if let Some(ref device) = self.device {
            let mut reader = DeviceStateReader::new(device);
            // Use batched reader helper to gather as much as possible without early abort.

            if let Some(brightness) =
                reader.read(|d| command::get_keyboard_brightness(d), "keyboard brightness")
            {
                self.status.keyboard_brightness = brightness;
                self.temp_brightness_step = ui::lighting::raw_brightness_to_step_index(brightness);
            }

            if let Some((perf_mode, fan_mode)) =
                reader.read(|d| command::get_perf_mode(d), "performance mode")
            {
                self.status.performance_mode = Self::perf_mode_to_string(perf_mode).to_string();

                let (fan_speed, fan_rpm) = Self::get_fan_status_from_mode(fan_mode, device);
                self.status.fan_speed = fan_speed;
                self.status.fan_rpm = fan_rpm;

                if let Some(rpm) = fan_rpm {
                    self.manual_fan_rpm = rpm;
                }

                if matches!(perf_mode, PerfMode::Custom) {
                    if let Ok(v) = command::get_cpu_boost(device) {
                        self.cpu_boost = v;
                    }
                    if let Ok(v) = command::get_gpu_boost(device) {
                        self.gpu_boost = v;
                    }
                }
            }

            if self.status.fan_speed == "Reading..." {
                // Fallback: if earlier combined call failed but later succeeds, fill fan info.
                if let Ok((_, fan_mode)) = command::get_perf_mode(device) {
                    let (fan_speed, fan_rpm) = Self::get_fan_status_from_mode(fan_mode, device);
                    self.status.fan_speed = fan_speed;
                    self.status.fan_rpm = fan_rpm;

                    if let Some(rpm) = fan_rpm {
                        self.manual_fan_rpm = rpm;
                    }
                }
            }

            if let Some(lights_always_on) =
                reader.read(|d| command::get_lights_always_on(d), "lights always on")
            {
                self.status.lights_always_on = matches!(lights_always_on, LightsAlwaysOn::Enable);
            }

            if let Some(battery_care) =
                reader.read(|d| command::get_battery_care(d), "battery care")
            {
                self.status.battery_care = battery_care;
            }

            let errors = reader.finish();
            if !errors.is_empty() && cfg!(debug_assertions) {
                eprintln!("Device state reading errors: {:?}", errors);
            }
        }
    }

    fn start_background_initialization(&mut self, sender: mpsc::Sender<InitMessage>) {
        // Device may not be known yet; pass None here. We'll still display specs we can read.
        let device_name: Option<String> = None;

        std::thread::spawn(move || {
            if let Ok(ac_power) = get_power_state() {
                let _ = sender.send(InitMessage::PowerStateRead(ac_power));
            }

            let _ = sender.send(InitMessage::InitializationComplete);

            let device_name_ref = device_name.as_deref();
            let system_specs = get_system_specs(device_name_ref);
            let _ = sender.send(InitMessage::SystemSpecsComplete(system_specs));
        });

        self.loading = false;
    }

    fn process_background_initialization(&mut self) {
        let mut messages_to_process = Vec::new();
        // Drain all pending init messages this frame (non-blocking).

        if let Some(ref receiver) = self.init_receiver {
            while let Ok(message) = receiver.try_recv() {
                messages_to_process.push(message);
            }
        }

        for message in messages_to_process {
            match message {
                InitMessage::DeviceDetectionComplete(present) => {
                    self.device_detection_done = true;
                    // If a device is found, switch immediately. If not, keep detecting until grace expires.
                    if present {
                        self.detecting_device = false;
                    }
                    if present {
                        // Acquire the device on the UI thread.
                        if let Ok(dev) = Device::detect() {
                            let display_name = dev.info().display_name.clone();
                            self.device = Some(dev);
                            let specs = get_system_specs(Some(&display_name));
                            self.system_specs.device_model = specs.device_model;
                        }
                    }
                    self.detect_available_performance_modes();
                    if self.device.is_some() {
                        self.read_initial_device_state();
                        // Now that the device is known, we can show a brief init message.
                        self.set_status_message("Initializing...".to_string());
                    }
                }
                InitMessage::SystemSpecsComplete(mut specs) => {
                    if let Some(ref device) = self.device {
                        specs.device_model =
                            get_system_specs(Some(&device.info().display_name)).device_model;
                    }
                    self.system_specs = specs;
                    self.init_specs_complete = true;
                    if self.fully_initialized && self.init_power_read && self.init_specs_complete {
                        // Avoid flashing a transient message on unsupported devices.
                        if self.device.is_some() {
                            self.set_status_message("Initialization complete".to_string());
                        } else {
                            self.set_optional_status_message("Initialization complete".to_string());
                        }
                    } else {
                        self.set_optional_status_message(
                            "System specifications loaded".to_string(),
                        );
                    }
                }
                InitMessage::PowerStateRead(ac_power) => {
                    self.ac_power = ac_power;
                    self.init_power_read = true;
                }
                InitMessage::InitializationComplete => {
                    self.fully_initialized = true;
                    if self.device.is_some() {
                        if let Err(e) = self.read_device_status() {
                            self.set_error_message(format!("Failed to read device status: {}", e));
                        } else {
                            self.update_stored_device_state();
                            self.sync_ui_with_device_state();
                            self.init_fan_slider_from_device();
                        }
                    }
                }
            }
        }
    }
}

fn get_fan_rpm_actual(device: &Device, zone: librazer::types::FanZone) -> Option<u16> {
    match command::get_fan_actual_rpm(device, zone) {
        Ok(rpm) => Some(rpm),
        Err(_) => None,
    }
}

fn get_fan_rpm_set(device: &Device, zone: librazer::types::FanZone) -> Option<u16> {
    match command::get_fan_rpm(device, zone) {
        Ok(rpm) => Some(rpm),
        Err(_) => None,
    }
}

impl RazerGuiApp {
    fn read_device_status(&mut self) -> Result<()> {
        let (perf_mode, fan_mode, set_rpm, fan_actual_rpm, logo_mode, brightness, lights_always_on, battery_care) = {
            let device = self.device.as_ref().unwrap();
            let (perf_mode, fan_mode) = command::get_perf_mode(device)?;
            let set_rpm = get_fan_rpm_set(device, librazer::types::FanZone::Zone1);
            let fan_actual_rpm = get_fan_rpm_actual(device, librazer::types::FanZone::Zone1);
            let logo_mode = command::get_logo_mode(device).ok();
            let brightness = command::get_keyboard_brightness(device).ok();
            let lights_always_on = command::get_lights_always_on(device).ok();
            let battery_care = command::get_battery_care(device).ok();
            (
                perf_mode,
                fan_mode,
                set_rpm,
                fan_actual_rpm,
                logo_mode,
                brightness,
                lights_always_on,
                battery_care,
            )
        };

        self.status.performance_mode = Self::perf_mode_to_string(perf_mode).to_string();
        self.apply_device_fan_status(fan_mode, set_rpm);
        self.status.fan_actual_rpm = fan_actual_rpm;
        if let Some(logo_mode) = logo_mode {
            self.status.logo_mode = Self::logo_mode_to_string(logo_mode).to_string();
        }
        if let Some(brightness) = brightness {
            self.status.keyboard_brightness = brightness;
            self.temp_brightness_step = ui::lighting::raw_brightness_to_step_index(brightness);
        }
        if let Some(lights_always_on) = lights_always_on {
            self.status.lights_always_on = matches!(lights_always_on, LightsAlwaysOn::Enable);
        }
        if let Some(battery_care) = battery_care {
            self.status.battery_care = battery_care;
        }

        Ok(())
    }

    fn sync_ui_with_device_state(&mut self) {
        if let Some(ref device) = self.device {
            // Refresh only fields that can drift externally (brightness skipped if user dragging slider).
            if !self.brightness_slider_active {
                if let Ok(brightness) = command::get_keyboard_brightness(device) {
                    self.status.keyboard_brightness = brightness;
                    self.temp_brightness_step =
                        ui::lighting::raw_brightness_to_step_index(brightness);
                }
            }
            let (fan_mode, set_rpm) = Self::read_current_fan_state(device);
            let (fan_speed, fan_rpm) = Self::get_fan_status_from_mode(fan_mode, device);
            self.status.fan_speed = fan_speed;
            self.status.fan_rpm = fan_rpm;
            if let Some(rpm) = set_rpm {
                self.manual_fan_rpm = rpm;
            }
            if let Ok(lights_always_on) = command::get_lights_always_on(device) {
                self.status.lights_always_on = matches!(lights_always_on, LightsAlwaysOn::Enable);
            }
            if let Ok(battery_care) = command::get_battery_care(device) {
                self.status.battery_care = battery_care;
            }
        }
    }

    fn sync_other_dynamic_state(&mut self) {
        if let Some(ref device) = self.device {
            // Light-weight periodic poll for simple toggles.
            if let Ok(lights_always_on) = command::get_lights_always_on(device) {
                self.status.lights_always_on = matches!(lights_always_on, LightsAlwaysOn::Enable);
            }
            if let Ok(battery_care) = command::get_battery_care(device) {
                self.status.battery_care = battery_care;
            }
        }
    }

    fn init_fan_slider_from_device(&mut self) {
        if let Some(ref device) = self.device {
            // Initializes manual fan slider to currently set RPM if in Manual.
            let (fan_mode, set_rpm) = Self::read_current_fan_state(device);
            let (fan_speed, fan_rpm) = Self::get_fan_status_from_mode(fan_mode, device);
            self.status.fan_speed = fan_speed;
            self.status.fan_rpm = fan_rpm;
            if let Some(rpm) = set_rpm {
                self.manual_fan_rpm = rpm;
            }
        }
    }

    fn check_device_state_changes(&mut self) -> Result<()> {
        if let Some(ref device) = self.device {
            // Full snapshot comparison to detect external changes.
            let current_state = CompleteDeviceState::read_from_device(device)?;

            if let Some(ref stored_state) = self.device_state {
                if current_state != *stored_state {
                    let old_perf_mode = Self::perf_mode_to_string(stored_state.perf_mode);
                    let new_perf_mode = Self::perf_mode_to_string(current_state.perf_mode);

                    self.device_state = Some(current_state.clone());

                    // Convert the low-level state to our UI format
                    self.status.performance_mode =
                        Self::perf_mode_to_string(current_state.perf_mode).to_string();

                    let (fan_speed, fan_rpm) =
                        Self::get_fan_status_from_mode(current_state.fan_mode, device);
                    if !self.is_user_auto_mode() {
                        self.status.fan_speed = fan_speed;
                        self.status.fan_rpm = fan_rpm;
                        if let Some(rpm) = fan_rpm {
                            self.manual_fan_rpm = rpm;
                        }
                    }

                    self.status.logo_mode =
                        Self::logo_mode_to_string(current_state.logo_mode).to_string();

                    self.status.keyboard_brightness = current_state.keyboard_brightness;
                    self.temp_brightness_step = ui::lighting::raw_brightness_to_step_index(
                        current_state.keyboard_brightness,
                    );

                    self.status.lights_always_on =
                        matches!(current_state.lights_always_on, LightsAlwaysOn::Enable);
                    self.status.battery_care = current_state.battery_care;

                    if old_perf_mode != new_perf_mode {
                        self.set_optional_status_message("Mode updated".to_string());
                    } else if self.status_messages {
                        self.set_optional_status_message(
                            "Device state updated externally".to_string(),
                        );
                    }
                }
            } else {
                self.device_state = Some(current_state);
            }
        }
        Ok(())
    }

    fn set_status_message(&mut self, message: String) {
        self.message_manager.add_message(status_message(message));
    }

    fn set_optional_status_message(&mut self, message: String) {
        if self.status_messages {
            self.message_manager.add_message(status_message(message));
        }
    }

    fn set_error_message(&mut self, message: String) {
        self.message_manager.add_message(error_message(message));
    }

    fn update_stored_device_state(&mut self) {
        if let Some(ref device) = self.device {
            if let Ok(current_state) = CompleteDeviceState::read_from_device(device) {
                self.device_state = Some(current_state);
            }
        }
    }

    fn auto_switch_profile(&mut self) {
        if !self.auto_switch_enabled {
            return;
        }

        let Some(ref device) = self.device else {
            return;
        };

        let target_profile =
            if self.ac_power { self.ac_profile.clone() } else { self.battery_profile.clone() };
        let profile_name = if self.ac_power { "AC" } else { "Battery" };

        if let Err(e) = target_profile.apply_to_device(device) {
            self.set_error_message(format!(
                "Failed to switch to {} profile: {}",
                profile_name, e
            ));
            return;
        }

        self.sync_ui_after_profile_apply(&target_profile);
        self.update_stored_device_state();
        self.set_status_message(format!("Auto-switched to {} settings", profile_name));
    }

    fn sync_ui_after_profile_apply(&mut self, profile: &CompleteDeviceState) {
        self.status.performance_mode = Self::perf_mode_to_string(profile.perf_mode).to_string();

        let (fan_speed, fan_rpm) = match profile.fan_mode {
            FanMode::Auto => ("Auto".to_string(), None),
            FanMode::Manual => ("Manual".to_string(), profile.fan_rpm),
        };
        self.status.fan_speed = fan_speed;
        self.status.fan_rpm = fan_rpm;
        if let Some(rpm) = profile.fan_rpm {
            self.manual_fan_rpm = rpm;
        }
        self.auto_fan_cap_override = false;

        self.status.logo_mode = Self::logo_mode_to_string(profile.logo_mode).to_string();
        self.status.keyboard_brightness = profile.keyboard_brightness;
        self.temp_brightness_step =
            ui::lighting::raw_brightness_to_step_index(profile.keyboard_brightness);
        self.status.lights_always_on =
            matches!(profile.lights_always_on, LightsAlwaysOn::Enable);
        self.status.battery_care = profile.battery_care;

        if let Some(cpu) = profile.cpu_boost {
            self.cpu_boost = cpu;
        }
        if let Some(gpu) = profile.gpu_boost {
            self.gpu_boost = gpu;
        }

        if let Some(ref device) = self.device {
            self.status.fan_actual_rpm =
                get_fan_rpm_actual(device, librazer::types::FanZone::Zone1);
        }
    }

    fn persist_config(&self) -> Result<()> {
        let config = config::AppConfig {
            version: 1,
            auto_switch_enabled: self.auto_switch_enabled,
            ac_profile: self.ac_profile.clone(),
            battery_profile: self.battery_profile.clone(),
            auto_fan_limit_enabled: self.auto_fan_limit_enabled,
            auto_fan_max_rpm: self.auto_fan_max_rpm,
        };
        config.save()
    }

    fn save_current_as_ac_profile(&mut self) {
        let Some(ref device) = self.device else {
            self.set_no_device_message();
            return;
        };

        match CompleteDeviceState::read_from_device(device) {
            Ok(profile) => {
                self.ac_profile = profile;
                match self.persist_config() {
                    Ok(_) => self.set_status_message("AC settings saved".into()),
                    Err(e) => self.set_error_message(format!("Failed to save config: {}", e)),
                }
            }
            Err(e) => self.set_error_message(format!("Failed to read device state: {}", e)),
        }
    }

    fn save_current_as_battery_profile(&mut self) {
        let Some(ref device) = self.device else {
            self.set_no_device_message();
            return;
        };

        match CompleteDeviceState::read_from_device(device) {
            Ok(profile) => {
                self.battery_profile = profile;
                match self.persist_config() {
                    Ok(_) => self.set_status_message("Battery settings saved".into()),
                    Err(e) => self.set_error_message(format!("Failed to save config: {}", e)),
                }
            }
            Err(e) => self.set_error_message(format!("Failed to read device state: {}", e)),
        }
    }

    fn render_profiles_section(&mut self, ui: &mut egui::Ui) {
        use ui::profiles::{render_profiles_section, ProfileAction};

        let action = render_profiles_section(
            ui,
            self.auto_switch_enabled,
            self.ac_profile.perf_mode,
            self.battery_profile.perf_mode,
            self.device.is_none(),
        );

        match action {
            ProfileAction::None => {}
            ProfileAction::ToggleAutoSwitch(enabled) => {
                self.auto_switch_enabled = enabled;
                if let Err(e) = self.persist_config() {
                    self.set_error_message(format!("Failed to save config: {}", e));
                    self.auto_switch_enabled = !enabled;
                } else {
                    self.set_optional_status_message(if enabled {
                        "Auto-switch enabled".into()
                    } else {
                        "Auto-switch disabled".into()
                    });
                }
            }
            ProfileAction::SaveAcProfile => self.save_current_as_ac_profile(),
            ProfileAction::SaveBatteryProfile => self.save_current_as_battery_profile(),
        }
    }

    fn set_performance_mode(&mut self, mode: &str) {
        let perf_mode = match Self::string_to_perf_mode(mode) {
            Some(m) => m,
            None => return,
        };

        let mut restore_manual = None::<u16>;
        let mut read_boosts = false;
        let mut set_mode_ok = false;
        let mut error_msg: Option<String> = None;

        if let Some(ref device) = self.device {
            let (current_fan_mode, set_rpm) = Self::read_current_fan_state(device);

            match command::set_perf_mode(device, perf_mode) {
                Ok(_) => {
                    set_mode_ok = true;
                    // Preserve manual fan RPM if user had manual mode before switching.
                    if matches!(current_fan_mode, FanMode::Manual) {
                        restore_manual = set_rpm;
                    }
                    // Only query boost states for Custom (other modes ignore those values).
                    if mode == "Custom" {
                        read_boosts = true;
                    }
                }
                Err(e) => {
                    error_msg = Some(format!("Failed to set performance mode: {}", e));
                }
            }

            if set_mode_ok {
                if let Some(rpm) = restore_manual {
                    // Short delays give firmware time to commit mode before restoring manual fan state.
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    if command::set_fan_mode(device, FanMode::Manual).is_ok() {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        if command::set_fan_rpm(device, rpm, true).is_err() {
                            error_msg = Some(
                                "Failed to restore fan RPM after performance mode change".into(),
                            );
                        } else {
                            restore_manual = Some(rpm);
                        }
                    } else {
                        error_msg = Some(
                            "Failed to restore manual fan mode after performance mode change"
                                .into(),
                        );
                    }
                }
                if read_boosts {
                    // Populate boost controls so UI reflects actual device values.
                    if let Ok(v) = command::get_cpu_boost(device) {
                        self.cpu_boost = v;
                    }
                    if let Ok(v) = command::get_gpu_boost(device) {
                        self.gpu_boost = v;
                    }
                }
            }
        } else {
            self.set_no_device_message();
            return;
        }

        if let Some(msg) = error_msg {
            self.set_error_message(msg);
        }
        if set_mode_ok {
            self.status.performance_mode = mode.to_string();
            if let Some(rpm) = restore_manual {
                self.status.fan_speed = "Manual".into();
                self.status.fan_rpm = Some(rpm);
                self.manual_fan_rpm = rpm;
            } else {
                self.auto_fan_cap_override = false;
                if self.is_user_auto_mode() {
                    self.status.fan_speed = "Auto".into();
                    self.status.fan_rpm = None;
                }
            }
            self.set_optional_status_message("Mode changed".into());
            self.update_stored_device_state();
        }
    }

    fn render_performance_section(&mut self, ui: &mut egui::Ui) {
        use ui::performance::{render_performance_section, PerformanceAction};
        let (mut allowed_cpu, mut allowed_gpu, disallowed_pairs) =
            self.get_descriptor_allowed_boosts();
        let base_cpu = allowed_cpu.clone();
        let base_gpu = allowed_gpu.clone();

        // If user toggled the eye icon (hidden modes view), also reveal all boost options.
        let showing_hidden =
            ui.ctx().data(|d| d.get_temp::<bool>("perf_hidden_show".into()).unwrap_or(false));
        if showing_hidden {
            // Full canonical sets (excluding debug-only Undervolt which stays debug gated)
            let full_cpu = [CpuBoost::Low, CpuBoost::Medium, CpuBoost::High, CpuBoost::Boost];
            let full_gpu = [GpuBoost::Low, GpuBoost::Medium, GpuBoost::High];
            for b in full_cpu {
                if !allowed_cpu.contains(&b) {
                    allowed_cpu.push(b);
                }
            }
            for b in full_gpu {
                if !allowed_gpu.contains(&b) {
                    allowed_gpu.push(b);
                }
            }
            // Maintain a stable displayed order
            let order_cpu = |b: &CpuBoost| match b {
                CpuBoost::Low => 0,
                CpuBoost::Medium => 1,
                CpuBoost::High => 2,
                CpuBoost::Boost => 3,
                CpuBoost::Undervolt => 4,
            };
            allowed_cpu.sort_by_key(order_cpu);
            let order_gpu = |b: &GpuBoost| match b {
                GpuBoost::Low => 0,
                GpuBoost::Medium => 1,
                GpuBoost::High => 2,
            };
            allowed_gpu.sort_by_key(order_gpu);
        }
        let action = render_performance_section(
            ui,
            &self.status.performance_mode,
            self.ac_power,
            &self.available_performance_modes,
            &self.base_performance_modes,
            self.status_messages, // debug flag reuse
            self.cpu_boost,
            self.gpu_boost,
            &allowed_cpu,
            &allowed_gpu,
            &disallowed_pairs,
            &base_cpu,
            &base_gpu,
            self.device.is_none(),
        );

        match action {
            PerformanceAction::None => {}
            PerformanceAction::SetPerformanceMode(mode) => {
                self.set_performance_mode(&mode);
            }
            PerformanceAction::ToggleHidden => {
                let current = ui
                    .ctx()
                    .data(|d| d.get_temp::<bool>("perf_hidden_show".into()).unwrap_or(false));
                ui.ctx().data_mut(|d| d.insert_temp("perf_hidden_show".into(), !current));
            }
            PerformanceAction::SetCpuBoost(boost) => {
                if self.status.performance_mode == "Custom" {
                    if let Some(ref device) = self.device {
                        if let Err(e) = command::set_cpu_boost(device, boost) {
                            self.set_error_message(format!("Failed CPU boost: {}", e));
                        } else {
                            self.cpu_boost = boost;
                            self.set_optional_status_message(format!("CPU {:?}", boost));
                        }
                    }
                }
            }
            PerformanceAction::SetGpuBoost(boost) => {
                if self.status.performance_mode == "Custom" {
                    if let Some(ref device) = self.device {
                        if let Err(e) = command::set_gpu_boost(device, boost) {
                            self.set_error_message(format!("Failed GPU boost: {}", e));
                        } else {
                            self.gpu_boost = boost;
                            self.set_optional_status_message(format!("GPU {:?}", boost));
                        }
                    }
                }
            }
        }
    }

    fn set_fan_mode(&mut self, mode: &str, rpm: Option<u16>) {
        if let Some(ref device) = self.device {
            let result = match mode {
                "auto" => {
                    self.auto_fan_cap_override = false;
                    match command::set_fan_mode(device, FanMode::Auto) {
                        Ok(_) => {
                            self.status.fan_speed = "Auto".to_string();
                            self.status.fan_rpm = None;
                            Ok(())
                        }
                        Err(e) => Err(e),
                    }
                }
                "manual" => {
                    self.auto_fan_cap_override = false;
                    self.auto_fan_max_rpm_editing = false;
                    match command::set_fan_mode(device, FanMode::Manual) {
                        Ok(_) => {
                            let rpm_val = rpm.unwrap_or(2000);
                            match command::set_fan_rpm(device, rpm_val, true) {
                                Ok(_) => {
                                    self.status.fan_speed = "Manual".to_string();
                                    self.status.fan_rpm = Some(rpm_val);
                                    Ok(())
                                }
                                Err(e) => Err(e),
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                _ => return,
            };

            match result {
                Ok(_) => {
                    self.set_optional_status_message(format!("Fan set to {} mode", mode));
                }
                Err(e) => {
                    self.set_status_message(format!("Failed to set fan: {}", e));
                }
            }
        } else {
            self.set_no_device_message();
        }
    }

    fn set_fan_rpm_only(&mut self, rpm: u16) {
        match execute_device_command_simple(
            self.device.as_ref(),
            |device| command::set_fan_rpm(device, rpm, true),
            &format!("Fans RPM set to: {}", rpm),
            "Failed to set fan RPM",
        ) {
            Ok(message) => {
                self.status.fan_rpm = Some(rpm);
                self.set_optional_status_message(message);
            }
            Err(message) => {
                self.set_error_message(message);
            }
        }
    }

    fn enforce_manual_fan_rpm(&mut self) {
        if self.auto_fan_cap_override {
            return;
        }
        if self.status.fan_speed == "Manual" {
            if let Some(ref device) = self.device {
                // Periodically re-set manual RPM (device may drift after perf mode changes).
                if let Some(current_set_rpm) =
                    get_fan_rpm_set(device, librazer::types::FanZone::Zone1)
                {
                    if let Ok(_) = command::set_fan_rpm(device, current_set_rpm, true) {
                        self.manual_fan_rpm = current_set_rpm;
                        self.status.fan_rpm = Some(current_set_rpm);
                        self.last_fan_enforce_time = std::time::Instant::now();
                    }
                }
            }
        }
    }

    fn enforce_auto_fan_max_rpm(&mut self) {
        if !self.auto_fan_limit_enabled
            || !self.is_user_auto_mode()
            || self.auto_fan_max_rpm_editing
        {
            return;
        }

        let Some(ref device) = self.device else {
            return;
        };
        let Some(actual) = self.status.fan_actual_rpm else {
            return;
        };
        let max = self.auto_fan_max_rpm;

        if !self.auto_fan_cap_override {
            if actual > max {
                if command::set_fan_mode(device, FanMode::Manual).is_ok()
                    && command::set_fan_rpm(device, max, true).is_ok()
                {
                    self.auto_fan_cap_override = true;
                    self.status.fan_rpm = Some(max);
                }
            }
        } else if actual < max.saturating_sub(AUTO_FAN_HYSTERESIS) {
            if command::set_fan_mode(device, FanMode::Auto).is_ok() {
                self.auto_fan_cap_override = false;
                self.status.fan_rpm = None;
            }
        } else if command::set_fan_rpm(device, max, true).is_ok() {
            self.status.fan_rpm = Some(max);
        }
    }

    fn restore_auto_fan_mode(&mut self) {
        self.auto_fan_cap_override = false;
        if let Some(ref device) = self.device {
            match command::set_fan_mode(device, FanMode::Auto) {
                Ok(_) => {
                    self.status.fan_rpm = None;
                    self.set_optional_status_message("Auto fan mode restored".into());
                }
                Err(e) => {
                    self.set_error_message(format!("Failed to restore auto fan mode: {}", e));
                }
            }
        }
    }

    fn render_fan_section(&mut self, ui: &mut egui::Ui) {
        use ui::fan::{render_fan_section, FanAction};

        let key = egui::Id::new("max_fan_speed_enabled");
        let mut max_enabled = ui.ctx().data(|d| d.get_temp::<bool>(key).unwrap_or(false));
        let (action, new_toggle) = render_fan_section(
            ui,
            &self.status.fan_speed,
            self.status.fan_actual_rpm,
            self.status.fan_rpm,
            &mut self.manual_fan_rpm,
            self.auto_fan_limit_enabled,
            &mut self.auto_fan_max_rpm,
            self.status_messages,
            self.status.performance_mode == "Custom",
            max_enabled,
        );
        if new_toggle != max_enabled && self.status.performance_mode == "Custom" {
            if let Some(ref device) = self.device {
                let result = if new_toggle {
                    command::set_max_fan_speed_mode(device, MaxFanSpeedMode::Enable)
                } else {
                    command::set_max_fan_speed_mode(device, MaxFanSpeedMode::Disable)
                };
                match result {
                    Ok(_) => {
                        max_enabled = new_toggle;
                        self.set_optional_status_message(if new_toggle {
                            "Max fan enabled".into()
                        } else {
                            "Max fan disabled".into()
                        });
                    }
                    Err(e) => self.set_error_message(format!("Failed to toggle max fan: {}", e)),
                }
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(key, max_enabled));

        match action {
            FanAction::None => {}
            FanAction::SetAutoMode => {
                self.set_fan_mode("auto", None);
            }
            FanAction::SetManualMode(rpm) => {
                self.set_fan_mode("manual", Some(rpm));
            }
            FanAction::SetManualRpm(rpm) => {
                self.set_fan_rpm_only(rpm);
            }
            FanAction::SliderDragging(_) => {}
            FanAction::ToggleAutoFanLimit(enabled) => {
                self.auto_fan_limit_enabled = enabled;
                self.auto_fan_max_rpm_editing = false;
                if !enabled && self.auto_fan_cap_override {
                    self.restore_auto_fan_mode();
                }
                if let Err(e) = self.persist_config() {
                    self.set_error_message(format!("Failed to save config: {}", e));
                }
            }
            FanAction::AutoMaxRpmDragging(rpm) => {
                self.auto_fan_max_rpm = rpm;
                self.auto_fan_max_rpm_editing = true;
            }
            FanAction::SetAutoFanMaxRpm(rpm) => {
                self.auto_fan_max_rpm = rpm;
                self.auto_fan_max_rpm_editing = false;
                if self.auto_fan_cap_override {
                    self.set_fan_rpm_only(rpm);
                }
                if let Err(e) = self.persist_config() {
                    self.set_error_message(format!("Failed to save config: {}", e));
                }
            }
        }
    }

    fn set_logo_mode(&mut self, mode: &str) {
        let logo_mode = match Self::string_to_logo_mode(mode) {
            Some(mode) => mode,
            None => return,
        };

        match execute_device_command_simple(
            self.device.as_ref(),
            |device| command::set_logo_mode(device, logo_mode),
            &format!("Logo mode set to {}", mode),
            "Failed to set logo mode",
        ) {
            Ok(message) => {
                self.status.logo_mode = mode.to_string();
                self.set_optional_status_message(message);
            }
            Err(message) => {
                self.set_error_message(message);
            }
        }
    }

    fn set_brightness(&mut self, brightness: u8) {
        match execute_device_command_simple(
            self.device.as_ref(),
            |device| command::set_keyboard_brightness(device, brightness),
            &format!(
                "Brightness set to step {}",
                ui::lighting::raw_brightness_to_step_index(brightness)
            ),
            "Failed to set brightness",
        ) {
            Ok(message) => {
                self.status.keyboard_brightness = brightness;
                self.temp_brightness_step = ui::lighting::raw_brightness_to_step_index(brightness);
                self.set_optional_status_message(message);
            }
            Err(message) => {
                self.set_error_message(message);
            }
        }
    }

    fn toggle_lights_always_on(&mut self) {
        let lights_always_on = if self.status.lights_always_on {
            LightsAlwaysOn::Enable
        } else {
            LightsAlwaysOn::Disable
        };

        if let Some(ref device) = self.device {
            match command::set_lights_always_on(device, lights_always_on) {
                Ok(_) => {
                    self.set_optional_status_message(format!(
                        "Keyboard Backlight Always On {}",
                        if self.status.lights_always_on { "enabled" } else { "disabled" }
                    ));
                    self.update_stored_device_state();
                }
                Err(e) => {
                    self.set_status_message(format!("Failed to set lights always on: {}", e));
                    self.status.lights_always_on = !self.status.lights_always_on;
                }
            }
        } else {
            self.set_no_device_message();
        }
    }

    fn render_lighting_section(&mut self, ui: &mut egui::Ui) {
        use ui::lighting::render_lighting_section;

        let action = render_lighting_section(
            ui,
            &self.status.logo_mode,
            &mut self.temp_brightness_step,
            &mut self.status.lights_always_on,
        );

        if let Some(active) = action.slider_active {
            self.brightness_slider_active = active;
        }

        if let Some(mode) = action.logo_mode {
            self.set_logo_mode(&mode);
        }

        if let Some(brightness) = action.brightness {
            self.set_brightness(brightness);
        }

        if action.lights_always_on {
            self.toggle_lights_always_on();
        }
    }

    fn set_battery_care(&mut self, level: BatteryCare) {
        let previous = self.status.battery_care;
        self.status.battery_care = level;

        if let Some(ref device) = self.device {
            match command::set_battery_care(device, level) {
                Ok(_) => {
                    self.set_optional_status_message(format!(
                        "Battery care set to {}",
                        level.label()
                    ));
                    self.update_stored_device_state();
                }
                Err(e) => {
                    self.set_status_message(format!("Failed to set battery care: {}", e));
                    self.status.battery_care = previous;
                }
            }
        } else {
            self.set_no_device_message();
            self.status.battery_care = previous;
        }
    }

    fn render_battery_section(&mut self, ui: &mut egui::Ui) {
        use ui::battery::{render_battery_section, BatteryAction};

        let action = render_battery_section(ui, &mut self.status.battery_care);

        match action {
            BatteryAction::None => {}
            BatteryAction::SetBatteryCare(level) => {
                self.set_battery_care(level);
            }
        }
    }

    fn is_window_visible(&self) -> bool {
        self.tray_state
            .as_ref()
            .map(|s| s.is_visible())
            .unwrap_or(true)
    }

    fn hide_to_tray(&mut self) {
        if let Some(state) = self.tray_state.clone() {
            state.hide();
            self.set_optional_status_message("Running in system tray".into());
        }
    }

    fn poll_while_hidden(&mut self, ctx: &egui::Context) {
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
        self.process_background_initialization();

        if self.fully_initialized && self.device.is_some() {
            const HIDDEN_POLL_INTERVAL: f32 = 2.5;
            if self.last_perf_poll_time.elapsed().as_secs_f32() >= HIDDEN_POLL_INTERVAL {
                if let Ok(current_ac_power) = get_power_state() {
                    if current_ac_power != self.ac_power {
                        self.ac_power = current_ac_power;
                        self.auto_switch_profile();
                    }
                }
                if let Some(ref device) = self.device {
                    if let Ok((perf_mode, fan_mode)) = command::get_perf_mode(device) {
                        let new_mode = Self::perf_mode_to_string(perf_mode).to_string();
                        if self.status.performance_mode != new_mode {
                            self.status.performance_mode = new_mode;
                            if !self.is_user_auto_mode() {
                                let (fan_speed, fan_rpm) =
                                    Self::get_fan_status_from_mode(fan_mode, device);
                                self.status.fan_speed = fan_speed;
                                self.status.fan_rpm = fan_rpm;
                            }
                        }
                    }
                }
                self.last_perf_poll_time = std::time::Instant::now();
            }
        }
    }
}

impl eframe::App for RazerGuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.is_window_visible() {
            self.poll_while_hidden(ctx);
            if self.should_quit {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        self.process_background_initialization();

        let hidden_on =
            ctx.data(|d| d.get_temp::<bool>("perf_hidden_show".into()).unwrap_or(false));
        if self.device.is_some() {
            if hidden_on {
                self.available_performance_modes = PerfMode::iter().collect();
            } else {
                self.detect_available_performance_modes();
            }
        }

        self.message_manager.update();

        // When minimized, poll infrequently to catch external performance mode changes
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if minimized && self.fully_initialized {
            const PERF_POLL_INTERVAL: f32 = 2.5; // seconds
            if self.last_perf_poll_time.elapsed().as_secs_f32() >= PERF_POLL_INTERVAL {
                if let Some(ref device) = self.device {
                    if let Ok((perf_mode, fan_mode)) = command::get_perf_mode(device) {
                        let new_mode = Self::perf_mode_to_string(perf_mode).to_string();
                        if self.status.performance_mode != new_mode {
                            self.status.performance_mode = new_mode;
                            if !self.is_user_auto_mode() {
                                let (fan_speed, fan_rpm) =
                                    Self::get_fan_status_from_mode(fan_mode, device);
                                self.status.fan_speed = fan_speed;
                                self.status.fan_rpm = fan_rpm;
                            }
                        }
                    }
                }
                self.last_perf_poll_time = std::time::Instant::now();
            }
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.minimize_to_tray {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.hide_to_tray();
            } else {
                self.should_quit = true;
            }
        }

        // Handle quit
        if self.should_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Only update when window is not minimized to save resources
        if !ctx.input(|i| i.viewport().minimized.unwrap_or(false)) {
            // Only do regular updates if fully initialized to avoid slow operations during startup
            if self.fully_initialized {
                // Auto-refresh device status based on backlight setting
                const AUTO_REFRESH_INTERVAL: f32 = 0.5;
                if self.last_refresh_time.elapsed().as_secs_f32() >= AUTO_REFRESH_INTERVAL {
                    if self.device.is_some() && !self.loading {
                        // High-frequency AC power detection: switching triggers profile application.
                        if let Ok(current_ac_power) = get_power_state() {
                            if current_ac_power != self.ac_power {
                                self.ac_power = current_ac_power;
                                self.auto_switch_profile();
                            }
                        }

                        if let Some(ref device) = self.device {
                            self.status.fan_actual_rpm =
                                get_fan_rpm_actual(device, librazer::types::FanZone::Zone1);

                            let (current_fan_mode, set_rpm) = Self::read_current_fan_state(device);
                            if self.status.fan_speed.eq_ignore_ascii_case("manual") {
                                self.apply_device_fan_status(current_fan_mode, set_rpm);
                            }
                        }

                        self.enforce_auto_fan_max_rpm();

                        if self.last_fan_enforce_time.elapsed().as_secs_f32() >= 1.0 {
                            self.enforce_manual_fan_rpm();
                        }

                        if let Some(ref device) = self.device {
                            if !self.brightness_slider_active {
                                if let Ok(brightness) = command::get_keyboard_brightness(device) {
                                    self.status.keyboard_brightness = brightness;
                                    self.temp_brightness_step =
                                        ui::lighting::raw_brightness_to_step_index(brightness);
                                }
                            }
                        }

                        self.sync_other_dynamic_state();
                        if self.device.is_some() {
                            if self.last_state_check_time.elapsed().as_secs_f32() >= 3.0 {
                                if let Err(_e) = self.check_device_state_changes() {
                                    // Fallback: read full device status instead of minimal subset
                                    let _ = self.read_device_status();
                                }
                                self.last_state_check_time = std::time::Instant::now();
                            }
                        }
                    }

                    self.last_refresh_time = std::time::Instant::now();
                }
            }
        }
        // Enforce a minimum detecting period before showing "No device detected"
        if self.detecting_device && self.device.is_none() && self.device_detection_done {
            if std::time::Instant::now() >= self.min_detecting_until {
                self.detecting_device = false;
            }
        }
        // (clear_status_message_if_disabled removed)
        let prev_startup = self.run_at_startup;
        let footer_height = egui::TopBottomPanel::bottom("footer")
            .show(ctx, |ui| {
                ui::footer::render_footer(
                    ui,
                    &mut self.status_messages,
                    &mut self.minimize_to_tray,
                    &mut self.run_at_startup,
                );
            })
            .response
            .rect
            .height();

        if self.run_at_startup != prev_startup {
            if let Err(e) = startup::set_startup_enabled(self.run_at_startup) {
                self.set_error_message(format!("Failed to update startup setting: {}", e));
                self.run_at_startup = prev_startup;
            } else {
                self.set_optional_status_message(if self.run_at_startup {
                    "Run at startup enabled".into()
                } else {
                    "Run at startup disabled".into()
                });
            }
        }

        let central_response = egui::CentralPanel::default().show(ctx, |ui| {
            // Header with device name and status messages
            ui::header::render_header(
                ui,
                ctx,
                self.loading,
                &self.system_specs,
                &self.device,
                &self.message_manager,
                self.detecting_device,
            );
            ui.separator();

            self.render_performance_section(ui);
            ui.separator();

            self.render_fan_section(ui);
            ui.separator();

            self.render_lighting_section(ui);
            ui.separator();

            self.render_battery_section(ui);
            ui.separator();

            self.render_profiles_section(ui);
        });
        // Discrete height adjustment only when custom/debug controls appear or disappear
        let custom_visible_now = self.device.is_some() && self.status.performance_mode == "Custom";
        if self.base_window_height == 0.0 {
            // Capture initial (non-custom) height once
            self.base_window_height =
                central_response.response.rect.height() + footer_height + 16.0;
        }
        if custom_visible_now != self.custom_controls_visible_last {
            let width = 450.0;
            if custom_visible_now {
                // Estimate added height for custom controls (CPU row + GPU row + spacing)
                let added = 3.0 * ctx.style().spacing.interact_size.y;
                self.expanded_window_height = Some(self.base_window_height + added);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    width,
                    self.expanded_window_height.unwrap(),
                )));
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    width,
                    self.base_window_height,
                )));
            }
            self.custom_controls_visible_last = custom_visible_now;
        }
    }
}
fn load_icon() -> IconData {
    const ICON_DATA: &[u8] = include_bytes!("../rhelper.ico");

    if let Ok(image) = image::load_from_memory(ICON_DATA) {
        let image = image.resize_exact(32, 32, image::imageops::FilterType::Lanczos3).to_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();

        IconData { rgba, width, height }
    } else {
        let size = 32;
        let mut rgba = vec![0u8; (size * size * 4) as usize];

        for i in 0..(size * size) as usize {
            let base = i * 4;
            rgba[base] = 0;
            rgba[base + 1] = 150;
            rgba[base + 2] = 255;
            rgba[base + 3] = 255;
        }

        IconData { rgba, width: size, height: size }
    }
}

#[cfg(windows)]
fn set_windows_app_id() {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    // Build a per-version AppUserModelID so taskbar grouping updates with releases
    let app_id =
        format!("RHelper.Application.{}\0", APP_VERSION).encode_utf16().collect::<Vec<u16>>();
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(PCWSTR(app_id.as_ptr()));
    }
}

#[cfg(not(windows))]
fn set_windows_app_id() {}

fn main() -> Result<(), eframe::Error> {
    set_windows_app_id();
    let initial_height = 580.0;
    let egui_icon = load_icon();
    let tray_icon = tray::icon_from_egui(egui_icon.clone());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([450.0, initial_height])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_fullscreen(false)
            .with_title(APP_NAME)
            .with_icon(egui_icon)
            .with_always_on_top()
            .with_active(true),
        ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(500));
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::Normal,
                ));
            });

            let tray_state = tray::TraySharedState::new(cc.egui_ctx.clone());
            if let Some(hwnd) = tray::hwnd_from_window_handle(cc) {
                tray_state.set_hwnd(hwnd);
            }
            let tray_guard = tray::TrayHandle::init(tray_icon, Arc::clone(&tray_state));
            let mut app = RazerGuiApp::new();
            app.tray_state = Some(tray_state);
            app._tray_guard = Some(tray_guard);
            app.base_window_height = initial_height as f32;
            Ok(Box::new(app))
        }),
    )
}
