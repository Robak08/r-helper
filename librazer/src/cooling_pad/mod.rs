mod device;
mod fan;
mod lighting;

pub use device::{is_present, CoolingPadDevice, COOLING_PAD_PID};
pub use fan::{MAX_RPM, MIN_RPM, RPM_STEP};
pub use lighting::PadLightingMode;
