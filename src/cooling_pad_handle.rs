use std::sync::{Arc, Mutex};

use librazer::cooling_pad::CoolingPadDevice;

/// Thread-shareable cooling pad handle (HID I/O is synchronized via mutex).
#[derive(Clone)]
pub struct SharedCoolingPad(Arc<Mutex<CoolingPadDevice>>);

impl SharedCoolingPad {
    pub fn new(device: CoolingPadDevice) -> Self {
        Self(Arc::new(Mutex::new(device)))
    }

    pub fn arc(&self) -> Arc<Mutex<CoolingPadDevice>> {
        Arc::clone(&self.0)
    }

    pub fn with<R>(&self, f: impl FnOnce(&CoolingPadDevice) -> R) -> Option<R> {
        self.0.lock().ok().map(|guard| f(&*guard))
    }
}
