use std::sync::{Arc, Mutex};

use librazer::device::Device;

/// Thread-shareable Razer device handle (HID I/O is synchronized via mutex).
#[derive(Clone)]
pub struct SharedDevice(Arc<Mutex<Device>>);

impl SharedDevice {
    pub fn new(device: Device) -> Self {
        Self(Arc::new(Mutex::new(device)))
    }

    pub fn arc(&self) -> Arc<Mutex<Device>> {
        Arc::clone(&self.0)
    }

    pub fn with<R>(&self, f: impl FnOnce(&Device) -> R) -> Option<R> {
        self.0.lock().ok().map(|guard| f(&*guard))
    }

    pub fn with_mut<R>(&self, f: impl FnOnce(&mut Device) -> R) -> Option<R> {
        self.0.lock().ok().map(|mut guard| f(&mut *guard))
    }

}

/// Run a device command with standard error handling.
pub fn execute_command<T, F>(
    device: Option<&SharedDevice>,
    command: F,
    success_msg: &str,
    error_prefix: &str,
) -> Result<String, String>
where
    F: FnOnce(&Device) -> anyhow::Result<T>,
{
    let Some(shared) = device else {
        return Err("No device connected".to_string());
    };
    match shared.with(command) {
        Some(Ok(_)) => Ok(success_msg.to_string()),
        Some(Err(e)) => Err(format!("{}: {}", error_prefix, e)),
        None => Err("Device busy".to_string()),
    }
}
