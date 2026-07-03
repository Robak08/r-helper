mod device;
mod packet;
mod pids;
mod protocol;
mod transport;

pub use device::{
    hid_debug, probe_headset_battery, ranked_entries_for_pid, select_headset_entries,
    HeadsetBatteryManager, HeadsetSession,
};
pub use pids::{device_protocol, headset_profile, is_modern_headset_pid, DeviceProtocol, HeadsetProfile};
