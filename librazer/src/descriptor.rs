use crate::types::{CpuBoost, GpuBoost, PerfMode};

#[derive(Debug, Clone)]
pub struct Descriptor {
    pub model_sku: String,
    pub display_name: String,
    pub pid: u16,
    pub features: Vec<&'static str>,
    pub perf_modes: Option<Vec<PerfMode>>,
    pub cpu_boosts: Option<Vec<CpuBoost>>,
    pub gpu_boosts: Option<Vec<GpuBoost>>,
    pub disallowed_boost_pairs: Vec<(CpuBoost, GpuBoost)>,
}
