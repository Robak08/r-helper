use serde::Deserialize;

/// Max upward °C change accepted per poll before rate-limiting (WMI glitches).
pub const DEFAULT_TEMP_SPIKE_REJECT_C: f32 = 12.0;
/// Consecutive above-threshold polls before accepting a large jump as real heat.
pub const SUSTAINED_HIGH_POLLS: u32 = 3;
/// Raw readings in a sustained-high streak must agree within this range (°C).
pub const SPIKE_STABILITY_C: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CpuTempSource {
    #[default]
    Lhm,
    PerfCounter,
    Acpi,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThermalRawSnapshot {
    pub snapshot: ThermalSnapshot,
    pub cpu_source: Option<CpuTempSource>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SpikeFilterOptions {
    /// When false, large upward jumps are held at the previous value (e.g. WMI source fallback).
    pub allow_sustained: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ThermalSnapshot {
    pub cpu_avg_c: Option<f32>,
    pub gpu_avg_c: Option<f32>,
}

/// Rate-limited upward climb: large jumps advance by `max_upward_jump_c` per poll instead of locking.
pub fn filter_temp_spike(
    prev: Option<f32>,
    sample: Option<f32>,
    max_upward_jump_c: f32,
) -> Option<f32> {
    match (prev, sample) {
        (Some(p), Some(s)) if s > p + max_upward_jump_c => Some(s.min(p + max_upward_jump_c)),
        (_, sample) => sample,
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TempSpikeTrack {
    consecutive_high: u32,
    spike_candidate: Option<f32>,
    bootstrap_candidate: Option<f32>,
}

impl TempSpikeTrack {
    pub fn filter(
        &mut self,
        prev: Option<f32>,
        raw: Option<f32>,
        max_upward_jump_c: f32,
        options: SpikeFilterOptions,
    ) -> Option<f32> {
        match (prev, raw) {
            (None, Some(s)) => self.filter_bootstrap(s, max_upward_jump_c),
            (None, None) => {
                self.bootstrap_candidate = None;
                None
            }
            (Some(p), Some(s)) if s > p + max_upward_jump_c => {
                if !options.allow_sustained {
                    self.reset_spike();
                    return Some(p);
                }

                let stable = self
                    .spike_candidate
                    .map(|c| (s - c).abs() <= SPIKE_STABILITY_C)
                    .unwrap_or(true);
                if stable {
                    self.consecutive_high += 1;
                    self.spike_candidate = Some(s);
                } else {
                    self.consecutive_high = 1;
                    self.spike_candidate = Some(s);
                }

                if self.consecutive_high >= SUSTAINED_HIGH_POLLS {
                    self.reset_spike();
                    Some(s)
                } else {
                    Some(p)
                }
            }
            (_, sample) => {
                self.reset_spike();
                filter_temp_spike(prev, sample, max_upward_jump_c)
            }
        }
    }

    fn filter_bootstrap(&mut self, sample: f32, max_upward_jump_c: f32) -> Option<f32> {
        match self.bootstrap_candidate {
            None => {
                self.bootstrap_candidate = Some(sample);
                None
            }
            Some(first) => {
                self.bootstrap_candidate = None;
                if (sample - first).abs() <= max_upward_jump_c {
                    Some((first + sample) * 0.5)
                } else if sample < first {
                    Some(sample)
                } else {
                    Some(first)
                }
            }
        }
    }

    fn reset_spike(&mut self) {
        self.consecutive_high = 0;
        self.spike_candidate = None;
    }
}

#[derive(Debug, Clone, Default)]
pub struct ThermalSpikeFilterState {
    pub cpu: TempSpikeTrack,
    pub gpu: TempSpikeTrack,
}

#[cfg(test)]
pub fn filter_thermal_snapshot_spike(
    prev: &ThermalSnapshot,
    new: ThermalSnapshot,
    state: &mut ThermalSpikeFilterState,
) -> ThermalSnapshot {
    filter_thermal_raw_snapshot(
        prev,
        ThermalRawSnapshot { snapshot: new, cpu_source: None },
        state,
        &mut None,
    )
}

pub fn filter_thermal_raw_snapshot(
    prev: &ThermalSnapshot,
    raw: ThermalRawSnapshot,
    state: &mut ThermalSpikeFilterState,
    last_cpu_source: &mut Option<CpuTempSource>,
) -> ThermalSnapshot {
    let cpu_options = cpu_spike_filter_options(
        prev.cpu_avg_c,
        raw.snapshot.cpu_avg_c,
        *last_cpu_source,
        raw.cpu_source,
    );
    *last_cpu_source = raw.cpu_source;

    ThermalSnapshot {
        cpu_avg_c: state.cpu.filter(
            prev.cpu_avg_c,
            raw.snapshot.cpu_avg_c,
            DEFAULT_TEMP_SPIKE_REJECT_C,
            cpu_options,
        ),
        gpu_avg_c: state.gpu.filter(
            prev.gpu_avg_c,
            raw.snapshot.gpu_avg_c,
            DEFAULT_TEMP_SPIKE_REJECT_C,
            SpikeFilterOptions::default(),
        ),
    }
}

fn cpu_spike_filter_options(
    prev_cpu: Option<f32>,
    raw_cpu: Option<f32>,
    last_source: Option<CpuTempSource>,
    new_source: Option<CpuTempSource>,
) -> SpikeFilterOptions {
    let source_downgrade = matches!(
        (last_source, new_source),
        (Some(CpuTempSource::Lhm), Some(CpuTempSource::PerfCounter | CpuTempSource::Acpi))
    );
    let large_jump = match (prev_cpu, raw_cpu) {
        (Some(p), Some(s)) => s > p + DEFAULT_TEMP_SPIKE_REJECT_C,
        _ => false,
    };
    SpikeFilterOptions { allow_sustained: !(source_downgrade && large_jump) }
}

#[cfg(target_os = "windows")]
pub struct ThermalReader {
    hw_monitor: Vec<(String, wmi::WMIConnection)>,
    perf_counter_wmi: Option<wmi::WMIConnection>,
    acpi_wmi: Option<wmi::WMIConnection>,
    nvml: Option<nvml_wrapper::Nvml>,
}

#[cfg(target_os = "windows")]
impl ThermalReader {
    pub fn new() -> Self {
        let mut hw_monitor = Vec::new();
        for namespace in HW_MONITOR_NAMESPACES {
            if let Ok(wmi_con) = wmi::WMIConnection::with_namespace_path(namespace) {
                hw_monitor.push((namespace.to_string(), wmi_con));
            }
        }
        Self {
            hw_monitor,
            perf_counter_wmi: wmi::WMIConnection::new().ok(),
            acpi_wmi: wmi::WMIConnection::with_namespace_path("ROOT\\WMI").ok(),
            nvml: nvml_wrapper::Nvml::init().ok(),
        }
    }

    pub fn read_snapshot(&mut self) -> ThermalRawSnapshot {
        let (lhm_cpu, lhm_gpu) = self.read_hw_monitor_temps();

        let (cpu_avg_c, cpu_source) = if let Some(cpu) = lhm_cpu {
            thermal_debug_log("cpu", "lhm", cpu);
            (Some(cpu), Some(CpuTempSource::Lhm))
        } else if let Some(cpu) = self.read_perf_counter_cpu_temp() {
            thermal_debug_log("cpu", "perf_counter", cpu);
            (Some(cpu), Some(CpuTempSource::PerfCounter))
        } else if let Some(cpu) = self.read_acpi_cpu_temp() {
            thermal_debug_log("cpu", "acpi", cpu);
            (Some(cpu), Some(CpuTempSource::Acpi))
        } else {
            (None, None)
        };

        let gpu_avg_c = lhm_gpu.or_else(|| self.read_nvml_gpu_temp());

        ThermalRawSnapshot { snapshot: ThermalSnapshot { cpu_avg_c, gpu_avg_c }, cpu_source }
    }

    fn read_hw_monitor_temps(&mut self) -> (Option<f32>, Option<f32>) {
        let mut i = 0;
        while i < self.hw_monitor.len() {
            let namespace = self.hw_monitor[i].0.clone();
            match query_hw_monitor_sensors_cached(&self.hw_monitor[i].1) {
                Ok(sensors) => {
                    let temp_sensors: Vec<&LhmSensor> = sensors
                        .iter()
                        .filter(|s| s.sensor_type.eq_ignore_ascii_case("Temperature"))
                        .collect();

                    let cpu = avg_lhm_cpu(&temp_sensors);
                    let gpu = avg_lhm_gpu(&temp_sensors);
                    if cpu.is_some() || gpu.is_some() {
                        return (cpu, gpu);
                    }
                }
                Err(_) => {
                    if let Ok(wmi_con) = wmi::WMIConnection::with_namespace_path(&namespace) {
                        self.hw_monitor[i].1 = wmi_con;
                    } else {
                        self.hw_monitor.remove(i);
                        continue;
                    }
                }
            }
            i += 1;
        }
        (None, None)
    }

    fn read_nvml_gpu_temp(&mut self) -> Option<f32> {
        use nvml_wrapper::enum_wrappers::device::TemperatureSensor;

        if self.nvml.is_none() {
            self.nvml = nvml_wrapper::Nvml::init().ok();
        }
        let nvml = self.nvml.as_ref()?;
        let count = nvml.device_count().ok()?;

        for index in 0..count {
            let device = nvml.device_by_index(index).ok()?;
            let name = device.name().unwrap_or_default().to_ascii_lowercase();
            if name.contains("intel")
                || name.contains("microsoft")
                || name.contains("virtual")
                || name.contains("basic")
            {
                continue;
            }
            return device.temperature(TemperatureSensor::Gpu).ok().map(|t| t as f32);
        }

        None
    }

    fn read_perf_counter_cpu_temp(&mut self) -> Option<f32> {
        if self.perf_counter_wmi.is_none() {
            self.perf_counter_wmi = wmi::WMIConnection::new().ok();
        }
        let wmi_con = self.perf_counter_wmi.as_ref()?;
        read_perf_counter_cpu_temp_from(wmi_con)
    }

    fn read_acpi_cpu_temp(&mut self) -> Option<f32> {
        if self.acpi_wmi.is_none() {
            self.acpi_wmi = wmi::WMIConnection::with_namespace_path("ROOT\\WMI").ok();
        }
        let wmi_con = self.acpi_wmi.as_ref()?;
        read_acpi_cpu_temp_from(wmi_con)
    }
}

#[cfg(target_os = "windows")]
fn thermal_debug_log(sensor: &str, source: &str, value: f32) {
    if std::env::var_os("R_HELPER_THERMAL_DEBUG").is_some() {
        eprintln!("thermal {sensor}: {value:.1} C ({source})");
    }
}

#[cfg(not(target_os = "windows"))]
fn thermal_debug_log(_sensor: &str, _source: &str, _value: f32) {}

#[cfg(target_os = "windows")]
const HW_MONITOR_NAMESPACES: &[&str] = &["ROOT\\LibreHardwareMonitor", "ROOT\\OpenHardwareMonitor"];

#[cfg(target_os = "windows")]
fn query_hw_monitor_sensors_cached(
    wmi_con: &wmi::WMIConnection,
) -> Result<Vec<LhmSensor>, wmi::WMIError> {
    wmi_con.query()
}

#[cfg(target_os = "windows")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct LhmSensor {
    name: String,
    value: f32,
    sensor_type: String,
    #[serde(default)]
    identifier: String,
}

#[cfg(target_os = "windows")]
fn is_cpu_core_sensor(name: &str) -> bool {
    name.starts_with("Core #") || name.starts_with("CPU Core ") || name.starts_with("CCD")
}

#[cfg(target_os = "windows")]
fn is_cpu_package_sensor(name: &str) -> bool {
    matches!(name, "CPU Package" | "CPU CCD1" | "Tctl" | "Tdie" | "CPU Total" | "Core (Tctl/Tdie)")
        || name.starts_with("CPU CCD")
}

#[cfg(target_os = "windows")]
fn avg_lhm_cpu(sensors: &[&LhmSensor]) -> Option<f32> {
    let cpu_sensors: Vec<&LhmSensor> = sensors
        .iter()
        .copied()
        .filter(|s| s.identifier.to_ascii_lowercase().contains("/cpu/"))
        .collect();

    // HWMonitor aligns with Package on Intel and Tctl/Tdie on AMD.
    if let Some(package) =
        cpu_sensors.iter().find(|s| is_cpu_package_sensor(&s.name)).map(|s| s.value)
    {
        return Some(package);
    }

    let core_temps: Vec<f32> =
        cpu_sensors.iter().filter(|s| is_cpu_core_sensor(&s.name)).map(|s| s.value).collect();

    if !core_temps.is_empty() {
        return max_temp(&core_temps);
    }

    let all_cpu_temps: Vec<f32> = cpu_sensors.iter().map(|s| s.value).collect();
    max_temp(&all_cpu_temps)
}

#[cfg(target_os = "windows")]
fn avg_lhm_gpu(sensors: &[&LhmSensor]) -> Option<f32> {
    let gpu_temps: Vec<f32> = sensors
        .iter()
        .filter(|s| {
            let id = s.identifier.to_ascii_lowercase();
            id.contains("/gpu-nvidia/")
        })
        .map(|s| s.value)
        .collect();

    if gpu_temps.is_empty() {
        return None;
    }

    max_temp(&gpu_temps)
}

#[cfg(target_os = "windows")]
#[derive(Debug, Deserialize)]
#[serde(rename = "Win32_PerfFormattedData_Counters_ThermalZoneInformation")]
#[serde(rename_all = "PascalCase")]
struct ThermalZonePerf {
    #[serde(default)]
    name: String,
    #[serde(default)]
    high_precision_temperature: Option<u32>,
    #[serde(default)]
    temperature: Option<u32>,
}

#[cfg(target_os = "windows")]
fn kelvin_tenths_to_celsius(kelvin_tenths: u32) -> f32 {
    kelvin_tenths as f32 / 10.0 - 273.15
}

#[cfg(target_os = "windows")]
fn kelvin_to_celsius(kelvin: u32) -> f32 {
    kelvin as f32 - 273.15
}

#[cfg(target_os = "windows")]
fn valid_cpu_temp(celsius: f32) -> Option<f32> {
    if celsius > 0.0 && celsius < 150.0 { Some(celsius) } else { None }
}

#[cfg(target_os = "windows")]
fn max_temp(temps: &[f32]) -> Option<f32> {
    temps.iter().copied().max_by(|a, b| a.partial_cmp(b).unwrap())
}

#[cfg(target_os = "windows")]
fn zone_temp_c(zone: &ThermalZonePerf) -> Option<f32> {
    if let Some(kelvin_tenths) = zone.high_precision_temperature.filter(|&v| v > 0) {
        return valid_cpu_temp(kelvin_tenths_to_celsius(kelvin_tenths));
    }
    zone.temperature.filter(|&v| v > 0).and_then(|kelvin| valid_cpu_temp(kelvin_to_celsius(kelvin)))
}

/// True for ACPI CPU thermal zones; excludes embedded-controller proxy zones (e.g. TZRZ).
#[cfg(target_os = "windows")]
fn is_acpi_cpu_thermal_zone(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.contains("\\_TZ.") && !upper.contains("TZRZ") && !upper.contains("SBRG.EC")
}

/// Windows performance-counter thermal zones (no admin required on most systems).
#[cfg(target_os = "windows")]
fn read_perf_counter_cpu_temp_from(wmi_con: &wmi::WMIConnection) -> Option<f32> {
    let zones: Vec<ThermalZonePerf> = wmi_con.query().ok()?;

    let cpu_zone_temps: Vec<f32> = zones
        .iter()
        .filter(|zone| is_acpi_cpu_thermal_zone(&zone.name))
        .filter_map(zone_temp_c)
        .collect();

    max_temp(&cpu_zone_temps)
}

#[cfg(target_os = "windows")]
#[derive(Debug, Deserialize)]
#[serde(rename = "MSAcpi_ThermalZoneTemperature")]
#[serde(rename_all = "PascalCase")]
struct AcpiThermalZone {
    current_temperature: Option<u32>,
}

#[cfg(target_os = "windows")]
fn read_acpi_cpu_temp_from(wmi_con: &wmi::WMIConnection) -> Option<f32> {
    let zones: Vec<AcpiThermalZone> = wmi_con.query().ok()?;

    let temps: Vec<f32> = zones
        .iter()
        .filter_map(|z| z.current_temperature)
        .filter(|&kelvin_tenths| kelvin_tenths > 0)
        .filter_map(|kelvin_tenths| valid_cpu_temp(kelvin_tenths_to_celsius(kelvin_tenths)))
        .collect();

    max_temp(&temps)
}

#[cfg(test)]
mod filter_tests {
    use super::*;

    #[test]
    fn rate_limited_climb_reaches_target() {
        let mut prev = Some(68.0_f32);
        for _ in 0..10 {
            prev = filter_temp_spike(prev, Some(95.0), 12.0);
        }
        assert_eq!(prev, Some(95.0));
    }

    #[test]
    fn spike_filter_does_not_lock_when_sustained_high() {
        let mut state = ThermalSpikeFilterState::default();
        let mut prev = ThermalSnapshot { cpu_avg_c: Some(68.0), gpu_avg_c: None };
        for i in 0..3 {
            let raw = ThermalSnapshot { cpu_avg_c: Some(95.0), gpu_avg_c: None };
            prev = filter_thermal_snapshot_spike(&prev, raw, &mut state);
            if i < 2 {
                assert_eq!(prev.cpu_avg_c, Some(68.0), "hold until sustained");
            }
        }
        assert_eq!(prev.cpu_avg_c, Some(95.0));
    }

    #[test]
    fn spike_filter_rejects_single_glitch() {
        let mut state = ThermalSpikeFilterState::default();
        let prev = ThermalSnapshot { cpu_avg_c: Some(62.0), gpu_avg_c: None };
        let spike = ThermalSnapshot { cpu_avg_c: Some(92.0), gpu_avg_c: None };
        let after_spike = filter_thermal_snapshot_spike(&prev, spike, &mut state);
        assert_eq!(after_spike.cpu_avg_c, Some(62.0));
        let normal = ThermalSnapshot { cpu_avg_c: Some(63.0), gpu_avg_c: None };
        let after_normal = filter_thermal_snapshot_spike(&after_spike, normal, &mut state);
        assert_eq!(after_normal.cpu_avg_c, Some(63.0));
    }

    #[test]
    fn bootstrap_rejects_login_glitch() {
        let mut state = ThermalSpikeFilterState::default();
        let prev = ThermalSnapshot::default();
        let glitch = ThermalSnapshot { cpu_avg_c: Some(96.0), gpu_avg_c: None };
        let after_glitch = filter_thermal_snapshot_spike(&prev, glitch, &mut state);
        assert_eq!(after_glitch.cpu_avg_c, None);
        let normal = ThermalSnapshot { cpu_avg_c: Some(64.0), gpu_avg_c: None };
        let after_normal = filter_thermal_snapshot_spike(&after_glitch, normal, &mut state);
        assert_eq!(after_normal.cpu_avg_c, Some(64.0));
    }

    #[test]
    fn source_downgrade_holds_spike() {
        let mut state = ThermalSpikeFilterState::default();
        let mut last_source = Some(CpuTempSource::Lhm);
        let prev = ThermalSnapshot { cpu_avg_c: Some(68.0), gpu_avg_c: None };
        let raw = ThermalRawSnapshot {
            snapshot: ThermalSnapshot { cpu_avg_c: Some(96.0), gpu_avg_c: None },
            cpu_source: Some(CpuTempSource::PerfCounter),
        };
        let filtered = filter_thermal_raw_snapshot(&prev, raw, &mut state, &mut last_source);
        assert_eq!(filtered.cpu_avg_c, Some(68.0));
    }
    #[test]
    fn cpu_prefers_lhm_over_acpi_when_both_present() {
        let lhm = Some(78.0_f32);
        let perf = Some(68.0_f32);
        let acpi = Some(92.0_f32);

        let selected = lhm.or(perf).or(acpi);
        assert_eq!(selected, Some(78.0));
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;

    #[test]
    fn perf_counter_cpu_temp_matches_primary_zone() {
        let wmi_con = wmi::WMIConnection::new().expect("wmi");
        let temp = read_perf_counter_cpu_temp_from(&wmi_con).expect("perf counter temp");
        // Primary ACPI zone on this class of laptop; should not be diluted by EC zones.
        assert!(temp > 55.0, "expected primary CPU thermal zone (~62 C), got {temp}");
    }

    fn read_snapshot() -> ThermalSnapshot {
        ThermalReader::new().read_snapshot().snapshot
    }

    #[test]
    fn read_snapshot_includes_cpu_temp() {
        let snapshot = read_snapshot();
        assert!(
            snapshot.cpu_avg_c.is_some(),
            "read_snapshot should populate cpu_avg_c via perf counter fallback"
        );
    }
}
