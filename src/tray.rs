use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    TrayIcon, TrayIconBuilder, TrayIconEvent,
};

pub struct TraySharedState {
    hwnd: Mutex<Option<isize>>,
    visible: AtomicBool,
    ctx: eframe::egui::Context,
}

impl TraySharedState {
    pub fn new(ctx: eframe::egui::Context) -> Arc<Self> {
        Arc::new(Self {
            hwnd: Mutex::new(None),
            visible: AtomicBool::new(true),
            ctx,
        })
    }

    pub fn set_hwnd(&self, hwnd: isize) {
        *self.hwnd.lock().expect("tray hwnd lock") = Some(hwnd);
    }

    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::Relaxed)
    }

    pub fn show(&self) {
        let hwnd = *self.hwnd.lock().expect("tray hwnd lock");
        if let Some(hwnd) = hwnd {
            show_window(hwnd);
            self.visible.store(true, Ordering::Relaxed);
            self.ctx.request_repaint();
        }
    }

    pub fn hide(self: &Arc<Self>) {
        let hwnd = *self.hwnd.lock().expect("tray hwnd lock");
        if let Some(hwnd) = hwnd {
            hide_window(hwnd);
            self.visible.store(false, Ordering::Relaxed);
        }
    }
}

pub struct TrayHandle {
    _tray: TrayIcon,
}

impl TrayHandle {
    pub fn init(icon: tray_icon::Icon, state: Arc<TraySharedState>) -> Self {
        let show_menu_id = MenuId::new("show");
        let quit_menu_id = MenuId::new("quit");
        let show_item = MenuItem::with_id(show_menu_id.clone(), "Show R-Helper", true, None);
        let quit_item = MenuItem::with_id(quit_menu_id.clone(), "Quit", true, None);
        let menu = Menu::with_items(&[
            &show_item,
            &PredefinedMenuItem::separator(),
            &quit_item,
        ])
        .expect("tray menu");

        let menu_state = Arc::clone(&state);
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == show_menu_id {
                menu_state.show();
            } else if event.id == quit_menu_id {
                std::process::exit(0);
            }
        }));

        let icon_state = Arc::clone(&state);
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            match event {
                TrayIconEvent::DoubleClick { .. } => icon_state.show(),
                TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                } => icon_state.show(),
                _ => {}
            }
        }));

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("R-Helper")
            .with_icon(icon)
            .build()
            .expect("tray icon");

        Self { _tray: tray }
    }
}

#[cfg(windows)]
pub fn hwnd_from_window_handle(
    handle: &dyn raw_window_handle::HasWindowHandle,
) -> Option<isize> {
    use raw_window_handle::RawWindowHandle;

    let raw = handle.window_handle().ok()?.as_raw();
    match raw {
        RawWindowHandle::Win32(win) => Some(win.hwnd.get() as isize),
        _ => None,
    }
}

#[cfg(windows)]
pub fn hide_window(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut _), SW_HIDE);
    }
}

#[cfg(windows)]
pub fn show_window(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_SHOW};

    unsafe {
        let hwnd = HWND(hwnd as *mut _);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Windows taskbar icon comes from the AppUserModelID shortcut association, not egui's
/// RGBA window icon. Load the embedded .ico resource and point the shell at it.
#[cfg(windows)]
pub fn set_windows_taskbar_icon(hwnd: isize) {
    use std::ffi::c_void;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, WPARAM};
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_RelaunchIconResource;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
    use windows::Win32::UI::WindowsAndMessaging::{
        LoadImageW, SendMessageW, GDI_IMAGE_TYPE, ICON_BIG, ICON_SMALL, IMAGE_ICON, IMAGE_FLAGS,
        LR_DEFAULTSIZE, WM_SETICON,
    };

    const APP_ICON_RESOURCE_ID: u16 = 1;

    unsafe {
        let hwnd = HWND(hwnd as *mut c_void);
        let instance = GetModuleHandleW(None).unwrap_or_default().into();
        let resource_name = PCWSTR(APP_ICON_RESOURCE_ID as usize as *const u16);

        let load_icon = |width: i32, height: i32| -> Option<HANDLE> {
            LoadImageW(
                Some(instance),
                resource_name,
                GDI_IMAGE_TYPE(IMAGE_ICON.0),
                width,
                height,
                IMAGE_FLAGS(LR_DEFAULTSIZE.0),
            )
            .ok()
        };

        if let Some(icon) = load_icon(0, 0) {
            let handle = icon.0 as isize;
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(handle)),
            );
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(handle)),
            );
        }

        let mut path = vec![0u16; 512];
        let len = GetModuleFileNameW(Some(instance.into()), &mut path);
        if len == 0 {
            return;
        }

        let exe_path = String::from_utf16_lossy(&path[..len as usize]);
        let icon_resource = format!("{exe_path},0");
        let property_store = SHGetPropertyStoreForWindow::<IPropertyStore>(hwnd);
        if let Ok(store) = property_store {
            let value: PROPVARIANT = icon_resource.as_str().into();
            let _ = store.SetValue(&PKEY_AppUserModel_RelaunchIconResource, &value);
            let _ = store.Commit();
        }
    }
}

#[cfg(not(windows))]
pub fn hwnd_from_window_handle(
    _handle: &dyn raw_window_handle::HasWindowHandle,
) -> Option<isize> {
    None
}

#[cfg(not(windows))]
pub fn hide_window(_hwnd: isize) {}

#[cfg(not(windows))]
pub fn show_window(_hwnd: isize) {}

#[cfg(not(windows))]
pub fn set_windows_taskbar_icon(_hwnd: isize) {}

pub fn icon_from_egui(icon: eframe::egui::IconData) -> tray_icon::Icon {
    tray_icon::Icon::from_rgba(icon.rgba, icon.width, icon.height)
        .expect("tray icon rgba")
}
