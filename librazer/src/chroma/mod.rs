mod device;
mod packet;

pub use device::{probe_peripheral_battery, ChromaHandle, PeripheralBattery};
pub use packet::{build_request, REPORT_LEN};
