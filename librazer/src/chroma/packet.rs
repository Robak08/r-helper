/// Razer Chroma 90-byte HID feature report (peripherals).
pub const REPORT_LEN: usize = 90;

const STATUS_NEW: u8 = 0x00;
const STATUS_OK: u8 = 0x02;

pub fn build_request(transaction_id: u8, command_class: u8, command_id: u8, args: &[u8]) -> [u8; REPORT_LEN] {
    let mut report = [0u8; REPORT_LEN];
    report[0] = STATUS_NEW;
    report[1] = transaction_id;
    report[5] = args.len().min(80) as u8;
    report[6] = command_class;
    report[7] = command_id;
    report[8..8 + args.len().min(80)].copy_from_slice(&args[..args.len().min(80)]);
    report[88] = xor_crc(&report);
    report
}

pub fn response_ok(response: &[u8]) -> bool {
    response.len() >= REPORT_LEN && (response[0] == STATUS_OK || response[0] == STATUS_NEW)
}

/// XOR of bytes 2..=87 (OpenRazer convention).
pub fn xor_crc(report: &[u8]) -> u8 {
    report[2..88].iter().fold(0u8, |acc, b| acc ^ b)
}
