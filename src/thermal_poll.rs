use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use crate::system::thermal::{
    CpuTempSource, ThermalReader, ThermalSnapshot, ThermalSpikeFilterState,
    filter_thermal_raw_snapshot,
};
use crate::worker::StopSignal;

const FAST_POLL_INTERVAL: Duration = Duration::from_secs(2);
const SLOW_POLL_INTERVAL: Duration = Duration::from_secs(10);

pub fn spawn_thermal_poller(
    poll_slow: Arc<AtomicBool>,
    shared: Arc<Mutex<ThermalSnapshot>>,
    stop: Arc<StopSignal>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("thermal-poll".into())
        .spawn(move || {
            #[cfg(target_os = "windows")]
            let mut reader = ThermalReader::new();
            let mut spike_state = ThermalSpikeFilterState::default();
            let mut last_cpu_source: Option<CpuTempSource> = None;
            while !stop.is_stopped() {
                let interval = if poll_slow.load(Ordering::Relaxed) {
                    SLOW_POLL_INTERVAL
                } else {
                    FAST_POLL_INTERVAL
                };
                if stop.wait(interval) {
                    break;
                }

                #[cfg(target_os = "windows")]
                let raw = reader.read_snapshot();
                #[cfg(not(target_os = "windows"))]
                let raw = crate::system::thermal::ThermalRawSnapshot {
                    snapshot: ThermalSnapshot::default(),
                    cpu_source: None,
                };

                let mut guard = match shared.lock() {
                    Ok(guard) => guard,
                    Err(_) => continue,
                };
                let filtered = filter_thermal_raw_snapshot(
                    &guard,
                    raw,
                    &mut spike_state,
                    &mut last_cpu_source,
                );
                *guard = filtered;
            }
        })
        .expect("thermal poller thread")
}

pub fn read_shared_thermal(shared: &Arc<Mutex<ThermalSnapshot>>) -> ThermalSnapshot {
    shared.lock().map(|guard| *guard).unwrap_or_default()
}
