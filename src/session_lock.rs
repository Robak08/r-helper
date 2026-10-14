use std::sync::{Arc, Mutex, atomic::AtomicBool};

use crate::device_handle::SharedDevice;

/// Lazily populated when the Razer laptop connects.
pub type DeviceSlot = Arc<Mutex<Option<SharedDevice>>>;

pub fn new_device_slot() -> DeviceSlot {
    Arc::new(Mutex::new(None))
}

pub struct SessionState {
    pub locked: AtomicBool,
}

impl Default for SessionState {
    fn default() -> Self {
        Self { locked: AtomicBool::new(false) }
    }
}

#[cfg(windows)]
pub fn is_session_locked() -> bool {
    use windows::Win32::Foundation::{GetLastError, HANDLE};
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, GetUserObjectInformationW,
        OpenInputDesktop, UOI_NAME,
    };

    unsafe {
        let desktop = match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) {
            Ok(d) => d,
            Err(_) => return false,
        };

        let mut name_buf = [0u16; 256];
        let mut needed = 0u32;
        let ok = GetUserObjectInformationW(
            HANDLE(desktop.0),
            UOI_NAME,
            Some(name_buf.as_mut_ptr().cast()),
            (name_buf.len() * 2) as u32,
            Some(&mut needed),
        )
        .is_ok();
        let _ = CloseDesktop(desktop);

        if !ok {
            let _ = GetLastError();
            return false;
        }

        let len = needed as usize / 2;
        let name = String::from_utf16_lossy(&name_buf[..len.min(name_buf.len())]);
        name.trim_end_matches('\0') == "Winlogon"
    }
}

#[cfg(not(windows))]
pub fn is_session_locked() -> bool {
    false
}
