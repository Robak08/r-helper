pub mod specs;
pub mod thermal;

pub use specs::{SystemSpecs, get_system_specs, resolve_device_model};
pub use thermal::ThermalSnapshot;
