use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use crate::system::thermal::{
    filter_thermal_snapshot_spike, ThermalReader, ThermalSnapshot, ThermalSpikeFilterState,
};

const FAST_POLL_INTERVAL: Duration = Duration::from_secs(2);
const SLOW_POLL_INTERVAL: Duration = Duration::from_secs(10);

pub fn spawn_thermal_poller(
    poll_slow: Arc<AtomicBool>,
    shared: Arc<Mutex<ThermalSnapshot>>,
) {
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        let mut reader = ThermalReader::new();
        let mut spike_state = ThermalSpikeFilterState::default();
        loop {
            let interval = if poll_slow.load(Ordering::Relaxed) {
                SLOW_POLL_INTERVAL
            } else {
                FAST_POLL_INTERVAL
            };
            std::thread::sleep(interval);

            #[cfg(target_os = "windows")]
            let raw = reader.read_snapshot();
            #[cfg(not(target_os = "windows"))]
            let raw = ThermalSnapshot::default();

            let mut guard = match shared.lock() {
                Ok(guard) => guard,
                Err(_) => continue,
            };
            let filtered = filter_thermal_snapshot_spike(&guard, raw, &mut spike_state);
            *guard = filtered;
        }
    });
}

pub fn read_shared_thermal(shared: &Arc<Mutex<ThermalSnapshot>>) -> ThermalSnapshot {
    shared
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default()
}
