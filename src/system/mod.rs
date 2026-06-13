pub mod specs;
pub mod thermal;

pub use specs::{get_system_specs, resolve_device_model, SystemSpecs};
pub use thermal::ThermalSnapshot;
