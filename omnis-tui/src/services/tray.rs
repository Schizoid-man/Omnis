//! System-tray integration.
//!
//! On Windows a real tray icon is created with **Open / Run in Background / Quit**
//! menu items.  On other platforms the public API compiles but is a no-op.

use tokio::sync::mpsc;

/// Actions the tray menu can emit.
#[derive(Debug)]
pub enum TrayAction {
    Show,
    ToggleBackground(bool),
    Quit,
}

// ── Windows implementation ───────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod win32 {
    //! Minimal raw FFI so we don't need the heavy `windows` crate.

    type HWND = *mut std::ffi::c_void;
    type BOOL = i32;

    const SW_HIDE: i32 = 0;
    const SW_SHOW: i32 = 5;
    const PM_REMOVE: u32 = 0x0001;

    #[repr(C)]
    struct POINT {
        _x: i32,
        _y: i32,
    }

    #[repr(C)]
    #[allow(non_snake_case)]
    struct MSG {
        hwnd: HWND,
        message: u32,
        wParam: usize,
        lParam: isize,
        time: u32,
        pt: POINT,
    }

    extern "system" {
        fn GetConsoleWindow() -> HWND;
        fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> BOOL;
        fn SetForegroundWindow(hWnd: HWND) -> BOOL;
        fn PeekMessageW(
            lpMsg: *mut MSG,
            hWnd: HWND,
            wMsgFilterMin: u32,
            wMsgFilterMax: u32,
            wRemoveMsg: u32,
        ) -> BOOL;
        fn TranslateMessage(lpMsg: *const MSG) -> BOOL;
        fn DispatchMessageW(lpMsg: *const MSG) -> isize;
    }

    /// Show the console window and bring it to the foreground.
    pub fn show_console() {
        unsafe {
            let hwnd = GetConsoleWindow();
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_SHOW);
                SetForegroundWindow(hwnd);
            }
        }
    }

    /// Hide the console window (the process keeps running).
    pub fn hide_console() {
        unsafe {
            let hwnd = GetConsoleWindow();
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
    }

    /// Drain the Windows message queue (required for the tray icon to work).
    pub fn pump_messages() {
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

// ── TrayHandle ───────────────────────────────────────────────────────────────

/// Commands the app can send to the tray thread.
pub(crate) enum TrayCommand {
    SetBackgroundChecked(bool),
}

/// On macOS the tray icon must be created on the main thread.
/// `spawn()` stashes the init data here; `main()` calls `run_macos_loop()`.
#[cfg(target_os = "macos")]
pub struct MacosInit {
    pub action_tx: mpsc::UnboundedSender<TrayAction>,
    pub cmd_rx: std::sync::mpsc::Receiver<TrayCommand>,
    pub initial_bg: bool,
}

/// Returned to the application when the tray thread is spawned.
pub struct TrayHandle {
    cmd_tx: std::sync::mpsc::Sender<TrayCommand>,
    #[cfg(target_os = "macos")]
    pub macos_init: Option<MacosInit>,
}

impl TrayHandle {
    /// Update the checkbox in the tray menu (call when the TUI setting changes).
    pub fn set_run_in_background(&self, val: bool) {
        let _ = self.cmd_tx.send(TrayCommand::SetBackgroundChecked(val));
    }

    /// Take the macOS init data (only meaningful on macOS; must be called from main()).
    #[cfg(target_os = "macos")]
    pub fn take_macos_init(&mut self) -> Option<MacosInit> {
        self.macos_init.take()
    }
}

// ── spawn ────────────────────────────────────────────────────────────────────

/// Spawn the system-tray thread and return a channel + handle.
///
/// - **Windows**: a background OS thread is spawned immediately.
/// - **macOS**: no thread is spawned; the handle contains `MacosInit`.
///   The caller must extract it via `handle.take_macos_init()` and call
///   `run_macos_loop()` from the **main thread**.
/// - **Other**: no-op stubs.
pub fn spawn(initial_bg: bool) -> (mpsc::UnboundedReceiver<TrayAction>, TrayHandle) {
    #[cfg(target_os = "windows")]
    {
        spawn_windows(initial_bg)
    }
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = mpsc::unbounded_channel::<TrayAction>();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<TrayCommand>();
        let handle = TrayHandle {
            cmd_tx,
            macos_init: Some(MacosInit { action_tx: tx, cmd_rx, initial_bg }),
        };
        (rx, handle)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = initial_bg;
        let (_tx, rx) = mpsc::unbounded_channel();
        let (cmd_tx, _cmd_rx) = std::sync::mpsc::channel();
        (rx, TrayHandle { cmd_tx })
    }
}

#[cfg(target_os = "windows")]
fn spawn_windows(
    initial_bg: bool,
) -> (mpsc::UnboundedReceiver<TrayAction>, TrayHandle) {
    use std::time::Duration;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIconBuilder};

    let (tx, rx) = mpsc::unbounded_channel();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<TrayCommand>();

    // ── Tray thread — everything menu-related lives here ─────────────────
    std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || {
            // Build menu items (Menu/MenuItem are !Send, so they must live here)
            let show_item = MenuItem::new("Open Omnis", true, None);
            let bg_item = CheckMenuItem::new("Run in Background", true, initial_bg, None);
            let quit_item = MenuItem::new("Quit", true, None);

            let show_id = show_item.id().clone();
            let bg_id = bg_item.id().clone();
            let quit_id = quit_item.id().clone();

            let menu = Menu::new();
            let _ = menu.append(&show_item);
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&bg_item);
            let _ = menu.append(&PredefinedMenuItem::separator());
            let _ = menu.append(&quit_item);

            // Icon: 32×32 filled circle (accent blue)
            let size = 32u32;
            let mut rgba = vec![0u8; (size * size * 4) as usize];
            let center = size as f32 / 2.0;
            let radius = center - 1.0;
            for y in 0..size {
                for x in 0..size {
                    let dx = x as f32 - center + 0.5;
                    let dy = y as f32 - center + 0.5;
                    if (dx * dx + dy * dy).sqrt() <= radius {
                        let i = ((y * size + x) * 4) as usize;
                        rgba[i] = 88;      // R
                        rgba[i + 1] = 166;  // G
                        rgba[i + 2] = 255;  // B
                        rgba[i + 3] = 255;  // A
                    }
                }
            }
            let icon =
                Icon::from_rgba(rgba, size, size).expect("Failed to create tray icon image");

            let _tray = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("Omnis")
                .with_icon(icon)
                .build()
                .expect("Failed to build system-tray icon");

            let menu_rx = MenuEvent::receiver();

            loop {
                win32::pump_messages();

                // Process commands from the app (e.g. sync checkbox state)
                while let Ok(cmd) = cmd_rx.try_recv() {
                    match cmd {
                        TrayCommand::SetBackgroundChecked(val) => bg_item.set_checked(val),
                    }
                }

                if let Ok(event) = menu_rx.try_recv() {
                    let id = event.id;
                    if id == show_id {
                        win32::show_console();
                        let _ = tx.send(TrayAction::Show);
                    } else if id == bg_id {
                        let checked = bg_item.is_checked();
                        let _ = tx.send(TrayAction::ToggleBackground(checked));
                    } else if id == quit_id {
                        let _ = tx.send(TrayAction::Quit);
                        break;
                    }
                }

                std::thread::sleep(Duration::from_millis(50));
            }
        })
        .expect("Failed to spawn tray thread");

    (rx, TrayHandle { cmd_tx })
}

// ── Console window helpers (public) ──────────────────────────────────────────

/// Show the console window and bring it to the foreground.
pub fn show_console_window() {
    #[cfg(target_os = "windows")]
    win32::show_console();
}

/// Hide the console window (the process keeps running).
pub fn hide_console_window() {
    #[cfg(target_os = "windows")]
    win32::hide_console();
}

// ── macOS main-thread event loop ─────────────────────────────────────────────

/// Run the macOS system-tray event loop **on the main thread**.
///
/// Must be called after `spawn()` on macOS.  The function blocks until the
/// user clicks Quit in the menu or the TUI has exited (detected via the
/// closed sender channel).
#[cfg(target_os = "macos")]
pub fn run_macos_loop(init: MacosInit) {
    use std::time::Duration;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIconBuilder};

    let MacosInit { action_tx: tx, cmd_rx, initial_bg } = init;

    // Build menu items — must happen on the main thread on macOS
    let show_item = MenuItem::new("Open Omnis", true, None);
    let bg_item = CheckMenuItem::new("Run in Background", true, initial_bg, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let show_id = show_item.id().clone();
    let bg_id = bg_item.id().clone();
    let quit_id = quit_item.id().clone();

    let menu = Menu::new();
    let _ = menu.append(&show_item);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&bg_item);
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&quit_item);

    // Icon: 32×32 filled circle (accent blue)
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let center = size as f32 / 2.0;
    let radius = center - 1.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center + 0.5;
            let dy = y as f32 - center + 0.5;
            if (dx * dx + dy * dy).sqrt() <= radius {
                let i = ((y * size + x) * 4) as usize;
                rgba[i] = 88;       // R
                rgba[i + 1] = 166;  // G
                rgba[i + 2] = 255;  // B
                rgba[i + 3] = 255;  // A
            }
        }
    }
    let icon = Icon::from_rgba(rgba, size, size).expect("Failed to create tray icon image");

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Omnis")
        .with_icon(icon)
        .build()
        .expect("Failed to build system-tray icon");

    let menu_rx = MenuEvent::receiver();

    loop {
        // TUI has fully quit — stop the event loop
        if tx.is_closed() {
            break;
        }

        // Pump the macOS NSApp / CoreFoundation run loop so tray events fire
        pump_macos_runloop(50.0);

        // Relay commands from TUI → tray (e.g. checkbox sync)
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                TrayCommand::SetBackgroundChecked(val) => bg_item.set_checked(val),
            }
        }

        // Dispatch menu events
        if let Ok(event) = menu_rx.try_recv() {
            let id = event.id;
            if id == show_id {
                let _ = tx.send(TrayAction::Show);
            } else if id == bg_id {
                let checked = bg_item.is_checked();
                let _ = tx.send(TrayAction::ToggleBackground(checked));
            } else if id == quit_id {
                let _ = tx.send(TrayAction::Quit);
                break;
            }
        }
    }
}

/// Pump the macOS CoreFoundation run loop for `millis` milliseconds.
/// Uses a direct link to CoreFoundation.framework — no extra crate needed.
#[cfg(target_os = "macos")]
fn pump_macos_runloop(millis: f64) {
    use std::ffi::c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopDefaultMode: *const c_void;
        fn CFRunLoopRunInMode(
            mode: *const c_void,
            seconds: f64,
            returnAfterSourceHandled: u8,
        ) -> i32;
    }

    unsafe {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, millis / 1000.0, 0);
    }
}
