use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

/// Cooperative, interruptible stop signal for background workers.
#[derive(Debug, Default)]
pub struct StopSignal {
    stopped: AtomicBool,
    lock: Mutex<()>,
    wake: Condvar,
}

impl StopSignal {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        self.wake.notify_all();
    }

    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    /// Wait for `duration`; returns `true` when stop was requested.
    pub fn wait(&self, duration: Duration) -> bool {
        if self.is_stopped() {
            return true;
        }

        let guard = self.lock.lock().unwrap_or_else(|error| error.into_inner());
        let _ = self
            .wake
            .wait_timeout_while(guard, duration, |_| !self.is_stopped())
            .unwrap_or_else(|error| error.into_inner());
        self.is_stopped()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_interrupts_wait() {
        let signal = StopSignal::new();
        signal.stop();
        assert!(signal.wait(Duration::from_secs(10)));
    }
}
