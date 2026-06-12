use crate::types::{CpuBoost, GpuBoost, PerfMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BladeGeneration {
    /// Battery, Silent, Balanced, Custom — no Performance/Hyperboost
    Legacy4,
    /// Adds Performance, Hyperboost + CPU/GPU boost sliders
    Modern6,
    /// Expose full PerfMode enum (discovery / unknown hardware)
    Discovery,
}

const MODERN6_INIT_CMDS: &[u16] = &[0x0081, 0x0086, 0x0f90, 0x0086, 0x0f10, 0x0087];

const LEGACY4_PERF_MODES: &[PerfMode] = &[
    PerfMode::Battery,
    PerfMode::Silent,
    PerfMode::Balanced,
    PerfMode::Custom,
];

const MODERN6_PERF_MODES: &[PerfMode] = &[
    PerfMode::Battery,
    PerfMode::Silent,
    PerfMode::Balanced,
    PerfMode::Performance,
    PerfMode::Hyperboost,
    PerfMode::Custom,
];

const MODERN6_CPU_BOOSTS: &[CpuBoost] = &[CpuBoost::Low, CpuBoost::Medium, CpuBoost::High];
const MODERN6_GPU_BOOSTS: &[GpuBoost] = &[GpuBoost::Low, GpuBoost::Medium, GpuBoost::High];
const MODERN6_DISALLOWED_PAIRS: &[(CpuBoost, GpuBoost)] = &[(CpuBoost::High, GpuBoost::High)];

impl BladeGeneration {
    pub fn default_perf_modes(self) -> Option<&'static [PerfMode]> {
        match self {
            BladeGeneration::Legacy4 => Some(LEGACY4_PERF_MODES),
            BladeGeneration::Modern6 => Some(MODERN6_PERF_MODES),
            BladeGeneration::Discovery => None,
        }
    }

    pub fn cpu_boosts(self) -> Option<&'static [CpuBoost]> {
        match self {
            BladeGeneration::Modern6 => Some(MODERN6_CPU_BOOSTS),
            BladeGeneration::Legacy4 | BladeGeneration::Discovery => None,
        }
    }

    pub fn gpu_boosts(self) -> Option<&'static [GpuBoost]> {
        match self {
            BladeGeneration::Modern6 => Some(MODERN6_GPU_BOOSTS),
            BladeGeneration::Legacy4 | BladeGeneration::Discovery => None,
        }
    }

    pub fn disallowed_pairs(self) -> &'static [(CpuBoost, GpuBoost)] {
        match self {
            BladeGeneration::Modern6 => MODERN6_DISALLOWED_PAIRS,
            BladeGeneration::Legacy4 | BladeGeneration::Discovery => &[],
        }
    }

    pub fn default_init_cmds(self) -> &'static [u16] {
        match self {
            BladeGeneration::Modern6 => MODERN6_INIT_CMDS,
            BladeGeneration::Legacy4 | BladeGeneration::Discovery => &[],
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PidProfile {
    pub pid: u16,
    pub generation: BladeGeneration,
}

/// Known USB PIDs mapped to firmware generation (one entry per hardware revision).
pub const KNOWN_PROFILES: &[PidProfile] = &[
    // Legacy4
    PidProfile { pid: 0x0279, generation: BladeGeneration::Legacy4 }, // Blade 17 (2021)
    PidProfile { pid: 0x026d, generation: BladeGeneration::Legacy4 }, // Blade 15 Advanced (2021)
    PidProfile { pid: 0x028c, generation: BladeGeneration::Legacy4 }, // Blade 14 (2022)
    PidProfile { pid: 0x028b, generation: BladeGeneration::Legacy4 }, // Blade 17 (2022)
    PidProfile { pid: 0x029c, generation: BladeGeneration::Legacy4 }, // Blade 15 (2023)
    // Discovery
    PidProfile { pid: 0x028a, generation: BladeGeneration::Discovery }, // Blade 15 (2022)
    PidProfile { pid: 0x029d, generation: BladeGeneration::Discovery }, // Blade 14 (2023)
    PidProfile { pid: 0x029f, generation: BladeGeneration::Discovery }, // Blade 16 (2023)
    // Modern6
    PidProfile { pid: 0x02c5, generation: BladeGeneration::Modern6 }, // Blade 14 (2025)
    PidProfile { pid: 0x02c6, generation: BladeGeneration::Modern6 }, // Blade 16 (2025)
];

pub const GENERIC_FALLBACK: PidProfile = PidProfile {
    pid: 0,
    generation: BladeGeneration::Discovery,
};

pub fn lookup_profile(pid: u16) -> Option<&'static PidProfile> {
    KNOWN_PROFILES.iter().find(|p| p.pid == pid)
}

/// Infer generation from SystemSKU when PID is unknown (first 10 chars per Razer support doc).
pub fn infer_generation_from_sku(sku: &str) -> BladeGeneration {
    if sku.starts_with("RZ09-05") {
        return BladeGeneration::Modern6;
    }
    if sku.starts_with("RZ09-048") || sku.starts_with("RZ09-042") {
        return BladeGeneration::Legacy4;
    }
    BladeGeneration::Discovery
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sku_inference_modern6() {
        assert_eq!(infer_generation_from_sku("RZ09-0528"), BladeGeneration::Modern6);
        assert_eq!(infer_generation_from_sku("RZ09-05306"), BladeGeneration::Modern6);
    }

    #[test]
    fn sku_inference_legacy4() {
        assert_eq!(infer_generation_from_sku("RZ09-0421"), BladeGeneration::Legacy4);
        assert_eq!(infer_generation_from_sku("RZ09-04854"), BladeGeneration::Legacy4);
    }

    #[test]
    fn sku_inference_discovery() {
        assert_eq!(infer_generation_from_sku("RZ09-09999"), BladeGeneration::Discovery);
    }

    #[test]
    fn lookup_known_pid() {
        let p = lookup_profile(0x02c6).unwrap();
        assert_eq!(p.generation, BladeGeneration::Modern6);
    }

    #[test]
    fn lookup_unknown_pid() {
        assert!(lookup_profile(0xffff).is_none());
    }
}
