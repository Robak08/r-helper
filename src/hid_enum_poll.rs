use std::sync::Arc;
use std::sync::mpsc::{SyncSender, TrySendError};
use std::time::{Duration, Instant};

use librazer::{
    cooling_pad::COOLING_PAD_PID,
    enumerate::{
        RAZER_VID, RazerDeviceSummary, enrich_peripheral_batteries, list_razer_hid_devices,
        summarize_peripheral_devices,
    },
    headset::HeadsetBatteryManager,
};

use crate::worker::StopSignal;

const COOLING_PAD_CHECK_INTERVAL: Duration = Duration::from_secs(2);
const PERIPHERAL_REFRESH_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub enum HidEnumMessage {
    CoolingPadPresent(bool),
    PeripheralDevices(Vec<RazerDeviceSummary>),
}

pub fn spawn_hid_enum_poller(
    tx: SyncSender<HidEnumMessage>,
    stop: Arc<StopSignal>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("hid-enum-poll".into())
        .spawn(move || {
            let mut last_peripheral_refresh = Instant::now()
                .checked_sub(PERIPHERAL_REFRESH_INTERVAL)
                .unwrap_or_else(Instant::now);
            let mut last_pad_present = None;
            let mut headset_manager = HeadsetBatteryManager::new();

            while !stop.wait(COOLING_PAD_CHECK_INTERVAL) {
                let Ok(entries) = list_razer_hid_devices() else {
                    continue;
                };

                let pad_present = entries
                    .iter()
                    .any(|entry| entry.vid == RAZER_VID && entry.pid == COOLING_PAD_PID);
                if last_pad_present != Some(pad_present) {
                    match tx.try_send(HidEnumMessage::CoolingPadPresent(pad_present)) {
                        Ok(()) => last_pad_present = Some(pad_present),
                        Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => break,
                    }
                }

                headset_manager.tick(&entries);

                if last_peripheral_refresh.elapsed() >= PERIPHERAL_REFRESH_INTERVAL {
                    let mut summaries = summarize_peripheral_devices(&entries);
                    enrich_peripheral_batteries(&entries, &mut summaries, &mut headset_manager);
                    match tx.try_send(HidEnumMessage::PeripheralDevices(summaries)) {
                        Ok(()) => last_peripheral_refresh = Instant::now(),
                        Err(TrySendError::Full(_)) => {}
                        Err(TrySendError::Disconnected(_)) => break,
                    }
                }
            }
        })
        .expect("HID enumeration poller thread")
}
