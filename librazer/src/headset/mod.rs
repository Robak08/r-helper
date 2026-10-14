mod device;
mod packet;
mod pids;
mod protocol;
mod transport;

pub use device::{
    HeadsetBatteryManager, HeadsetSession, hid_debug, probe_headset_battery,
    ranked_entries_for_pid, select_headset_entries,
};
pub use pids::{
    DeviceProtocol, HeadsetProfile, device_protocol, headset_profile, is_modern_headset_pid,
};
