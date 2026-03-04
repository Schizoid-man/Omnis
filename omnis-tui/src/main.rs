mod app;
mod components;
mod error;
mod screens;
mod services;
mod theme;
mod types;

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

use app::App;

fn main() {
    // ── macOS ─────────────────────────────────────────────────────────────────
    // The system-tray icon (NSStatusItem) must be created and driven from the
    // main thread.  We spin up a dedicated Tokio runtime + Ratatui TUI on a
    // background thread, while the main thread owns the tray event loop.
    #[cfg(target_os = "macos")]
    {
        return macos_main();
    }

    // ── Windows / Linux ───────────────────────────────────────────────────────
    tokio::runtime::Runtime::new()
        .expect("Failed to create Tokio runtime")
        .block_on(async {
            if let Err(e) = run_tui().await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        });
}

// ── shared TUI bootstrap (Windows / Linux) ────────────────────────────────────

#[cfg(not(target_os = "macos"))]
async fn run_tui() -> error::Result<()> {
    enable_raw_mode().expect("Failed to enable raw mode");
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    ).expect("Failed to enter alternate screen");
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).expect("Failed to create terminal");

    let mut app = App::new()?;
    let result = app.run(&mut terminal).await;

    disable_raw_mode().expect("Failed to disable raw mode");
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )
    .expect("Failed to leave alternate screen");
    terminal.show_cursor().expect("Failed to show cursor");

    result
}

// ── macOS entry point ─────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn macos_main() {
    // App::new() calls tray::spawn() which, on macOS, sets up the channels but
    // defers icon creation (must happen on main thread).
    let mut app = App::new().unwrap_or_else(|e| {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    });

    // Pull out the deferred macOS tray state before moving `app` to the thread.
    let macos_init = app
        .take_macos_tray_init()
        .expect("macOS tray init missing — was tray::spawn() called?");

    // Ratatui TUI lives on this background thread with its own Tokio runtime.
    std::thread::Builder::new()
        .name("tui".into())
        .spawn(move || {
            tokio::runtime::Runtime::new()
                .expect("Failed to create Tokio runtime")
                .block_on(async move {
                    enable_raw_mode().expect("Failed to enable raw mode");
                    let mut stdout = io::stdout();
                    execute!(stdout, EnterAlternateScreen)
                        .expect("Failed to enter alternate screen");
                    let backend = CrosstermBackend::new(io::stdout());
                    let mut terminal =
                        Terminal::new(backend).expect("Failed to create terminal");

                    if let Err(e) = app.run(&mut terminal).await {
                        eprintln!("Error: {}", e);
                    }

                    let _ = disable_raw_mode();
                    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
                    let _ = terminal.show_cursor();
                });
        })
        .expect("Failed to spawn TUI thread");

    // Main thread drives the macOS NSApp / CoreFoundation run loop.
    // Returns when the user clicks Quit or the TUI thread exits.
    services::tray::run_macos_loop(macos_init);
}
