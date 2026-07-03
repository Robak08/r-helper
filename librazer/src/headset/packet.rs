/// Total size of modern headset HID output reports (report ID included).
pub const REPORT_LEN: usize = 64;

const HEADER: [u8; 8] = [0x00, 0x60, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00];

pub const CMD_CLASS_INIT: u8 = 0x02;
pub const CMD_CLASS_BATTERY: u8 = 0x21;
pub const CMD_CLASS_CHARGING: u8 = 0x2a;

const PREFIX_SET: u8 = 0x00;
const PREFIX_GET: u8 = 0x80;

/// XOR of bytes 0..=61 into byte 62 (OpenRazer / reverse-engineered convention).
pub fn xor_checksum(report: &[u8; REPORT_LEN]) -> u8 {
    report[0..62].iter().fold(0u8, |acc, b| acc ^ b)
}

/// Build a 64-byte report ID 0x02 packet with checksum.
pub fn build_report(get: bool, command_class: u8) -> [u8; REPORT_LEN] {
    let mut report = [0u8; REPORT_LEN];
    report[0] = 0x02;
    report[1..9].copy_from_slice(&HEADER);
    report[9] = if get { PREFIX_GET } else { PREFIX_SET };
    report[10] = command_class;
    report[62] = xor_checksum(&report);
    report
}

pub const RF_WAKE_REPORT: [u8; 2] = [0x05, 0x00];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step1_checksum_matches_capture() {
        let report = build_report(false, CMD_CLASS_INIT);
        assert_eq!(report[62], 0x64);
    }

    #[test]
    fn step2_checksum_matches_capture() {
        let report = build_report(false, CMD_CLASS_CHARGING);
        assert_eq!(report[62], 0x4c);
    }

    #[test]
    fn status_query_checksum_matches_capture() {
        let report = build_report(true, CMD_CLASS_CHARGING);
        assert_eq!(report[62], 0xcc);
    }

    #[test]
    fn battery_query_checksum_matches_capture() {
        let report = build_report(true, CMD_CLASS_BATTERY);
        assert_eq!(report[62], 0xc7);
    }
}
