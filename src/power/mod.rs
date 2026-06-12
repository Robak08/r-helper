use anyhow::Result;

#[cfg(target_os = "windows")]
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

/// Windows-reported main battery state (laptop pack charge level).
#[derive(Debug, Clone, Default)]
pub struct BatteryStatus {
    pub percent: Option<u8>,
    pub charging: bool,
    /// Minutes until full/empty; `None` when Windows reports unknown (-1).
    pub time_remaining_mins: Option<u32>,
}

#[cfg(target_os = "windows")]
const BATTERY_FLAG_CHARGING: u8 = 8;
#[cfg(target_os = "windows")]
const BATTERY_PERCENT_UNKNOWN: u8 = 255;

#[cfg(target_os = "windows")]
pub fn get_power_state() -> Result<bool> {
    unsafe {
        let mut status: SYSTEM_POWER_STATUS = std::mem::zeroed();
        if GetSystemPowerStatus(&mut status).is_ok() {
            Ok(status.ACLineStatus == 1)
        } else {
            Ok(true)
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_power_state() -> Result<bool> {
    Ok(true)
}

pub fn get_battery_status() -> BatteryStatus {
    #[cfg(target_os = "windows")]
    {
        unsafe {
            let mut status: SYSTEM_POWER_STATUS = std::mem::zeroed();
            if GetSystemPowerStatus(&mut status).is_err() {
                return BatteryStatus::default();
            }

            let percent = if status.BatteryLifePercent == BATTERY_PERCENT_UNKNOWN {
                None
            } else {
                Some(status.BatteryLifePercent)
            };

            let charging = status.BatteryFlag & BATTERY_FLAG_CHARGING != 0;

            // DWORD(-1) means unknown remaining time.
            let time_remaining_mins = if status.BatteryLifeTime == u32::MAX {
                None
            } else {
                Some(status.BatteryLifeTime / 60)
            };

            BatteryStatus { percent, charging, time_remaining_mins }
        }
    }

    #[cfg(not(target_os = "windows"))]
    BatteryStatus::default()
}
