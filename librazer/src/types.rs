use anyhow::{bail, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use strum_macros::{EnumIter, EnumString};

#[derive(Clone, Copy)]
pub enum Cluster {
    Cpu = 0x01,
    Gpu = 0x02,
}

#[derive(Clone, Copy)]
pub enum FanZone {
    Zone1 = 0x01,
    Zone2 = 0x02,
}

#[derive(EnumIter, Clone, Copy, Debug, PartialEq, ValueEnum, Serialize, Deserialize)]
pub enum PerfMode {
    Balanced = 0,
    Performance = 2,
    Custom = 4,
    Silent = 5,
    Battery = 6,
    Hyperboost = 7,
}

#[derive(EnumIter, Clone, Copy, Debug, ValueEnum, PartialEq, Serialize, Deserialize)]
pub enum MaxFanSpeedMode {
    Enable = 2,
    Disable = 0,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum FanMode {
    Auto = 0,
    Manual = 1,
}

#[derive(EnumIter, Clone, Copy, Debug, ValueEnum, PartialEq, Serialize, Deserialize)]
pub enum CpuBoost {
    Low = 0,
    Medium = 1,
    High = 2,
    Boost = 3,
    Undervolt = 4,
}

#[derive(EnumIter, Clone, Copy, Debug, ValueEnum, PartialEq, Serialize, Deserialize)]
pub enum GpuBoost {
    Low = 0,
    Medium = 1,
    High = 2,
}

#[derive(
    EnumString, EnumIter, Clone, Copy, Debug, ValueEnum, PartialEq, Serialize, Deserialize,
)]
pub enum LogoMode {
    Off,
    Breathing,
    Static,
}

#[derive(EnumString, ValueEnum, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LightsAlwaysOn {
    Enable = 0x03,
    Disable = 0x00,
}

#[derive(
    EnumIter, ValueEnum, Debug, Clone, Copy, PartialEq, Serialize, Deserialize,
)]
pub enum BatteryCare {
    Percent50 = 0xB2,
    Percent55 = 0xB7,
    Percent60 = 0xBC,
    Percent65 = 0xC1,
    Percent70 = 0xC6,
    Percent75 = 0xCB,
    Percent80 = 0xD0,
    Disable = 0x50,
}

impl TryFrom<u8> for GpuBoost {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Low),
            1 => Ok(Self::Medium),
            2 => Ok(Self::High),
            _ => bail!("Failed to convert {} to GpuBoost", value),
        }
    }
}

impl TryFrom<u8> for PerfMode {
    type Error = anyhow::Error;

    fn try_from(perf_mode: u8) -> Result<Self, Self::Error> {
        match perf_mode {
            0 => Ok(Self::Balanced),
            2 => Ok(Self::Performance),
            4 => Ok(Self::Custom),
            5 => Ok(Self::Silent),
            6 => Ok(Self::Battery),
            7 => Ok(Self::Hyperboost),
            _ => bail!("Failed to convert {} to PerformanceMode", perf_mode),
        }
    }
}

impl TryFrom<u8> for FanMode {
    type Error = anyhow::Error;

    fn try_from(fan_mode: u8) -> Result<Self, Self::Error> {
        match fan_mode {
            0 => Ok(Self::Auto),
            1 => Ok(Self::Manual),
            _ => bail!("Failed to convert {} to FanMode", fan_mode),
        }
    }
}

impl TryFrom<u8> for CpuBoost {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Low),
            1 => Ok(Self::Medium),
            2 => Ok(Self::High),
            3 => Ok(Self::Boost),
            4 => Ok(Self::Undervolt),
            _ => bail!("Failed to convert {} to CpuBoost", value),
        }
    }
}

impl TryFrom<u8> for LightsAlwaysOn {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(LightsAlwaysOn::Disable),
            3 => Ok(LightsAlwaysOn::Enable),
            _ => bail!("Failed to convert {} to LightsAlwaysOn", value),
        }
    }
}

impl TryFrom<u8> for BatteryCare {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0xB2 => Ok(BatteryCare::Percent50),
            0xB7 => Ok(BatteryCare::Percent55),
            0xBC => Ok(BatteryCare::Percent60),
            0xC1 => Ok(BatteryCare::Percent65),
            0xC6 => Ok(BatteryCare::Percent70),
            0xCB => Ok(BatteryCare::Percent75),
            0xD0 => Ok(BatteryCare::Percent80),
            0x50 => Ok(BatteryCare::Disable),
            _ => bail!("Failed to convert {:#x} to BatteryCare", value),
        }
    }
}

impl BatteryCare {
    /// Synapse-aligned charge limits (50–80% in 5% steps, or disabled = 100%).
    pub const LEVELS: &[BatteryCare] = &[
        BatteryCare::Disable,
        BatteryCare::Percent50,
        BatteryCare::Percent55,
        BatteryCare::Percent60,
        BatteryCare::Percent65,
        BatteryCare::Percent70,
        BatteryCare::Percent75,
        BatteryCare::Percent80,
    ];

    /// Round a percentage to the nearest supported Synapse step.
    pub fn from_percent(percent: u8) -> Result<Self> {
        match percent {
            0..=52 => Ok(BatteryCare::Percent50),
            53..=57 => Ok(BatteryCare::Percent55),
            58..=62 => Ok(BatteryCare::Percent60),
            63..=67 => Ok(BatteryCare::Percent65),
            68..=72 => Ok(BatteryCare::Percent70),
            73..=77 => Ok(BatteryCare::Percent75),
            78..=90 => Ok(BatteryCare::Percent80),
            91..=100 => Ok(BatteryCare::Disable),
            _ => bail!("Invalid battery care percentage: {} (must be 50-100)", percent),
        }
    }

    pub fn to_percent(self) -> u8 {
        match self {
            BatteryCare::Percent50 => 50,
            BatteryCare::Percent55 => 55,
            BatteryCare::Percent60 => 60,
            BatteryCare::Percent65 => 65,
            BatteryCare::Percent70 => 70,
            BatteryCare::Percent75 => 75,
            BatteryCare::Percent80 => 80,
            BatteryCare::Disable => 100,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            BatteryCare::Disable => "Disabled (100%)",
            BatteryCare::Percent50 => "50%",
            BatteryCare::Percent55 => "55%",
            BatteryCare::Percent60 => "60%",
            BatteryCare::Percent65 => "65%",
            BatteryCare::Percent70 => "70%",
            BatteryCare::Percent75 => "75%",
            BatteryCare::Percent80 => "80%",
        }
    }
}

#[cfg(test)]
mod battery_care_tests {
    use super::*;

    #[test]
    fn battery_care_round_trip_bytes() {
        for level in BatteryCare::LEVELS {
            let byte = *level as u8;
            assert_eq!(BatteryCare::try_from(byte).unwrap(), *level);
        }
    }

    #[test]
    fn battery_care_from_percent() {
        assert_eq!(BatteryCare::from_percent(50).unwrap(), BatteryCare::Percent50);
        assert_eq!(BatteryCare::from_percent(80).unwrap(), BatteryCare::Percent80);
        assert_eq!(BatteryCare::from_percent(100).unwrap(), BatteryCare::Disable);
    }
}

impl TryFrom<u8> for MaxFanSpeedMode {
    type Error = anyhow::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x02 => Ok(MaxFanSpeedMode::Enable),
            0x00 => Ok(MaxFanSpeedMode::Disable),
            _ => bail!("Failed to convert {} to MaxFanSpeedMode", value),
        }
    }
}
