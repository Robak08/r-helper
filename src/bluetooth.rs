use std::sync::mpsc::Sender;
use std::time::Duration;

const REFRESH_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BluetoothHeadsetSummary {
    pub id: String,
    pub name: String,
    pub battery_percent: Option<u8>,
}

pub fn spawn_bluetooth_headset_poller(tx: Sender<Vec<BluetoothHeadsetSummary>>) {
    std::thread::spawn(move || {
        #[cfg(windows)]
        let Some(_winrt_apartment) = WinRtApartment::initialize() else {
            let _ = tx.send(Vec::new());
            return;
        };

        loop {
            let headsets = enumerate_connected_bluetooth_headsets();
            if tx.send(headsets).is_err() {
                break;
            }
            std::thread::sleep(REFRESH_INTERVAL);
        }
    });
}

#[cfg(windows)]
struct WinRtApartment;

#[cfg(windows)]
impl WinRtApartment {
    fn initialize() -> Option<Self> {
        use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

        // This poller owns its thread, so it also owns the WinRT apartment lifecycle.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED).ok().map(|_| Self) }
    }
}

#[cfg(windows)]
impl Drop for WinRtApartment {
    fn drop(&mut self) {
        unsafe { windows::Win32::System::WinRT::RoUninitialize() };
    }
}

#[cfg(not(windows))]
fn enumerate_connected_bluetooth_headsets() -> Vec<BluetoothHeadsetSummary> {
    Vec::new()
}

#[cfg(windows)]
fn bluetooth_battery_property_key() -> windows::Win32::Foundation::DEVPROPKEY {
    windows::Win32::Foundation::DEVPROPKEY {
        fmtid: windows::core::GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddb_f425bbe5),
        pid: 2,
    }
}

#[cfg(windows)]
fn enumerate_connected_bluetooth_headsets() -> Vec<BluetoothHeadsetSummary> {
    use windows::Devices::Bluetooth::{
        BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice, BluetoothMajorClass,
    };
    use windows::Devices::Enumeration::DeviceInformation;

    let mut headsets = Vec::new();
    let pnp_batteries = collect_pnp_bluetooth_batteries();

    if let Ok(selector) =
        BluetoothDevice::GetDeviceSelectorFromConnectionStatus(BluetoothConnectionStatus::Connected)
    {
        if let Ok(operation) = DeviceInformation::FindAllAsyncAqsFilter(&selector) {
            if let Ok(devices) = operation.join() {
                for index in 0..devices.Size().unwrap_or(0) {
                    let Ok(info) = devices.GetAt(index) else {
                        continue;
                    };
                    let Ok(name) = info.Name().map(|value| value.to_string()) else {
                        continue;
                    };

                    let Ok(id) = info.Id() else {
                        continue;
                    };
                    let Ok(operation) = BluetoothDevice::FromIdAsync(&id) else {
                        continue;
                    };
                    let Ok(device) = operation.join() else {
                        continue;
                    };
                    let Ok(class) = device.ClassOfDevice() else {
                        continue;
                    };
                    let Ok(major) = class.MajorClass() else {
                        continue;
                    };
                    let Ok(minor) = class.MinorClass() else {
                        continue;
                    };
                    if major != BluetoothMajorClass::AudioVideo
                        || !is_headphone_class(major.0, minor.0)
                    {
                        continue;
                    }

                    let battery_percent = lookup_pnp_battery(&name, &pnp_batteries);
                    merge_headset(
                        &mut headsets,
                        BluetoothHeadsetSummary {
                            id: id.to_string(),
                            name,
                            battery_percent,
                        },
                    );
                }
            }
        }
    }

    if let Ok(selector) = BluetoothLEDevice::GetDeviceSelectorFromConnectionStatus(
        BluetoothConnectionStatus::Connected,
    ) {
        if let Ok(operation) = DeviceInformation::FindAllAsyncAqsFilter(&selector) {
            if let Ok(devices) = operation.join() {
                for index in 0..devices.Size().unwrap_or(0) {
                    let Ok(info) = devices.GetAt(index) else {
                        continue;
                    };
                    let Ok(name) = info.Name().map(|value| value.to_string()) else {
                        continue;
                    };

                    let key = normalized_device_name(&name);
                    let matches_classic =
                        headsets.iter().any(|headset| normalized_device_name(&headset.name) == key);
                    if !matches_classic && !looks_like_headphone_name(&name) {
                        continue;
                    }

                    let Ok(id) = info.Id() else {
                        continue;
                    };
                    let Ok(operation) = BluetoothLEDevice::FromIdAsync(&id) else {
                        continue;
                    };
                    let Ok(device) = operation.join() else {
                        continue;
                    };
                    let battery_percent = read_standard_battery_level(&device)
                        .or_else(|| lookup_pnp_battery(&name, &pnp_batteries));

                    merge_headset(
                        &mut headsets,
                        BluetoothHeadsetSummary { id: id.to_string(), name, battery_percent },
                    );
                }
            }
        }
    }

    enrich_batteries_from_windows(&mut headsets, &pnp_batteries);
    headsets.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    headsets
}

#[cfg(windows)]
fn enrich_batteries_from_windows(
    headsets: &mut [BluetoothHeadsetSummary],
    pnp_batteries: &std::collections::HashMap<String, u8>,
) {
    for headset in headsets.iter_mut() {
        if headset.battery_percent.is_none() {
            headset.battery_percent = lookup_pnp_battery(&headset.name, pnp_batteries);
        }
    }
}

#[cfg(windows)]
fn collect_pnp_bluetooth_batteries() -> std::collections::HashMap<String, u8> {
    use std::collections::HashMap;

    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiGetClassDevsW, DIGCF_ALLCLASSES, DIGCF_PRESENT,
    };

    let mut batteries = HashMap::new();

    unsafe {
        let Ok(device_info) =
            SetupDiGetClassDevsW(None, None, None, DIGCF_PRESENT | DIGCF_ALLCLASSES)
        else {
            return batteries;
        };

        collect_pnp_batteries_from_set(device_info, &mut batteries);
        let _ = SetupDiDestroyDeviceInfoList(device_info);
    }

    batteries
}

#[cfg(windows)]
unsafe fn collect_pnp_batteries_from_set(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    batteries: &mut std::collections::HashMap<String, u8>,
) {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiEnumDeviceInfo, SP_DEVINFO_DATA,
    };

    let property_key = bluetooth_battery_property_key();
    let mut device_data = SP_DEVINFO_DATA {
        cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
        ..Default::default()
    };
    let mut index = 0u32;

    while unsafe { SetupDiEnumDeviceInfo(device_info, index, &mut device_data).is_ok() } {
        index += 1;
        let Some(name) = (unsafe { read_pnp_device_name(device_info, &device_data) }) else {
            continue;
        };
        let Some(percent) = (unsafe {
            read_pnp_battery_percent(device_info, &device_data, property_key)
        }) else {
            continue;
        };

        let key = normalized_device_name(&name);
        if key.is_empty() {
            continue;
        }
        batteries.insert(key, percent);
    }
}

#[cfg(windows)]
unsafe fn read_pnp_device_name(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device_data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
) -> Option<String> {
    use windows::Win32::Devices::Properties::DEVPKEY_NAME;

    unsafe { read_pnp_string_property(device_info, device_data, &DEVPKEY_NAME) }
}

#[cfg(windows)]
unsafe fn read_pnp_battery_percent(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device_data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
    property_key: windows::Win32::Foundation::DEVPROPKEY,
) -> Option<u8> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDevicePropertyW;
    use windows::Win32::Devices::Properties::{DEVPROP_TYPE_BYTE, DEVPROP_TYPE_UINT32, DEVPROPTYPE};

    let mut property_type = DEVPROPTYPE::default();
    let mut buffer = [0u8; 4];
    let mut required_size = 0u32;

    unsafe {
        SetupDiGetDevicePropertyW(
            device_info,
            device_data,
            &property_key,
            &mut property_type,
            Some(&mut buffer),
            Some(&mut required_size),
            0,
        )
        .ok()?;
    }

    match property_type {
        DEVPROP_TYPE_BYTE if !buffer.is_empty() => Some(clamp_battery_percent(buffer[0])),
        DEVPROP_TYPE_UINT32 if required_size >= 4 => {
            let value = u32::from_le_bytes(buffer);
            Some(clamp_battery_percent(value.min(100) as u8))
        }
        _ => None,
    }
}

#[cfg(windows)]
unsafe fn read_pnp_string_property(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device_data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
    property_key: &windows::Win32::Foundation::DEVPROPKEY,
) -> Option<String> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDevicePropertyW;
    use windows::Win32::Devices::Properties::DEVPROPTYPE;

    let mut property_type = DEVPROPTYPE::default();
    let mut buffer = [0u8; 512];
    let mut required_size = 0u32;

    unsafe {
        SetupDiGetDevicePropertyW(
            device_info,
            device_data,
            property_key,
            &mut property_type,
            Some(&mut buffer),
            Some(&mut required_size),
            0,
        )
        .ok()?;
    }

    decode_utf16_property(&buffer, required_size as usize)
}

#[cfg(windows)]
fn decode_utf16_property(buffer: &[u8], byte_len: usize) -> Option<String> {
    let wide = buffer
        .chunks_exact(2)
        .take(byte_len / 2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .take_while(|code_unit| *code_unit != 0)
        .collect::<Vec<_>>();
    String::from_utf16(&wide).ok()
}

fn lookup_pnp_battery(name: &str, batteries: &std::collections::HashMap<String, u8>) -> Option<u8> {
    let key = normalized_device_name(name);
    if key.is_empty() {
        return None;
    }
    if let Some(percent) = batteries.get(&key) {
        return Some(*percent);
    }

    batteries
        .iter()
        .find_map(|(candidate, percent)| pnp_names_relate(&key, candidate).then_some(*percent))
}

fn pnp_names_relate(left: &str, right: &str) -> bool {
    left == right
        || (left.len() >= 6 && right.starts_with(left))
        || (right.len() >= 6 && left.starts_with(right))
}

#[cfg(windows)]
fn read_standard_battery_level(
    device: &windows::Devices::Bluetooth::BluetoothLEDevice,
) -> Option<u8> {
    use windows::Devices::Bluetooth::GenericAttributeProfile::{
        GattCharacteristicUuids, GattCommunicationStatus, GattServiceUuids,
    };
    use windows::Storage::Streams::DataReader;

    let battery_service = GattServiceUuids::Battery().ok()?;
    let result = device.GetGattServicesForUuidAsync(battery_service).ok()?.join().ok()?;
    if result.Status().ok()? != GattCommunicationStatus::Success {
        return None;
    }

    let services = result.Services().ok()?;
    let battery_characteristic = GattCharacteristicUuids::BatteryLevel().ok()?;
    for index in 0..services.Size().ok()? {
        let service = services.GetAt(index).ok()?;
        let result =
            service.GetCharacteristicsForUuidAsync(battery_characteristic).ok()?.join().ok()?;
        if result.Status().ok()? != GattCommunicationStatus::Success {
            continue;
        }

        let characteristics = result.Characteristics().ok()?;
        for characteristic_index in 0..characteristics.Size().ok()? {
            let characteristic = characteristics.GetAt(characteristic_index).ok()?;
            let result = characteristic.ReadValueAsync().ok()?.join().ok()?;
            if result.Status().ok()? != GattCommunicationStatus::Success {
                continue;
            }

            let reader = DataReader::FromBuffer(&result.Value().ok()?).ok()?;
            if reader.UnconsumedBufferLength().ok()? == 0 {
                continue;
            }
            return Some(clamp_battery_percent(reader.ReadByte().ok()?));
        }
    }

    None
}

fn merge_headset(headsets: &mut Vec<BluetoothHeadsetSummary>, incoming: BluetoothHeadsetSummary) {
    let incoming_key = normalized_device_name(&incoming.name);
    if let Some(existing) =
        headsets.iter_mut().find(|headset| normalized_device_name(&headset.name) == incoming_key)
    {
        if incoming.battery_percent.is_some() {
            existing.battery_percent = incoming.battery_percent;
        }
        if incoming.name.len() > existing.name.len() {
            existing.name = incoming.name;
        }
        return;
    }
    headsets.push(incoming);
}

fn looks_like_headphone_name(name: &str) -> bool {
    const HEADPHONE_TERMS: &[&str] = &[
        "headset",
        "headphone",
        "earbud",
        "earphone",
        "in-ear",
        "inear",
        "buds",
        "airpods",
        "handsfree",
        "hands-free",
    ];
    const EXCLUDED_TERMS: &[&str] = &["speaker", "soundbar", "mouse", "keyboard", "controller"];

    let lower = name.to_ascii_lowercase();
    if EXCLUDED_TERMS.iter().any(|term| lower.contains(term)) {
        return false;
    }

    lower.contains("audio")
        || HEADPHONE_TERMS.iter().any(|term| lower.contains(term))
}

fn is_headphone_class(major: i32, minor: i32) -> bool {
    // Bluetooth Audio/Video major class, restricted to portable/head-worn audio subclasses.
    major == 4 && matches!(minor, 0 | 1 | 2 | 6 | 7 | 10 | 18)
}

pub(crate) fn normalized_device_name(name: &str) -> String {
    const NOISE_WORDS: &[&str] = &[
        "ag",
        "audio",
        "razer",
        "bluetooth",
        "bt",
        "handsfree",
        "hands",
        "free",
        "le",
        "stereo",
    ];

    name.to_ascii_lowercase()
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty() && !NOISE_WORDS.contains(part))
        .collect::<Vec<_>>()
        .join(" ")
}

fn clamp_battery_percent(value: u8) -> u8 {
    value.min(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(name: &str, battery_percent: Option<u8>) -> BluetoothHeadsetSummary {
        BluetoothHeadsetSummary { id: name.to_string(), name: name.to_string(), battery_percent }
    }

    #[test]
    fn recognizes_common_headphone_names() {
        assert!(looks_like_headphone_name("Galaxy Buds Pro"));
        assert!(!looks_like_headphone_name("Razer Basilisk V3"));
        assert!(!looks_like_headphone_name("Bluetooth Speaker"));
    }

    #[test]
    fn accepts_headphone_classes_but_rejects_speakers_and_mice() {
        assert!(is_headphone_class(4, 1));
        assert!(is_headphone_class(4, 6));
        assert!(!is_headphone_class(4, 5));
        assert!(!is_headphone_class(5, 1));
    }

    #[test]
    fn blackshark_bt_name_matches_windows_report() {
        assert!(is_headphone_class(4, 1));
    }

    #[test]
    fn normalizes_transport_suffixes() {
        assert_eq!(
            normalized_device_name("Razer Barracuda X (Bluetooth Stereo)"),
            normalized_device_name("Barracuda X BT")
        );
    }

    #[test]
    fn merges_classic_and_le_records_and_keeps_battery() {
        let mut devices = vec![summary("Barracuda X Stereo", None)];
        merge_headset(&mut devices, summary("Barracuda X Bluetooth", Some(73)));

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].battery_percent, Some(73));
    }

    #[test]
    fn lookup_matches_hands_free_transport_name() {
        let mut batteries = std::collections::HashMap::new();
        batteries.insert(
            normalized_device_name("BlackShark V3 BT Hands-Free AG"),
            93,
        );

        assert_eq!(
            lookup_pnp_battery("BlackShark V3 BT", &batteries),
            Some(93)
        );
    }

    #[test]
    fn pnp_name_relation_matches_prefixes() {
        assert!(pnp_names_relate("blackshark v3", "blackshark v3 hands free"));
        assert!(!pnp_names_relate("blackshark v3", "kraken v3"));
    }

    #[test]
    #[cfg(windows)]
    #[ignore = "manual check for Windows Bluetooth battery PnP data"]
    fn live_read_pnp_bluetooth_batteries() {
        let batteries = collect_pnp_bluetooth_batteries();
        eprintln!("{batteries:#?}");
        assert!(
            lookup_pnp_battery("BlackShark V3 BT", &batteries).is_some(),
            "expected BlackShark battery via Windows PnP property"
        );
    }

    #[test]
    fn clamps_invalid_battery_values() {
        assert_eq!(clamp_battery_percent(72), 72);
        assert_eq!(clamp_battery_percent(255), 100);
    }
}
