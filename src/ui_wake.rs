use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
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
}

impl RepaintWake {
    pub fn new(ctx: eframe::egui::Context, session: Arc<SessionState>) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            session,
            desired: AtomicBool::new(false),
            running: AtomicBool::new(false),
        })
    }

    pub fn set_desired(self: &Arc<Self>, desired: bool) {
        self.desired.store(desired, Ordering::Relaxed);
        if desired {
            self.ensure_thread();
        }
    }

    fn ensure_thread(self: &Arc<Self>) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }

        let wake = Arc::clone(self);
        std::thread::Builder::new()
            .name("ui-repaint-wake".into())
            .spawn(move || {
                while wake.desired.load(Ordering::Relaxed) {
                    if !wake.session.locked.load(Ordering::Relaxed) {
                        wake.ctx.request_repaint();
                    }
                    std::thread::sleep(WAKE_INTERVAL);
                }
                wake.running.store(false, Ordering::SeqCst);
            })
            .expect("ui repaint wake thread");
    }
}
