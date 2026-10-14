//! WinRT Bluetooth enumeration for connected headsets (presence only; no blocking battery probes).

use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothDevice};
use windows::Devices::Enumeration::DeviceInformation;
use windows_future::IAsyncOperation;

use crate::bluetooth::{
    BluetoothHeadsetSummary, canonical_bluetooth_audio_display_name, is_excluded_device_name,
    looks_like_bluetooth_headphone_name, merge_headset,
};
use crate::worker::StopSignal;

pub fn collect_winrt_headset_summaries(stop: &StopSignal) -> Vec<BluetoothHeadsetSummary> {
    if stop.is_stopped() {
        return Vec::new();
    }

    let selector = match BluetoothDevice::GetDeviceSelector() {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };

    let device_infos = match DeviceInformation::FindAllAsyncAqsFilter(&selector) {
        Ok(operation) => match wait_async(operation) {
            Ok(value) => value,
            Err(_) => return Vec::new(),
        },
        Err(_) => return Vec::new(),
    };

    if stop.is_stopped() {
        return Vec::new();
    }

    let mut headsets = Vec::new();

    for device_info in device_infos {
        if stop.is_stopped() {
            break;
        }

        let id = match device_info.Id() {
            Ok(value) => value,
            Err(_) => continue,
        };
        let bt_device = match BluetoothDevice::FromIdAsync(&id) {
            Ok(operation) => match wait_async(operation) {
                Ok(value) => value,
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        if bt_device.ConnectionStatus().ok() != Some(BluetoothConnectionStatus::Connected) {
            continue;
        }

        let name = device_info.Name().map(|h| h.to_string()).unwrap_or_default();
        let display_name = canonical_bluetooth_audio_display_name(&name).unwrap_or(name);
        if display_name.is_empty()
            || is_excluded_device_name(&display_name)
            || !headphone_candidate_name_or_class(&display_name, &bt_device)
        {
            continue;
        }

        merge_headset(
            &mut headsets,
            BluetoothHeadsetSummary { name: display_name, battery_percent: None },
        );
    }

    headsets
}

fn headphone_candidate_name_or_class(name: &str, bt_device: &BluetoothDevice) -> bool {
    if looks_like_bluetooth_headphone_name(name) {
        return true;
    }

    if let Ok(class_of_device) = bt_device.ClassOfDevice() {
        if let Ok(raw) = class_of_device.RawValue() {
            let major = ((raw >> 8) & 0x1F) as i32;
            let minor = ((raw >> 2) & 0x3F) as i32;
            if major == 4 && is_headphone_class(major, minor) {
                return true;
            }
        }
    }

    false
}

fn is_headphone_class(major: i32, minor: i32) -> bool {
    major == 4 && matches!(minor, 0 | 1 | 2 | 6 | 7 | 10 | 18)
}

fn wait_async<T: windows::core::RuntimeType>(
    operation: IAsyncOperation<T>,
) -> windows::core::Result<T> {
    operation.join().map_err(|error| windows::core::Error::from(error))
}
