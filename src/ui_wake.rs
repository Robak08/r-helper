use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use crate::session_lock::SessionState;

const WAKE_INTERVAL: Duration = Duration::from_secs(2);

/// Background thread that calls `request_repaint` while desired and session is unlocked.
///
/// eframe/winit often stop driving the event loop for hidden or unfocused windows;
/// this keeps `update()` running so polled state (temps, fan RPM, etc.) reaches the UI.
pub struct RepaintWake {
    ctx: eframe::egui::Context,
    session: Arc<SessionState>,
    desired: AtomicBool,
    running: AtomicBool,
    stopped: AtomicBool,
    handle: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl RepaintWake {
    pub fn new(ctx: eframe::egui::Context, session: Arc<SessionState>) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            session,
            desired: AtomicBool::new(false),
            running: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            handle: std::sync::Mutex::new(None),
        })
    }

    pub fn set_desired(self: &Arc<Self>, desired: bool) {
        self.desired.store(desired, Ordering::Relaxed);
        if desired {
            self.ensure_thread();
        }
    }

    fn ensure_thread(self: &Arc<Self>) {
        if self.stopped.load(Ordering::Acquire) {
            return;
        }
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }

        if let Ok(mut handle) = self.handle.lock() {
            if let Some(previous) = handle.take() {
                let _ = previous.join();
            }
        }

        let wake = Arc::clone(self);
        let handle = std::thread::Builder::new()
            .name("ui-repaint-wake".into())
            .spawn(move || {
                loop {
                    while wake.desired.load(Ordering::Acquire)
                        && !wake.stopped.load(Ordering::Acquire)
                    {
                        if !wake.session.locked.load(Ordering::Relaxed) {
                            wake.ctx.request_repaint();
                        }
                        std::thread::sleep(WAKE_INTERVAL);
                    }

                    wake.running.store(false, Ordering::SeqCst);
                    if wake.desired.load(Ordering::Acquire)
                        && !wake.stopped.load(Ordering::Acquire)
                        && !wake.running.swap(true, Ordering::SeqCst)
                    {
                        continue;
                    }
                    break;
                }
            })
            .expect("ui repaint wake thread");
        if let Ok(mut slot) = self.handle.lock() {
            *slot = Some(handle);
        }
    }

    pub fn stop_and_join(&self) {
        self.stopped.store(true, Ordering::Release);
        self.desired.store(false, Ordering::Release);
        if let Ok(mut handle) = self.handle.lock() {
            if let Some(handle) = handle.take() {
                let _ = handle.join();
            }
        }
    }
}
