/// How a modern Razer headset expects the HID session to be maintained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadsetProfile {
    /// Direct USB connection (headset plugged in). RF_WAKE may be optional.
    WiredDirect,
    /// 2.4 GHz wireless dongle — full arming handshake and periodic RF_WAKE required.
    WirelessDongle,
}

/// HID protocol family used for peripheral battery queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceProtocol {
    Chroma,
    HeadsetModern,
}

const BLACKSHARK_V2_PRO_DONGLE: u16 = 0x0555;
const BLACKSHARK_V3_USB: u16 = 0x057a;
/// Windows-reported PID for BlackShark V3 direct USB (variant of 0x057a).
const BLACKSHARK_V3_USB_WIN: u16 = 0x0579;
const BLACKSHARK_V3_PRO_WIRED: u16 = 0x0576;
const BLACKSHARK_V3_PRO_DONGLE: u16 = 0x0577;
const BLACKSHARK_V3_X_HYPERSPEED: u16 = 0x057d;
const KRAKEN_V4: u16 = 0x0567;
const KRAKEN_V4_PRO: u16 = 0x0568;

/// Known modern-headset PIDs and their session profile.
pub fn headset_profile(pid: u16) -> Option<HeadsetProfile> {
    match pid {
        BLACKSHARK_V3_USB
        | BLACKSHARK_V3_USB_WIN
        | BLACKSHARK_V3_PRO_WIRED
        | KRAKEN_V4
        | KRAKEN_V4_PRO => Some(HeadsetProfile::WiredDirect),
        BLACKSHARK_V2_PRO_DONGLE
        | BLACKSHARK_V3_PRO_DONGLE
        | BLACKSHARK_V3_X_HYPERSPEED => Some(HeadsetProfile::WirelessDongle),
        _ => None,
    }
}

pub fn is_modern_headset_pid(pid: u16) -> bool {
    headset_profile(pid).is_some()
}

pub fn device_protocol(pid: u16) -> DeviceProtocol {
    if is_modern_headset_pid(pid) {
        DeviceProtocol::HeadsetModern
    } else {
        DeviceProtocol::Chroma
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blackshark_v3_is_wired_direct() {
        assert_eq!(
            headset_profile(BLACKSHARK_V3_USB),
            Some(HeadsetProfile::WiredDirect)
        );
        assert_eq!(device_protocol(BLACKSHARK_V3_USB), DeviceProtocol::HeadsetModern);
    }

    #[test]
    fn v3_pro_dongle_is_wireless() {
        assert_eq!(
            headset_profile(BLACKSHARK_V3_PRO_DONGLE),
            Some(HeadsetProfile::WirelessDongle)
        );
    }

    #[test]
    fn mouse_uses_chroma() {
        assert_eq!(device_protocol(0x00a6), DeviceProtocol::Chroma);
    }
}
