use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThermalSnapshot {
    pub cpu_avg_c: Option<f32>,
    pub gpu_avg_c: Option<f32>,
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

    pub fn read_snapshot(&mut self) -> ThermalSnapshot {
        let (mut cpu_avg_c, mut gpu_avg_c) = self.read_hw_monitor_temps();

        if gpu_avg_c.is_none() {
            gpu_avg_c = self.read_nvml_gpu_temp();
        }
        if cpu_avg_c.is_none() {
            cpu_avg_c = self.read_perf_counter_cpu_temp();
        }
        if cpu_avg_c.is_none() {
            cpu_avg_c = self.read_acpi_cpu_temp();
        }

        ThermalSnapshot {
            cpu_avg_c,
            gpu_avg_c,
        }
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
            return device
                .temperature(TemperatureSensor::Gpu)
                .ok()
                .map(|t| t as f32);
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
const HW_MONITOR_NAMESPACES: &[&str] = &[
    "ROOT\\LibreHardwareMonitor",
    "ROOT\\OpenHardwareMonitor",
];

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
    name.starts_with("Core #")
        || name.starts_with("CPU Core ")
        || name.starts_with("CCD")
}

#[cfg(target_os = "windows")]
fn is_cpu_package_sensor(name: &str) -> bool {
    matches!(
        name,
        "CPU Package" | "CPU CCD1" | "Tctl" | "Tdie" | "CPU Total" | "Core (Tctl/Tdie)"
    ) || name.starts_with("CPU CCD")
}

#[cfg(target_os = "windows")]
fn avg_lhm_cpu(sensors: &[&LhmSensor]) -> Option<f32> {
    let cpu_sensors: Vec<&LhmSensor> = sensors
        .iter()
        .copied()
        .filter(|s| s.identifier.to_ascii_lowercase().contains("/cpu/"))
        .collect();

    // HWMonitor aligns with Package on Intel and Tctl/Tdie on AMD.
    if let Some(package) = cpu_sensors
        .iter()
        .find(|s| is_cpu_package_sensor(&s.name))
        .map(|s| s.value)
    {
        return Some(package);
    }

    let core_temps: Vec<f32> = cpu_sensors
        .iter()
        .filter(|s| is_cpu_core_sensor(&s.name))
        .map(|s| s.value)
        .collect();

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

    Some(gpu_temps.iter().sum::<f32>() / gpu_temps.len() as f32)
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
    if celsius > 0.0 && celsius < 150.0 {
        Some(celsius)
    } else {
        None
    }
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
    zone.temperature
        .filter(|&v| v > 0)
        .and_then(|kelvin| valid_cpu_temp(kelvin_to_celsius(kelvin)))
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

    if let Some(temp) = max_temp(&cpu_zone_temps) {
        return Some(temp);
    }

    let all_temps: Vec<f32> = zones.iter().filter_map(zone_temp_c).collect();
    max_temp(&all_temps)
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

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn perf_counter_cpu_temp_matches_primary_zone() {
        let wmi_con = wmi::WMIConnection::new().expect("wmi");
        let temp = read_perf_counter_cpu_temp_from(&wmi_con).expect("perf counter temp");
        // Primary ACPI zone on this class of laptop; should not be diluted by EC zones.
        assert!(
            temp > 55.0,
            "expected primary CPU thermal zone (~62 C), got {temp}"
        );
    }

    fn read_snapshot() -> ThermalSnapshot {
        ThermalReader::new().read_snapshot()
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
