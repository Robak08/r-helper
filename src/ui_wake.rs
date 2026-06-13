use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

const WAKE_INTERVAL: Duration = Duration::from_secs(2);

/// Background thread that calls `request_repaint` while active.
///
/// eframe/winit often stop driving the event loop for hidden or unfocused windows;
/// this keeps `update()` running so polled state (temps, fan RPM, etc.) reaches the UI.
pub struct RepaintWake {
    ctx: eframe::egui::Context,
    active: AtomicBool,
    running: AtomicBool,
}

impl RepaintWake {
    pub fn new(ctx: eframe::egui::Context) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            active: AtomicBool::new(false),
            running: AtomicBool::new(false),
        })
    }

    pub fn set_active(self: &Arc<Self>, active: bool) {
        self.active.store(active, Ordering::Relaxed);
        if active {
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
                while wake.active.load(Ordering::Relaxed) {
                    wake.ctx.request_repaint();
                    std::thread::sleep(WAKE_INTERVAL);
                }
                wake.running.store(false, Ordering::SeqCst);
            })
            .expect("ui repaint wake thread");
    }
}
