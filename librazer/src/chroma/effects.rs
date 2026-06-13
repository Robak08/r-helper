use crate::chroma::packet::{build_request, xor_crc, REPORT_LEN};

pub const VARSTORE: u8 = 0x01;
pub const ZERO_LED: u8 = 0x00;

pub const TRANSACTION_ID_COOLING_PAD: u8 = 0x1F;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const RAZER_GREEN: Self = Self { r: 0x00, g: 0xFF, b: 0x00 };
}

fn finalize(mut report: [u8; REPORT_LEN]) -> [u8; REPORT_LEN] {
    report[88] = xor_crc(&report);
    report
}

fn extended_matrix_effect_base(
    transaction_id: u8,
    arg_size: u8,
    variable_storage: u8,
    led_id: u8,
    effect_id: u8,
) -> [u8; REPORT_LEN] {
    let mut args = [0u8; 80];
    args[0] = variable_storage;
    args[1] = led_id;
    args[2] = effect_id;
    let mut report = build_request(transaction_id, 0x0F, 0x02, &args[..arg_size as usize]);
    report[5] = arg_size;
    finalize(report)
}

pub fn build_extended_none(transaction_id: u8, variable_storage: u8, led_id: u8) -> [u8; REPORT_LEN] {
    extended_matrix_effect_base(transaction_id, 0x06, variable_storage, led_id, 0x00)
}

pub fn build_extended_static(
    transaction_id: u8,
    variable_storage: u8,
    led_id: u8,
    rgb: Rgb,
) -> [u8; REPORT_LEN] {
    let mut report = extended_matrix_effect_base(transaction_id, 0x09, variable_storage, led_id, 0x01);
    report[8 + 5] = 0x01;
    report[8 + 6] = rgb.r;
    report[8 + 7] = rgb.g;
    report[8 + 8] = rgb.b;
    finalize(report)
}

pub fn build_extended_breathing_random(
    transaction_id: u8,
    variable_storage: u8,
    led_id: u8,
) -> [u8; REPORT_LEN] {
    extended_matrix_effect_base(transaction_id, 0x06, variable_storage, led_id, 0x02)
}

pub fn build_extended_breathing_single(
    transaction_id: u8,
    variable_storage: u8,
    led_id: u8,
    rgb: Rgb,
) -> [u8; REPORT_LEN] {
    let mut report = extended_matrix_effect_base(transaction_id, 0x09, variable_storage, led_id, 0x02);
    report[8 + 3] = 0x01;
    report[8 + 5] = 0x01;
    report[8 + 6] = rgb.r;
    report[8 + 7] = rgb.g;
    report[8 + 8] = rgb.b;
    finalize(report)
}

pub fn build_extended_brightness(
    transaction_id: u8,
    variable_storage: u8,
    led_id: u8,
    brightness: u8,
) -> [u8; REPORT_LEN] {
    let args = [variable_storage, led_id, brightness];
    finalize(build_request(transaction_id, 0x0F, 0x04, &args))
}

pub fn build_extended_get_brightness(transaction_id: u8, variable_storage: u8, led_id: u8) -> [u8; REPORT_LEN] {
    let args = [variable_storage, led_id];
    finalize(build_request(transaction_id, 0x0F, 0x84, &args))
}
