use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use librazer::{
    cooling_pad::is_present,
    enumerate::{
        enrich_peripheral_batteries, list_razer_hid_devices, summarize_peripheral_devices,
        RazerDeviceSummary,
    },
    headset::HeadsetBatteryManager,
};

const COOLING_PAD_CHECK_INTERVAL: Duration = Duration::from_secs(2);
const PERIPHERAL_REFRESH_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub enum HidEnumMessage {
    CoolingPadPresent(bool),
    PeripheralDevices(Vec<RazerDeviceSummary>),
}

pub fn spawn_hid_enum_poller(tx: Sender<HidEnumMessage>) {
    std::thread::spawn(move || {
        let mut last_peripheral_refresh = Instant::now()
            .checked_sub(PERIPHERAL_REFRESH_INTERVAL)
            .unwrap_or_else(Instant::now);
        let mut last_pad_present = None;
        let mut headset_manager = HeadsetBatteryManager::new();

        loop {
            std::thread::sleep(COOLING_PAD_CHECK_INTERVAL);

            let pad_present = is_present();
            if last_pad_present != Some(pad_present) {
                last_pad_present = Some(pad_present);
                if tx
                    .send(HidEnumMessage::CoolingPadPresent(pad_present))
                    .is_err()
                {
                    break;
                }
            }

            if let Ok(entries) = list_razer_hid_devices() {
                headset_manager.tick(&entries);
            }

            if last_peripheral_refresh.elapsed() >= PERIPHERAL_REFRESH_INTERVAL {
                last_peripheral_refresh = Instant::now();
                if let Ok(entries) = list_razer_hid_devices() {
                    let mut summaries = summarize_peripheral_devices(&entries);
                    enrich_peripheral_batteries(&entries, &mut summaries, &mut headset_manager);
                    if tx
                        .send(HidEnumMessage::PeripheralDevices(summaries))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
}
