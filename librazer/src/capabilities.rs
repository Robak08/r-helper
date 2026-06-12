use crate::command;
use crate::descriptor::Descriptor;
use crate::device::Device;
use crate::feature::ALL_FEATURES;
use crate::profile::BladeGeneration;
use crate::types::FanZone;

use anyhow::Result;

const FEATURE_PERF: &str = "perf";
const FEATURE_FAN: &str = "fan";
const FEATURE_BATTERY_CARE: &str = "battery-care";
const FEATURE_KBD_BACKLIGHT: &str = "kbd-backlight";
const FEATURE_LIGHTS_ALWAYS_ON: &str = "lights-always-on";
const FEATURE_LID_LOGO: &str = "lid-logo";

fn is_known_feature(name: &str) -> bool {
    ALL_FEATURES.contains(&name)
}

/// Probe device capabilities via read-only HID commands.
pub fn probe_features(device: &Device) -> Vec<&'static str> {
    let mut features = Vec::new();

    if command::get_perf_mode(device).is_ok() {
        features.push(FEATURE_PERF);
    }
    if command::get_fan_rpm(device, FanZone::Zone1).is_ok() {
        features.push(FEATURE_FAN);
    }
    if command::get_battery_care(device).is_ok() {
        features.push(FEATURE_BATTERY_CARE);
    }
    if command::get_keyboard_brightness(device).is_ok() {
        features.push(FEATURE_KBD_BACKLIGHT);
    }
    if command::get_lights_always_on(device).is_ok() {
        features.push(FEATURE_LIGHTS_ALWAYS_ON);
    }
    if command::get_logo_mode(device).is_ok() {
        features.push(FEATURE_LID_LOGO);
    }

    features.retain(|f| is_known_feature(f));
    features
}

pub fn resolve_descriptor(
    model_sku: String,
    display_name: String,
    pid: u16,
    generation: BladeGeneration,
    probed_features: Vec<&'static str>,
) -> Descriptor {
    let features: Vec<&'static str> = probed_features
        .into_iter()
        .filter(|f| is_known_feature(f))
        .collect();

    let perf_modes = generation
        .default_perf_modes()
        .map(|modes| modes.to_vec());

    let cpu_boosts = generation.cpu_boosts().map(|b| b.to_vec());
    let gpu_boosts = generation.gpu_boosts().map(|b| b.to_vec());
    let disallowed_boost_pairs = generation.disallowed_pairs().to_vec();

    Descriptor {
        model_sku,
        display_name,
        pid,
        features,
        perf_modes,
        cpu_boosts,
        gpu_boosts,
        disallowed_boost_pairs,
    }
}

pub fn run_init_cmds(device: &Device, cmds: &[u16]) -> Result<()> {
    for &cmd in cmds {
        command::send_command(device, cmd, &[])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_descriptor_modern6_with_all_features() {
        let all = vec![
            FEATURE_PERF,
            FEATURE_FAN,
            FEATURE_BATTERY_CARE,
            FEATURE_KBD_BACKLIGHT,
            FEATURE_LIGHTS_ALWAYS_ON,
            FEATURE_LID_LOGO,
        ];
        let d = resolve_descriptor(
            "RZ09-0528".into(),
            "Razer Blade 16 (2025)".into(),
            0x02c6,
            BladeGeneration::Modern6,
            all,
        );

        assert_eq!(d.model_sku, "RZ09-0528");
        assert_eq!(d.display_name, "Razer Blade 16 (2025)");
        assert_eq!(d.pid, 0x02c6);
        assert_eq!(d.features.len(), 6);
        assert!(d.perf_modes.as_ref().unwrap().contains(&crate::types::PerfMode::Hyperboost));
        assert!(d.cpu_boosts.is_some());
        assert!(d.gpu_boosts.is_some());
        assert_eq!(d.disallowed_boost_pairs.len(), 1);
    }

    #[test]
    fn resolve_descriptor_discovery_no_perf_mode_list() {
        let d = resolve_descriptor(
            "RZ09-0421".into(),
            "Razer Blade 15 (2022)".into(),
            0x028a,
            BladeGeneration::Legacy4,
            vec![FEATURE_PERF, FEATURE_FAN],
        );

        assert!(d.perf_modes.as_ref().unwrap().contains(&crate::types::PerfMode::Balanced));
        assert!(d.cpu_boosts.is_none());
        assert!(d.disallowed_boost_pairs.is_empty());
    }

    #[test]
    fn resolve_descriptor_drops_unknown_features() {
        let d = resolve_descriptor(
            "RZ09-0482".into(),
            "Razer Blade 14 (2023)".into(),
            0x029d,
            BladeGeneration::Discovery,
            vec![FEATURE_PERF, FEATURE_FAN, "not-a-feature"],
        );

        assert_eq!(d.features.len(), 2);
        assert!(!d.features.contains(&FEATURE_LID_LOGO));
    }
}
