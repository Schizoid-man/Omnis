//! Desktop notification helper.
//!
//! Fires a native toast / banner notification when a new message arrives:
//! - **Windows**: WinRT toast via `notify-rust` (shows in Action Center)
//! - **macOS**: UNUserNotification / NSUserNotification via `notify-rust`
//! - **Linux**: libnotify / D-Bus via `notify-rust`
//!
//! The OS call is dispatched onto a short-lived background thread so it never
//! blocks the Tokio runtime or the Ratatui render loop.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Show a "New message from <from_user>" notification.
///
/// Safe to call from any context — spawns a detached thread internally.
/// If the notification system is unavailable the call is silently ignored.
pub fn notify(from_user: &str) {
    let title = "Omnis".to_string();
    let body = format!("New message from {}", from_user);

    std::thread::Builder::new()
        .name("notification".into())
        .spawn(move || {
            let result = notify_rust::Notification::new()
                .appname("Omnis")
                .summary(&title)
                .body(&body)
                // Use a stock icon name; ignored on Windows (uses app icon)
                .icon("dialog-information")
                .show();

            // Silently ignore any failure (e.g. no notification daemon on Linux)
            let _ = result;
        })
        .ok(); // silently ignore thread-spawn failure
}

// ── Incoming call notification ───────────────────────────────────────────────────

/// Show an OS toast for an incoming call, then loop a sine-wave ringtone
/// until the returned `Arc<AtomicBool>` is set to `true`.
///
/// Returns immediately; the ringtone plays on a detached background thread.
pub fn notify_incoming_call(caller: &str) -> Arc<AtomicBool> {
    let stop_flag = Arc::new(AtomicBool::new(false));

    // OS toast
    let caller_name = caller.to_owned();
    std::thread::Builder::new()
        .name("call-toast".into())
        .spawn(move || {
            let _ = notify_rust::Notification::new()
                .appname("Omnis")
                .summary("Incoming call")
                .body(&format!("{} is calling you", caller_name))
                .icon("call-start")
                .show();
        })
        .ok();

    // Ringtone loop on a separate thread
    let stop = Arc::clone(&stop_flag);
    std::thread::Builder::new()
        .name("call-ringtone".into())
        .spawn(move || {
            use rodio::{OutputStream, Sink, Source};
            use std::time::Duration;

            let Ok((_stream, stream_handle)) = OutputStream::try_default() else { return };
            let Ok(sink) = Sink::try_new(&stream_handle) else { return };
            sink.set_volume(0.5);

            // Ring: 440 Hz (0.8 s) + 490 Hz (0.8 s), silence (1.4 s) — repeat
            while !stop.load(Ordering::Relaxed) {
                // First tone
                let tone_a = rodio::source::SineWave::new(440.0)
                    .take_duration(Duration::from_millis(800))
                    .amplify(0.6);
                let tone_b = rodio::source::SineWave::new(490.0)
                    .take_duration(Duration::from_millis(800))
                    .amplify(0.6);
                let silence = rodio::source::Zero::<f32>::new(1, 44100)
                    .take_duration(Duration::from_millis(1400));

                sink.append(tone_a);
                sink.append(tone_b);
                sink.append(silence);

                // Sleep in small steps so we can check stop_flag
                let deadline = std::time::Instant::now() + Duration::from_millis(3000);
                while std::time::Instant::now() < deadline {
                    if stop.load(Ordering::Relaxed) {
                        sink.stop();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                sink.sleep_until_end();
            }
        })
        .ok();

    stop_flag
}

/// Signal the ringtone thread started by [`notify_incoming_call`] to stop.
#[inline]
pub fn stop_ringtone(flag: &Arc<AtomicBool>) {
    flag.store(true, Ordering::Relaxed);
}

// ── /fah sound ───────────────────────────────────────────────────────────────────

/// Bytes of the bundled fah.mp3, compiled into the binary.
static FAH_MP3: &[u8] = include_bytes!("../assets/fah.mp3");

/// Play the /fah sound on a background thread.
/// Safe to call from any context; silently ignores any audio error.
pub fn play_fah() {
    std::thread::Builder::new()
        .name("fah-audio".into())
        .spawn(|| {
            use rodio::{Decoder, OutputStream, Sink};
            use std::io::Cursor;

            // Open the default audio output — this can fail if no audio device exists.
            let Ok((_stream, stream_handle)) = OutputStream::try_default() else { return };
            let Ok(sink) = Sink::try_new(&stream_handle) else { return };

            let cursor = Cursor::new(FAH_MP3);
            let Ok(source) = Decoder::new(cursor) else { return };

            sink.append(source);
            // Block this thread until playback finishes (the thread is detached so
            // it doesn't hold up anything else).
            sink.sleep_until_end();
        })
        .ok();
}
