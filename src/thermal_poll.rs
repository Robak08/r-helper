use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};
use std::time::Duration;

use crate::system::thermal::{read_snapshot, ThermalSnapshot};

const FAST_POLL_INTERVAL: Duration = Duration::from_secs(2);
const SLOW_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub fn spawn_thermal_poller(tx: Sender<ThermalSnapshot>, poll_slow: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        loop {
            let interval = if poll_slow.load(Ordering::Relaxed) {
                SLOW_POLL_INTERVAL
            } else {
                FAST_POLL_INTERVAL
            };
            std::thread::sleep(interval);

            let snapshot = read_snapshot();
            if tx.send(snapshot).is_err() {
                break;
            }
        }
    });
}
