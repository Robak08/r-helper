use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};
use std::time::Duration;

use crate::system::thermal::{ThermalReader, ThermalSnapshot};

const FAST_POLL_INTERVAL: Duration = Duration::from_secs(2);
const SLOW_POLL_INTERVAL: Duration = Duration::from_secs(10);

pub fn spawn_thermal_poller(tx: Sender<ThermalSnapshot>, poll_slow: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        let mut reader = ThermalReader::new();
        loop {
            let interval = if poll_slow.load(Ordering::Relaxed) {
                SLOW_POLL_INTERVAL
            } else {
                FAST_POLL_INTERVAL
            };
            std::thread::sleep(interval);

            #[cfg(target_os = "windows")]
            let snapshot = reader.read_snapshot();
            #[cfg(not(target_os = "windows"))]
            let snapshot = ThermalSnapshot::default();

            if tx.send(snapshot).is_err() {
                break;
            }
        }
    });
}
