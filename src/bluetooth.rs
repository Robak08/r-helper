use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};
use std::time::{Duration, Instant};

const PRESENCE_INTERVAL: Duration = Duration::from_secs(10);
const BATTERY_CACHE_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BluetoothHeadsetSummary {
    pub id: String,
    pub name: String,
    pub battery_percent: Option<u8>,
}

pub fn spawn_bluetooth_headset_poller(
    tx: Sender<Vec<BluetoothHeadsetSummary>>,
    running: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let mut cached_batteries = HashMap::new();
        let mut last_battery_refresh =
            Instant::now().checked_sub(BATTERY_CACHE_INTERVAL).unwrap_or_else(Instant::now);

        while running.load(Ordering::Relaxed) {
            if last_battery_refresh.elapsed() >= BATTERY_CACHE_INTERVAL {
                cached_batteries = collect_pnp_bluetooth_batteries();
                last_battery_refresh = Instant::now();
            }

            let headsets = enumerate_pnp_bluetooth_headphones(&cached_batteries);
            if tx.send(headsets).is_err() {
                break;
            }

            std::thread::sleep(PRESENCE_INTERVAL);
        }
    });
}

#[cfg(not(windows))]
fn enumerate_pnp_bluetooth_headphones(
    _batteries: &HashMap<String, u8>,
) -> Vec<BluetoothHeadsetSummary> {
    Vec::new()
}

#[cfg(not(windows))]
fn collect_pnp_bluetooth_batteries() -> HashMap<String, u8> {
    HashMap::new()
}

#[cfg(windows)]
const BLUETOOTH_DEVICE_CLASS: windows::core::GUID =
    windows::core::GUID::from_u128(0xe0cbf06c_cd8b_4647_bb8a_263b43f0f974);
#[cfg(windows)]
const MEDIA_DEVICE_CLASS: windows::core::GUID =
    windows::core::GUID::from_u128(0x4d36e96c_e325_11ce_bfc1_08002be10318);
#[cfg(windows)]
const SYSTEM_DEVICE_CLASS: windows::core::GUID =
    windows::core::GUID::from_u128(0x4d36e97d_e325_11ce_bfc1_08002be10318);

#[cfg(windows)]
const SCOPED_DEVICE_CLASSES: &[windows::core::GUID] = &[
    BLUETOOTH_DEVICE_CLASS,
    MEDIA_DEVICE_CLASS,
    SYSTEM_DEVICE_CLASS,
];

#[cfg(windows)]
fn bluetooth_battery_property_key() -> windows::Win32::Foundation::DEVPROPKEY {
    windows::Win32::Foundation::DEVPROPKEY {
        fmtid: windows::core::GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddb_f425bbe5),
        pid: 2,
    }
}

#[cfg(windows)]
const BDIF_CONNECTED: u32 = 0x0000_0020;

#[cfg(windows)]
fn bluetooth_device_flags_property_key() -> windows::Win32::Foundation::DEVPROPKEY {
    windows::Win32::Foundation::DEVPROPKEY {
        fmtid: windows::core::GUID::from_u128(0x2bd67d8b_8beb_48d5_87e0_6cda3428040a),
        pid: 3,
    }
}

#[cfg(windows)]
const DISCOVERY_DEVICE_CLASSES: &[windows::core::GUID] = &[BLUETOOTH_DEVICE_CLASS];

#[cfg(windows)]
fn enumerate_pnp_bluetooth_headphones(
    pnp_batteries: &HashMap<String, u8>,
) -> Vec<BluetoothHeadsetSummary> {
    let mut headsets = Vec::new();
    let mut connected_keys = std::collections::HashSet::new();

    collect_connected_media_headphones(&mut headsets, &mut connected_keys);

    for class in DISCOVERY_DEVICE_CLASSES {
        unsafe {
            let Ok(device_info) = enumerate_present_devices_for_class(class) else {
                continue;
            };
            collect_pnp_headphones_from_set(device_info, *class, &mut headsets, &mut connected_keys);
            let _ = windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiDestroyDeviceInfoList(
                device_info,
            );
        }
    }

    enrich_batteries_from_windows(&mut headsets, pnp_batteries);
    headsets.retain(|headset| {
        connected_keys.contains(&normalized_device_name(&headset.name))
    });
    headsets.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    headsets
}

#[cfg(windows)]
fn collect_connected_media_headphones(
    headsets: &mut Vec<BluetoothHeadsetSummary>,
    connected_keys: &mut std::collections::HashSet<String>,
) {
    unsafe {
        let Ok(device_info) = enumerate_present_devices_for_class(&MEDIA_DEVICE_CLASS) else {
            return;
        };
        collect_media_headphones_from_set(device_info, headsets, connected_keys);
        let _ = windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiDestroyDeviceInfoList(
            device_info,
        );
    }
}

#[cfg(windows)]
unsafe fn collect_media_headphones_from_set(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    headsets: &mut Vec<BluetoothHeadsetSummary>,
    connected_keys: &mut std::collections::HashSet<String>,
) {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiEnumDeviceInfo, SP_DEVINFO_DATA,
    };

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
        let Some(device_name) = media_headphone_device_name(&name) else {
            continue;
        };
        if is_excluded_device_name(&device_name) {
            continue;
        }

        let id = unsafe { read_pnp_instance_id(device_info, &device_data) }
            .unwrap_or_else(|| name.clone());
        connected_keys.insert(normalized_device_name(&device_name));

        merge_headset(
            headsets,
            BluetoothHeadsetSummary {
                id,
                name: device_name,
                battery_percent: None,
            },
        );
    }
}

#[cfg(windows)]
unsafe fn enumerate_present_devices_for_class(
    class: &windows::core::GUID,
) -> windows::core::Result<
    windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiGetClassDevsW, DIGCF_PRESENT,
    };

    unsafe { SetupDiGetClassDevsW(Some(class), None, None, DIGCF_PRESENT) }
}

#[cfg(windows)]
fn enrich_batteries_from_windows(
    headsets: &mut [BluetoothHeadsetSummary],
    pnp_batteries: &HashMap<String, u8>,
) {
    for headset in headsets.iter_mut() {
        if headset.battery_percent.is_none() {
            headset.battery_percent = lookup_pnp_battery(&headset.name, pnp_batteries);
        }
    }
}

#[cfg(windows)]
fn collect_pnp_bluetooth_batteries() -> HashMap<String, u8> {
    let mut batteries = HashMap::new();

    for class in SCOPED_DEVICE_CLASSES {
        unsafe {
            let Ok(device_info) = enumerate_present_devices_for_class(class) else {
                continue;
            };
            collect_pnp_batteries_from_set(device_info, &mut batteries);
            let _ = windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiDestroyDeviceInfoList(
                device_info,
            );
        }
    }

    batteries
}

#[cfg(windows)]
unsafe fn collect_pnp_batteries_from_set(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    batteries: &mut HashMap<String, u8>,
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
        let Some(percent) = (unsafe {
            read_pnp_battery_percent(device_info, &device_data, property_key)
        }) else {
            continue;
        };
        let Some(name) = (unsafe { read_pnp_device_name(device_info, &device_data) }) else {
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
unsafe fn collect_pnp_headphones_from_set(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device_class: windows::core::GUID,
    headsets: &mut Vec<BluetoothHeadsetSummary>,
    connected_keys: &mut std::collections::HashSet<String>,
) {
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiEnumDeviceInfo, SP_DEVINFO_DATA,
    };

    let mut device_data = SP_DEVINFO_DATA {
        cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
        ..Default::default()
    };
    let mut index = 0u32;

    while unsafe { SetupDiEnumDeviceInfo(device_info, index, &mut device_data).is_ok() } {
        index += 1;
        if device_class == BLUETOOTH_DEVICE_CLASS
            && !unsafe { is_pnp_bluetooth_connected(device_info, &device_data) }
        {
            continue;
        }
        if !unsafe { is_pnp_headphone_candidate(device_info, &device_data, &device_class) } {
            continue;
        }

        let Some(name) = (unsafe { read_pnp_device_name(device_info, &device_data) }) else {
            continue;
        };
        if is_excluded_device_name(&name) || is_transport_endpoint_name(&name) {
            continue;
        }

        let id = unsafe { read_pnp_instance_id(device_info, &device_data) }
            .unwrap_or_else(|| name.clone());
        connected_keys.insert(normalized_device_name(&name));

        merge_headset(
            headsets,
            BluetoothHeadsetSummary {
                id,
                name,
                battery_percent: None,
            },
        );
    }
}

#[cfg(windows)]
unsafe fn is_pnp_bluetooth_connected(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device_data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
) -> bool {
    let Some(flags) = (unsafe { read_pnp_bluetooth_device_flags(device_info, device_data) }) else {
        return false;
    };
    flags & BDIF_CONNECTED != 0
}

#[cfg(windows)]
unsafe fn read_pnp_bluetooth_device_flags(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device_data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
) -> Option<u32> {
    let property_key = bluetooth_device_flags_property_key();
    unsafe { read_pnp_uint32_property(device_info, device_data, &property_key) }
}

#[cfg(windows)]
unsafe fn is_pnp_headphone_candidate(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device_data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
    device_class: &windows::core::GUID,
) -> bool {
    if *device_class == SYSTEM_DEVICE_CLASS {
        return false;
    }

    if let Some(class_of_device) =
        unsafe { read_pnp_bluetooth_class_of_device(device_info, device_data) }
    {
        let major = ((class_of_device >> 8) & 0x1F) as i32;
        let minor = ((class_of_device >> 2) & 0x3F) as i32;
        if major == 4 && is_headphone_class(major, minor) {
            return true;
        }
        if major == 4 && !is_headphone_class(major, minor) {
            return false;
        }
    }

    if let Some(category) =
        unsafe { read_pnp_container_category(device_info, device_data) }
    {
        let lower = category.to_ascii_lowercase();
        if lower.contains("headset.bluetooth")
            || lower.contains("headphone")
            || lower.contains("earbud")
        {
            return true;
        }
    }

    let Some(name) = (unsafe { read_pnp_device_name(device_info, device_data) }) else {
        return false;
    };

    if *device_class == MEDIA_DEVICE_CLASS {
        return is_media_headphone_endpoint_name(&name);
    }

    looks_like_bluetooth_headphone_name(&name)
}

#[cfg(windows)]
fn bluetooth_class_of_device_property_key() -> windows::Win32::Foundation::DEVPROPKEY {
    windows::Win32::Foundation::DEVPROPKEY {
        fmtid: windows::core::GUID::from_u128(0x2bd67d8b_8beb_48d5_87e0_6cda3428040a),
        pid: 10,
    }
}

#[cfg(windows)]
unsafe fn read_pnp_bluetooth_class_of_device(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device_data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
) -> Option<u32> {
    let property_key = bluetooth_class_of_device_property_key();
    unsafe { read_pnp_uint32_property(device_info, device_data, &property_key) }
}

#[cfg(windows)]
unsafe fn read_pnp_container_category(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device_data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
) -> Option<String> {
    use windows::Win32::Devices::Properties::DEVPKEY_DeviceContainer_Category;

    unsafe { read_pnp_string_property(device_info, device_data, &DEVPKEY_DeviceContainer_Category) }
}

#[cfg(windows)]
unsafe fn read_pnp_instance_id(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device_data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
) -> Option<String> {
    use windows::Win32::Devices::Properties::DEVPKEY_Device_InstanceId;

    unsafe { read_pnp_string_property(device_info, device_data, &DEVPKEY_Device_InstanceId) }
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
unsafe fn read_pnp_uint32_property(
    device_info: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device_data: &windows::Win32::Devices::DeviceAndDriverInstallation::SP_DEVINFO_DATA,
    property_key: &windows::Win32::Foundation::DEVPROPKEY,
) -> Option<u32> {
    use windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDevicePropertyW;
    use windows::Win32::Devices::Properties::{DEVPROP_TYPE_UINT32, DEVPROPTYPE};

    let mut property_type = DEVPROPTYPE::default();
    let mut buffer = [0u8; 4];
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

    match property_type {
        DEVPROP_TYPE_UINT32 if required_size >= 4 => Some(u32::from_le_bytes(buffer)),
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

fn lookup_pnp_battery(name: &str, batteries: &HashMap<String, u8>) -> Option<u8> {
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

fn merge_headset(headsets: &mut Vec<BluetoothHeadsetSummary>, incoming: BluetoothHeadsetSummary) {
    let incoming_key = normalized_device_name(&incoming.name);
    if let Some(existing) =
        headsets.iter_mut().find(|headset| normalized_device_name(&headset.name) == incoming_key)
    {
        if incoming.battery_percent.is_some() {
            existing.battery_percent = incoming.battery_percent;
        }
        existing.name = prefer_display_name(&existing.name, &incoming.name);
        if existing.id.is_empty() {
            existing.id = incoming.id;
        }
        return;
    }
    headsets.push(incoming);
}

fn prefer_display_name(current: &str, incoming: &str) -> String {
    let current_rank = display_name_rank(current);
    let incoming_rank = display_name_rank(incoming);
    if incoming_rank < current_rank
        || (incoming_rank == current_rank && incoming.len() < current.len())
    {
        incoming.to_string()
    } else {
        current.to_string()
    }
}

fn display_name_rank(name: &str) -> u32 {
    let lower = name.to_ascii_lowercase();
    if lower.contains("hands-free") || lower.contains("hands free") {
        return 4;
    }
    if lower.contains("avrcp") {
        return 4;
    }
    if lower.contains("stereo") && !lower.contains("bt") {
        return 3;
    }
    if lower.starts_with("headphones (") || lower.starts_with("headset (") {
        return 2;
    }
    0
}

fn looks_like_bluetooth_headphone_name(name: &str) -> bool {
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
    const EXCLUDED_TERMS: &[&str] = &[
        "speaker",
        "soundbar",
        "stage",
        "mouse",
        "keyboard",
        "controller",
    ];

    let lower = name.to_ascii_lowercase();
    if EXCLUDED_TERMS.iter().any(|term| lower.contains(term)) {
        return false;
    }

    lower.ends_with(" bt")
        || lower.contains(" bt ")
        || HEADPHONE_TERMS.iter().any(|term| lower.contains(term))
}

fn is_media_headphone_endpoint_name(name: &str) -> bool {
    media_headphone_device_name(name).is_some()
}

fn media_headphone_device_name(name: &str) -> Option<String> {
    const PREFIXES: &[&str] = &["Headphones (", "Headset ("];
    for prefix in PREFIXES {
        let Some(remainder) = name.strip_prefix(prefix) else {
            continue;
        };
        let device_name = remainder.strip_suffix(')')?.trim();
        if device_name.is_empty() {
            return None;
        }
        return Some(device_name.to_string());
    }

    let lower = name.to_ascii_lowercase();
    for prefix in ["headphones (", "headset ("] {
        let Some(inner) = lower.strip_prefix(prefix) else {
            continue;
        };
        let inner = inner.strip_suffix(')')?.trim();
        if inner.is_empty() {
            return None;
        }
        let start = prefix.len();
        let end = name.len().saturating_sub(1);
        return Some(name[start..end].trim().to_string());
    }

    None
}

fn is_transport_endpoint_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("avrcp") || lower.contains("hands-free") || lower.contains("hands free")
}

fn looks_like_headphone_name(name: &str) -> bool {
    looks_like_bluetooth_headphone_name(name) || is_media_headphone_endpoint_name(name)
}

fn is_excluded_device_name(name: &str) -> bool {
    const EXCLUDED_TERMS: &[&str] = &[
        "microphone",
        "mic (",
        "soundbar",
        "speaker",
        "stage",
        "mouse",
        "keyboard",
        "controller",
    ];

    let lower = name.to_ascii_lowercase();
    EXCLUDED_TERMS.iter().any(|term| lower.contains(term))
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
        assert!(looks_like_headphone_name("BlackShark V3 BT"));
        assert!(!looks_like_headphone_name("Razer Basilisk V3"));
        assert!(!looks_like_headphone_name("Bluetooth Speaker"));
        assert!(!looks_like_headphone_name("AMD Audio Device"));
        assert!(!looks_like_headphone_name("Creative Stage"));
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
        assert!(looks_like_headphone_name("BlackShark V3 BT"));
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
    fn merges_transport_records_and_keeps_battery() {
        let mut devices = vec![summary("BlackShark V3 BT Hands-Free AG", None)];
        merge_headset(&mut devices, summary("BlackShark V3 BT", Some(93)));

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].name, "BlackShark V3 BT");
        assert_eq!(devices[0].battery_percent, Some(93));
    }

    #[test]
    fn lookup_matches_hands_free_transport_name() {
        let mut batteries = HashMap::new();
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
    fn prefers_primary_bluetooth_name_over_transport() {
        assert_eq!(
            prefer_display_name("BlackShark V3 BT Hands-Free AG", "BlackShark V3 BT"),
            "BlackShark V3 BT"
        );
        assert_eq!(
            prefer_display_name("Headphones (BlackShark V3 BT)", "BlackShark V3 BT"),
            "BlackShark V3 BT"
        );
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
    #[cfg(windows)]
    #[ignore = "manual check for Windows Bluetooth headphone PnP discovery"]
    fn live_enumerate_pnp_bluetooth_headphones() {
        let batteries = collect_pnp_bluetooth_batteries();
        let headsets = enumerate_pnp_bluetooth_headphones(&batteries);
        eprintln!("{headsets:#?}");
        assert!(
            headsets.iter().any(|headset| headset.name.contains("BlackShark")),
            "expected BlackShark headset via scoped PnP enumeration"
        );
    }

    #[test]
    fn extracts_device_name_from_media_endpoint() {
        assert_eq!(
            media_headphone_device_name("Headphones (BlackShark V3 BT)"),
            Some("BlackShark V3 BT".to_string())
        );
        assert_eq!(
            media_headphone_device_name("Headset (SOUNDPEATS Q40 HD)"),
            Some("SOUNDPEATS Q40 HD".to_string())
        );
        assert_eq!(media_headphone_device_name("Speakers (Creative Stage)"), None);
    }

    #[test]
    #[cfg(windows)]
    fn paired_only_device_without_connection_flag_is_not_connected() {
        assert_eq!(BDIF_CONNECTED, 0x20);
        assert_eq!(0x08 & BDIF_CONNECTED, 0); // paired only
        assert_ne!(0x28 & BDIF_CONNECTED, 0); // paired + connected
    }

    #[test]
    fn clamps_invalid_battery_values() {
        assert_eq!(clamp_battery_percent(72), 72);
        assert_eq!(clamp_battery_percent(255), 100);
    }
}
