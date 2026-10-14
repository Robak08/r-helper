use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::worker::StopSignal;

/// When inactive (session locked or system suspended), background workers should idle
/// without performing HID/API I/O.
#[derive(Debug, Default)]
pub struct ActivityGate {
    active: AtomicBool,
    lock: Mutex<()>,
    wake: Condvar,
}

impl ActivityGate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn new_active() -> Arc<Self> {
        let gate = Self::new();
        gate.active.store(true, Ordering::Release);
        gate
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    pub fn set_active(&self, active: bool) {
        let prev = self.is_active();
        self.active.store(active, Ordering::Release);
        if prev != active {
            self.wake.notify_all();
        }
    }

    /// Wait up to `duration`, returning early when `stop` is set or activity resumes.
    pub fn wait_for_active_or_stop(
        &self,
        stop: &StopSignal,
        duration: std::time::Duration,
    ) -> bool {
        if stop.is_stopped() {
            return true;
        }
        if self.is_active() {
            return stop.wait(duration);
        }

        let guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let (guard, _) = self
            .wake
            .wait_timeout_while(guard, duration, |_| !stop.is_stopped() && !self.is_active())
            .unwrap_or_else(|e| e.into_inner());
        drop(guard);
        stop.is_stopped()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn inactive_gate_blocks_until_active() {
        let gate = ActivityGate::new();
        let stop = StopSignal::new();
        assert!(!gate.wait_for_active_or_stop(&stop, Duration::from_millis(20)));
        gate.set_active(true);
        assert!(!gate.wait_for_active_or_stop(&stop, Duration::from_millis(20)));
    }
}
