use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use crate::activity_gate::ActivityGate;
use crate::session_lock::is_session_locked;
use crate::worker::StopSignal;

const MONITOR_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
pub struct LifecycleState {
    pub locked: AtomicBool,
    pub suspended: AtomicBool,
    pub pending_restore: AtomicBool,
}

impl LifecycleState {
    pub fn should_pause_activity(&self) -> bool {
        self.locked.load(Ordering::Relaxed) || self.suspended.load(Ordering::Relaxed)
    }
}

pub fn spawn_lifecycle_monitor(
    lifecycle: Arc<LifecycleState>,
    activity_gate: Arc<ActivityGate>,
    session_locked: Arc<crate::session_lock::SessionState>,
    stop: Arc<StopSignal>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("lifecycle-monitor".into())
        .spawn(move || {
            let mut was_locked = is_session_locked();
            session_locked.locked.store(was_locked, Ordering::Relaxed);
            lifecycle.locked.store(was_locked, Ordering::Relaxed);
            sync_gate(&lifecycle, &activity_gate);

            while !stop.wait(MONITOR_INTERVAL) {
                let locked = is_session_locked();
                if locked != was_locked {
                    was_locked = locked;
                    session_locked.locked.store(locked, Ordering::Relaxed);
                    lifecycle.locked.store(locked, Ordering::Relaxed);
                    if !locked {
                        lifecycle.pending_restore.store(true, Ordering::Relaxed);
                    }
                    sync_gate(&lifecycle, &activity_gate);
                }
            }
        })
        .expect("lifecycle monitor thread")
}

#[cfg(windows)]
pub fn spawn_power_suspend_monitor(
    lifecycle: Arc<LifecycleState>,
    activity_gate: Arc<ActivityGate>,
    stop: Arc<StopSignal>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("power-suspend-monitor".into())
        .spawn(move || run_power_message_loop(lifecycle, activity_gate, stop))
        .expect("power suspend monitor thread")
}

#[cfg(not(windows))]
pub fn spawn_power_suspend_monitor(
    _lifecycle: Arc<LifecycleState>,
    _activity_gate: Arc<ActivityGate>,
    _stop: Arc<StopSignal>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("power-suspend-monitor".into())
        .spawn(|| {})
        .expect("power suspend monitor stub")
}

pub(crate) fn sync_gate(lifecycle: &LifecycleState, gate: &ActivityGate) {
    gate.set_active(!lifecycle.should_pause_activity());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_pauses_when_locked_or_suspended() {
        let lifecycle = LifecycleState::default();
        let gate = ActivityGate::new_active();
        sync_gate(&lifecycle, &gate);
        assert!(gate.is_active());

        lifecycle.locked.store(true, Ordering::Relaxed);
        sync_gate(&lifecycle, &gate);
        assert!(!gate.is_active());

        lifecycle.locked.store(false, Ordering::Relaxed);
        lifecycle.suspended.store(true, Ordering::Relaxed);
        sync_gate(&lifecycle, &gate);
        assert!(!gate.is_active());
    }
}

#[cfg(windows)]
fn run_power_message_loop(
    lifecycle: Arc<LifecycleState>,
    activity_gate: Arc<ActivityGate>,
    stop: Arc<StopSignal>,
) {
    use std::sync::OnceLock;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DispatchMessageW, GWLP_USERDATA, MSG,
        PM_REMOVE, PeekMessageW, RegisterClassW, SetWindowLongPtrW, TranslateMessage, WM_NCCREATE,
        WM_POWERBROADCAST, WNDCLASSW,
    };
    use windows::core::PCWSTR;

    const PBT_APMSUSPEND: u32 = 0x0004;
    const PBT_APMRESUMEAUTOMATIC: u32 = 0x0012;

    static CTX: OnceLock<(Arc<LifecycleState>, Arc<ActivityGate>)> = OnceLock::new();
    let _ = CTX.set((Arc::clone(&lifecycle), Arc::clone(&activity_gate)));

    const CLASS_NAME: [u16; 13] = [
        b'R' as u16,
        b'H' as u16,
        b'e' as u16,
        b'l' as u16,
        b'p' as u16,
        b'e' as u16,
        b'r' as u16,
        b'P' as u16,
        b'o' as u16,
        b'w' as u16,
        b'e' as u16,
        b'r' as u16,
        0,
    ];

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            if msg == WM_NCCREATE {
                let create = lparam.0 as *const CREATESTRUCTW;
                if !create.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*create).lpCreateParams as isize);
                }
                return LRESULT(1);
            }

            if msg == WM_POWERBROADCAST {
                let (lifecycle, gate) = CTX.get().expect("power ctx");
                match wparam.0 as u32 {
                    PBT_APMSUSPEND => {
                        lifecycle.suspended.store(true, Ordering::Relaxed);
                        sync_gate(lifecycle, gate);
                    }
                    PBT_APMRESUMEAUTOMATIC => {
                        lifecycle.suspended.store(false, Ordering::Relaxed);
                        lifecycle.pending_restore.store(true, Ordering::Relaxed);
                        sync_gate(lifecycle, gate);
                    }
                    _ => {}
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }

    unsafe {
        let class = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            lpszClassName: PCWSTR(CLASS_NAME.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&class);

        let hwnd = CreateWindowExW(
            Default::default(),
            PCWSTR(CLASS_NAME.as_ptr()),
            PCWSTR(CLASS_NAME.as_ptr()),
            Default::default(),
            0,
            0,
            0,
            0,
            None,
            None,
            None,
            Some(1 as _),
        )
        .expect("power message window");

        while !stop.is_stopped() {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, Some(hwnd), 0, 0, PM_REMOVE).into() {
                if msg.message == 0 {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            sync_gate(&lifecycle, &activity_gate);
            if stop.wait(Duration::from_millis(250)) {
                break;
            }
        }
    }
}
