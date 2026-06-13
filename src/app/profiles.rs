use crate::device::CompleteDeviceState;
use crate::device_handle::SharedDevice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSlot {
    Ac,
    Battery,
}

pub fn read_profile_from_device(
    device: &SharedDevice,
) -> Result<CompleteDeviceState, &'static str> {
    match device.with(|d| CompleteDeviceState::read_from_device(d)) {
        Some(Ok(profile)) => Ok(profile),
        Some(Err(_)) => Err("Failed to read device state"),
        None => Err("device busy"),
    }
}
