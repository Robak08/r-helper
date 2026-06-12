use crate::utils::{clean_display_string, execute_powershell_command};
use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct SystemSpecs {
    pub device_model: String,
    pub gpu_models: Vec<String>,
    pub cpu_name: String,
    pub ram_gb: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct HardwareSpecs {
    pub cpu_name: String,
    pub gpu_models: Vec<String>,
    pub ram_gb: Option<u32>,
}

impl Default for HardwareSpecs {
    fn default() -> Self {
        Self {
            cpu_name: "Unknown".to_string(),
            gpu_models: vec!["Unknown".to_string()],
            ram_gb: None,
        }
    }
}

impl Default for SystemSpecs {
    fn default() -> Self {
        Self {
            device_model: "Unknown".to_string(),
            gpu_models: vec!["Unknown".to_string()],
            cpu_name: "Unknown".to_string(),
            ram_gb: None,
        }
    }
}

/// Load CPU, GPU, and RAM via a single PowerShell invocation.
pub fn load_hardware_specs() -> HardwareSpecs {
    match load_hardware_specs_inner() {
        Ok(specs) => specs,
        Err(e) => {
            eprintln!("Failed to load hardware specs: {}", e);
            HardwareSpecs::default()
        }
    }
}

pub fn get_system_specs(device_name: Option<&str>, usb_pid: Option<u16>) -> SystemSpecs {
    let hw = load_hardware_specs();
    let mut specs = SystemSpecs::default();
    specs.device_model = resolve_device_model(device_name, usb_pid);
    if hw.cpu_name != "Unknown" {
        specs.cpu_name = hw.cpu_name;
    }
    if !hw.gpu_models.is_empty() && hw.gpu_models != vec!["Unknown".to_string()] {
        specs.gpu_models = hw.gpu_models;
    }
    specs.ram_gb = hw.ram_gb;
    specs
}

/// Prefer PID-specific marketing name (includes year); fall back to BIOS product name.
pub fn resolve_device_model(device_name: Option<&str>, usb_pid: Option<u16>) -> String {
    if let Some(pid) = usb_pid {
        if let Some(name) = librazer::profile::lookup_marketing_name(pid) {
            return name.to_string();
        }
    }

    device_name.map(simplify_model_name).unwrap_or_else(|| "Unknown".to_string())
}

#[derive(Debug, Deserialize)]
struct PsHardwareSpecs {
    #[serde(default)]
    gpu: serde_json::Value,
    #[serde(default)]
    cpu: String,
    #[serde(default)]
    ram: Option<u32>,
}

#[cfg(target_os = "windows")]
fn load_hardware_specs_inner() -> Result<HardwareSpecs> {
    const SCRIPT: &str = r#"
$gpu = @(Get-WmiObject -Class Win32_VideoController |
    Where-Object { $_.Name -notlike '*Virtual*' -and $_.Name -notlike '*Basic*' } |
    ForEach-Object { $_.Name })
$cpu = (Get-WmiObject -Class Win32_Processor | Select-Object -First 1).Name
$ram = [math]::Round((Get-WmiObject -Class Win32_ComputerSystem).TotalPhysicalMemory / 1GB)
[PSCustomObject]@{ gpu = $gpu; cpu = $cpu; ram = $ram } | ConvertTo-Json -Compress
"#;

    let output = execute_powershell_command(SCRIPT)?;
    let parsed: PsHardwareSpecs =
        serde_json::from_str(&output).context("failed to parse hardware specs JSON")?;

    let gpu_models = parse_gpu_json(&parsed.gpu);
    let cpu_name = clean_display_string(parsed.cpu.trim());
    let cpu_name = if cpu_name.is_empty() {
        "Unknown".to_string()
    } else {
        cpu_name
    };

    Ok(HardwareSpecs {
        cpu_name,
        gpu_models,
        ram_gb: parsed.ram,
    })
}

#[cfg(not(target_os = "windows"))]
fn load_hardware_specs_inner() -> Result<HardwareSpecs> {
    Err(anyhow::anyhow!("Hardware specs detection only supported on Windows"))
}

fn parse_gpu_json(value: &serde_json::Value) -> Vec<String> {
    let names: Vec<String> = match value {
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|v| v.as_str().map(clean_display_string))
            .filter(|s| !s.is_empty())
            .collect(),
        serde_json::Value::String(s) => {
            let cleaned = clean_display_string(s);
            if cleaned.is_empty() {
                vec![]
            } else {
                vec![cleaned]
            }
        }
        _ => vec![],
    };

    if names.is_empty() {
        vec!["No discrete GPU detected".to_string()]
    } else {
        names
    }
}

// Keep model + inch size + optional year from BIOS when PID is unknown.
fn simplify_model_name(name: &str) -> String {
    let s = name.trim();
    if let Some(open) = s.find('(') {
        if let Some(close_rel) = s[open..].find(')') {
            return s[..open + close_rel + 1].trim().to_string();
        }
    }
    if let Some(blade_pos) = s.find("Blade") {
        if let Some(rel_digit) = s[blade_pos..].find(|c: char| c.is_ascii_digit()) {
            let mut end = blade_pos + rel_digit;
            let bytes = s.as_bytes();
            while end < s.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end < s.len() && bytes[end] == b'"' {
                end += 1;
            }
            return s[..end].trim().to_string();
        }
    }
    s.to_string()
}
