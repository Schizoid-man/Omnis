//! **Omnis Launcher** — a minimal no-console shim for Windows.
//!
//! Built with `windows_subsystem = "windows"` so double-clicking it from
//! Explorer (or a shortcut / Start Menu tile) doesn't flash a black console
//! window.  It locates `omnis.exe` next to itself and spawns it inside
//! **Windows Terminal** (`wt.exe`) when available, falling back to a plain
//! `cmd /c start` so the TUI always runs in its own visible terminal window.
//!
//! On non-Windows platforms this binary is a no-op.

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    launch();
    #[cfg(not(windows))]
    {
        eprintln!("omnis-launcher is a Windows-only helper.");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn launch() {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let exe = std::env::current_exe().expect("Cannot determine launcher path");
    let dir = exe.parent().expect("Launcher has no parent directory");
    let omnis = dir.join("omnis.exe");

    if !omnis.exists() {
        // Nothing useful we can do without a console window — just exit.
        return;
    }

    // ── Try Windows Terminal ──────────────────────────────────────────────────
    // `wt new-tab -- <path>` opens omnis.exe in a new WT tab (or a fresh WT
    // window if none is open).  WT itself runs hidden; it opens its own window.
    let wt_ok = Command::new("wt")
        .args(["new-tab", "--title", "Omnis", "--"])
        .arg(&omnis)
        // Suppress the brief flash of wt's own invisible helper window
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .is_ok();

    if wt_ok {
        return;
    }

    // ── Fall back: cmd /c start ───────────────────────────────────────────────
    // `start ""` opens a new conhost/cmd window; omnis.exe runs inside it.
    let _ = Command::new("cmd")
        .args(["/c", "start", "Omnis"])
        .arg(&omnis)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}
