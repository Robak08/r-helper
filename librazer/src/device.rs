use crate::capabilities::{probe_features, resolve_descriptor, run_init_cmds};
use crate::descriptor::Descriptor;
use crate::packet::Packet;
use crate::profile::{lookup_profile, resolve_generation};

use anyhow::{Context, Result, anyhow};
use std::{thread, time::Duration};

pub struct Device {
    device: hidapi::HidDevice,
    pub info: Descriptor,
}

fn read_bios_value(name: &str) -> Result<String> {
    #[cfg(target_os = "windows")]
    {
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        let bios = hklm.open_subkey("HARDWARE\\DESCRIPTION\\System\\BIOS")?;
        bios.get_value(name).context(format!("Failed to read BIOS value {}", name))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
        anyhow::bail!("Automatic model detection is not implemented for this platform")
    }
}

// Read the model id and clip to conform with https://mysupport.razer.com/app/answers/detail/a_id/5481
fn read_device_model() -> Result<String> {
    Ok(read_bios_value("SystemSKU")?.chars().take(10).collect())
}

fn read_device_display_name() -> Result<String> {
    read_bios_value("SystemProductName")
}

impl Device {
    pub const RAZER_VID: u16 = crate::enumerate::RAZER_VID;

    pub fn info(&self) -> &Descriptor {
        &self.info
    }

    fn open_hid(pid: u16) -> Result<hidapi::HidDevice> {
        let api = hidapi::HidApi::new().context("Failed to create hid api")?;

        for info in api
            .device_list()
            .filter(|info| (info.vendor_id(), info.product_id()) == (Device::RAZER_VID, pid))
        {
            let device = api.open_path(info.path())?;
            if device.send_feature_report(&[0, 0]).is_ok() {
                return Ok(device);
            }
        }
        anyhow::bail!("Failed to open Razer device with PID {:04x}", pid)
    }

    fn open_by_pid(pid: u16) -> Result<Device> {
        let hid = Self::open_hid(pid)?;
        Ok(Device {
            device: hid,
            info: Descriptor {
                model_sku: String::new(),
                display_name: String::new(),
                pid,
                features: Vec::new(),
                perf_modes: None,
                cpu_boosts: None,
                gpu_boosts: None,
                disallowed_boost_pairs: Vec::new(),
            },
        })
    }

    fn pick_target_pid(pid_list: &[u16]) -> u16 {
        pid_list
            .iter()
            .copied()
            .find(|pid| lookup_profile(*pid).is_some())
            .or_else(|| pid_list.first().copied())
            .expect("pid_list is non-empty")
    }

    pub fn send(&self, report: Packet) -> Result<Packet> {
        let mut response_buf: Vec<u8> = vec![0x00; 1 + std::mem::size_of::<Packet>()];
        let report_bytes: Vec<u8> = (&report).into();
        let mut request_buf = Vec::with_capacity(1 + report_bytes.len());
        request_buf.push(0);
        request_buf.extend_from_slice(&report_bytes);

        const MAX_RETRIES: usize = 5;

        for attempt in 0..MAX_RETRIES {
            thread::sleep(Duration::from_micros(1000));

            self.device
                .send_feature_report(&request_buf)
                .context("Failed to send feature report")?;

            thread::sleep(Duration::from_micros(2000));

            let response_size = self.device.get_feature_report(&mut response_buf)?;
            if response_buf.len() != response_size {
                return Err(anyhow!("Response size != {}", response_buf.len()));
            }

            let response = <&[u8] as TryInto<Packet>>::try_into(&response_buf[1..])?;

            if response.ensure_matches_report(&report).is_ok() {
                return Ok(response);
            } else if attempt == MAX_RETRIES - 1 {
                return Err(anyhow!("Failed to match report after {} attempts", MAX_RETRIES));
            }

            thread::sleep(Duration::from_millis(500));
        }

        Err(anyhow!("Failed to send feature report"))
    }

    pub fn enumerate() -> Result<(Vec<u16>, String)> {
        let razer_pid_list: Vec<_> = hidapi::HidApi::new()?
            .device_list()
            .filter(|info| info.vendor_id() == Device::RAZER_VID)
            .map(|info| info.product_id())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if razer_pid_list.is_empty() {
            anyhow::bail!("No Razer devices found")
        }

        match read_device_model() {
            Ok(model) if model.starts_with("RZ09-") => Ok((razer_pid_list, model)),
            Ok(model) => anyhow::bail!("Detected model but it's not a Razer laptop: {}", model),
            Err(e) => anyhow::bail!("Failed to detect model: {}", e),
        }
    }

    pub fn detect() -> Result<Device> {
        let (pid_list, model_sku) = Device::enumerate()?;
        let display_name = read_device_display_name().unwrap_or_else(|_| model_sku.clone());

        let target_pid = Self::pick_target_pid(&pid_list);
        let mut device = Self::open_by_pid(target_pid)?;

        let generation = resolve_generation(target_pid, &model_sku);

        let probed = probe_features(&device);
        let descriptor =
            resolve_descriptor(model_sku, display_name, target_pid, generation, probed);
        device.info = descriptor;

        let init_cmds = generation.default_init_cmds();
        if !init_cmds.is_empty() {
            run_init_cmds(&device, init_cmds)?;
        }

        Ok(device)
    }
}
