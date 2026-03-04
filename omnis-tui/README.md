# Omnis TUI

A cross-platform, terminal-native client for the [Omnis](../Omnis-Backend/README.md) end-to-end encrypted chat platform. Built in Rust with [Ratatui](https://ratatui.rs) and [Tokio](https://tokio.rs).

```
  ┌──────────────────────────────────────────────────────────┐
  │  alice  21:07          You  21:09                        │
  │  hey                   hi                                │
  │                        [ATTACHMENT: image | photo.png]   │
  │                        ████████████████▌                 │
  │                        ████░░░░░░░░░░░░                  │
  │                        ⏳ 58s                             │
  ├──────────────────────────────────────────────────────────┤
  │ Type a message…  Ctrl+O attach  Ctrl+T timer             │
  │ Type a message…                                          │
  └──────────────────────────────────────────────────────────┘
```

---

## Table of Contents

1. [Features](#features)
2. [Key Bindings](#key-bindings)
3. [Architecture](#architecture)
4. [Module Reference](#module-reference)
5. [Cryptography](#cryptography)
6. [Data Storage](#data-storage)
7. [Configuration & Settings](#configuration--settings)
8. [Building from Source](#building-from-source)
9. [Cross-Platform Notes](#cross-platform-notes)
10. [Environment Variables](#environment-variables)

---

## Features

| Feature | Detail |
|---|---|
| **End-to-end encryption** | P-384 ECDH key agreement, HKDF-SHA-256 epoch key derivation, AES-256-GCM message encryption |
| **Real-time messaging** | WebSocket (`/chat/ws/{chat_id}`) with heartbeat ping/pong |
| **Media attachments** | Send images/files with per-file AES-256-GCM encryption; auto-downloads and renders inline as pixel art using half-block Unicode characters (`▀`) |
| **Image preview** | Decodes JPEG, PNG, GIF, WebP, BMP in-terminal using coloured `▀` half-block cells — no external renderer needed |
| **Ephemeral messages** | `Ctrl+T` cycles self-destruct timers (10 s → 30 s → 1 m → 5 m → 1 h → 24 h). Timer is sent as `expires_at` in the message envelope; server and all clients prune on expiry |
| **Reply threading** | Select a message with arrow keys → `Enter` to quote-reply |
| **Clipboard paste** | `Ctrl+V` attaches an image directly from the clipboard |
| **System tray** | Hide to tray (`Ctrl+Q` or close), restore with tray icon click; "Run in background" toggle in Settings |
| **Sound notification** | `/fah` makes the app play an audio clip for the recipient |
| **Encrypted VoIP calls** | End-to-end encrypted audio calls between TUI users; server-relayed (CGNAT-safe), AES-256-GCM per-packet audio, ECDH-derived call key |
| **Noise filtering** | Built-in RNNoise ML suppression, first-order IIR high-pass filter, and RMS noise gate; four one-key presets plus manual sliders |
| **Persistent local cache** | SQLite stores decrypted messages, epoch keys, and settings locally |
| **Theme** | Customisable accent colour (Settings → Theme) |
| **Secure key storage** | OS keychain (Windows Credential Manager / macOS Keychain / libsecret on Linux) via `keyring` |

---

## Key Bindings

### Global

| Key | Action |
|---|---|
| `Ctrl+Q` | Hide window to system tray (if "Run in background" is on), or quit |
| `Escape` | Go back / cancel current action |
| `Tab` / `Shift+Tab` | Move focus between fields (Onboarding, Settings) |

### Home Screen

| Key | Action |
|---|---|
| `↑` / `↓` | Navigate chat list |
| `Enter` | Open selected chat |
| `n` | Start a new chat (prompts for username) |
| `c` | Call selected contact (initiates encrypted VoIP call) |
| `s` | Open Settings |
| `p` | Open Profile |
| `/` | Filter / search chat list |

### Chat Screen

| Key | Action |
|---|---|
| `Enter` | Send message (or send staged attachment + caption) |
| `Escape` | Cancel reply / cancel attachment / go back to Home |
| `↑` / `↓` | Scroll message list / move message selection |
| `Ctrl+O` | Open OS file picker to stage an attachment |
| `Ctrl+V` | Paste image from clipboard as attachment |
| `Ctrl+T` | Cycle ephemeral self-destruct timer: Off → 10 s → 30 s → 1 m → 5 m → 1 h → 24 h |
| `Ctrl+D` | Download selected message's media attachment |
| `Ctrl+R` | Reload messages from server |
| `PgUp` | Scroll up in message list |
| `PgDn` | Scroll down in message list |
| Mouse scroll | Scroll message list |
| Mouse click `[+]` | Open file picker |

### Settings Screen

| Key | Action |
|---|---|
| `↑` / `↓` | Navigate settings items |
| `Enter` | Toggle / edit selected setting |
| `Escape` | Close settings and return |

### Call Screen (incoming / outgoing VoIP)

| Key | Action |
|---|---|
| `a` | **Answer** incoming call (ringing state) |
| `r` | **Reject** incoming call / cancel outgoing call |
| `m` | Toggle **mute** (microphone) |
| `h` | Toggle **hold** (pauses local audio; signals remote) |
| `e` / `q` | **End** active call |
| `1` | Apply **Quiet Room** noise-filter preset |
| `2` | Apply **Office** noise-filter preset |
| `3` | Apply **Outdoor** noise-filter preset |
| `4` | Apply **Heavy Noise** noise-filter preset |
| `↑` / `↓` / `Tab` | Move focus between noise filter controls |
| `←` / `→` | Decrease / increase focused filter slider |

### Profile Screen

| Key | Action |
|---|---|
| `Escape` | Close profile |
| Session items → `d` | Revoke that session |
| `D` | Revoke all other sessions |
| `L` | Log out (clears all local credentials) |

---

## Architecture

```
omnis-tui/
├── src/
│   ├── main.rs             Entry point; sets up terminal, macOS tray thread
│   ├── app.rs              Core event loop, screen router, async task dispatcher
│   ├── error.rs            AppError / Result aliases
│   ├── theme.rs            Colour palette derived from a hex accent colour
│   ├── types.rs            Wire types (WireMessage, WireChat…), local DB types,
│   │                       MediaInfo, AuthState, AppSettings, WsFrame
│   ├── screens/
│   │   ├── mod.rs          AppAction enum (messages between screens and app.rs)
│   │   ├── onboarding.rs   Login / signup wizard
│   │   ├── home.rs         Chat list, new-chat dialog
│   │   ├── chat.rs         Chat view, attachment preview, timer UI
│   │   ├── call.rs         Full-screen VoIP call UI (controls, noise sliders, VU meters)
│   │   ├── settings.rs     API URL, theme colour, background-run toggle
│   │   └── profile.rs      User info, session list, logout
│   ├── components/
│   │   ├── message_list.rs Scrollable message list with pixel-preview rendering
│   │   ├── input_box.rs    Single-line UTF-8 input with cursor movement
│   │   ├── reply_preview.rs Inline quote bar shown above the input
│   │   └── chat_list.rs    Chat list rows with unread badges
│   └── services/
│       ├── api.rs          Typed HTTP client (reqwest + serde_json)
│       ├── crypto.rs       Full E2E crypto pipeline (P-384, HKDF, AES-GCM, call-key derivation)
│       ├── database.rs     SQLite schema + queries (rusqlite, bundled)
│       ├── websocket.rs    Async WS connection (tokio-tungstenite)
│       ├── call.rs         VoIP call service: audio pipeline, noise filters, presence WS
│       ├── attachment.rs   File-type detection, image preview renderer
│       ├── storage.rs      OS keychain + dirs-based config persistence
│       ├── notification.rs Desktop notifications, ringtone, and sound
│       └── tray.rs         System-tray icon, cross-platform event loop
└── src/bin/
    └── launcher.rs         Minimal GUI launcher (double-click → spawns omnis)
```

### Event Flow

```
 Terminal events ──┐
                   │
 Tick (50 ms) ─────┤──▶ App::run() loop ──▶ render()
                   │         │
 Async results ────┤         ├── dispatch_event_to_screen()   → AppAction
                   │         │         │
 Presence WS ──────┤         │         ▼
                   │         ├── handle_action()     spawns Tokio tasks ──▶
 Call events ───────┘         │                         api.rs / crypto.rs /
                             │                         call.rs / database.rs
                             │                               │
                             └── poll_ws_and_decrypt() ◀──  AsyncResult channel
```

1. `App::run()` drives a 50 ms Tokio `select!` loop that handles:
   - Terminal key/mouse events
   - Async results from background tasks via an `mpsc::UnboundedChannel`
   - Tray events from the tray icon thread
   - Presence WebSocket frames (incoming call invites)
   - VoIP `CallEvent`s from the active `CallService`
2. The active `Screen` handles key events and emits `AppAction` variants.
3. `handle_action()` in `app.rs` interprets actions and spawns `tokio::spawn` tasks.
4. Tasks communicate back by sending `AsyncResult` variants on the shared `tx` channel.

---

## Module Reference

### `app.rs`

Central state machine and dispatcher.

| Symbol | Purpose |
|---|---|
| `App` | Owns all top-level state (`auth`, `settings`, `theme`, `screen`, `db`, `api`, `chats`, `logs`, `call_state`, `call_service`, `presence_rx`) |
| `App::new()` | Initialises DB, loads persisted settings + auth, spawns tray, picks initial screen, connects presence WS if already logged-in |
| `App::run()` | Main `async` event loop; draws each frame, routes events, polls async results, presence frames, and call events |
| `App::render()` | Delegates to the active screen's `render()` method |
| `App::handle_action()` | Interprets `AppAction`, mutates state, spawns tasks — includes `InitiateCall`, `AnswerCall`, `RejectCall`, `EndCall` |
| `App::poll_ws_and_decrypt()` | Each tick: prunes expired messages, drains WS events, decrypts new ciphertexts, auto-downloads images |
| `App::handle_presence_frame()` | Reacts to `WsFrame::CallInvite`; starts ringtone, switches to `Screen::Call` |
| `App::handle_call_event()` | Routes `CallEvent` variants (Answered/Rejected/Ended/Level/HoldChanged) to call screen and app state |
| `App::decrypt_messages()` | Batch-decrypts `LocalMessage`s using stored epoch keys; populates `media_info` from JSON envelope |
| `spawn_send_message()` | Encrypts plaintext with current epoch key, POSTs to server |
| `spawn_send_media()` | Reads file, encrypts it, chunked-upload, POSTs envelope message |
| `spawn_fetch_messages()` | Fetches + epoch-unwraps messages from REST, sends `MessagesLoaded` |
| `spawn_download_media()` | Downloads encrypted blob, decrypts, saves to cache dir, builds pixel preview |
| `spawn_auto_login()` | Re-authenticates with stored credentials on startup to restore the identity private key |
| `spawn_initiate_call()` | Fetches peer public key, POSTs `/call/initiate`, sends `CallInitiated` result |
| `Screen` enum | `Onboarding`, `Home`, `Chat`, `Call`, `Settings(_, Box<Screen>)`, `Profile(_, Box<Screen>)` |
| `AsyncResult` enum | `LoginSuccess`, `MessagesLoaded`, `MessageSent`, `MediaDownloaded`, `UploadProgress`, `CallInitiated`, `CallError`, `CallServiceEvent`, … |

### `types.rs`

Pure data types shared across the codebase.

| Type | Description |
|---|---|
| `WireMessage` | Server JSON message shape; fields `id`, `sender_id`, `epoch_id`, `ciphertext`, `nonce`, `media_id`, `expires_at` |
| `LocalMessage` | In-memory message; adds `plaintext`, `media_info`, `download_state`, `pixel_preview`, `expires_at` |
| `MediaInfo` | Decrypted media envelope (zeroed on drop): `media_id`, `file_key [u8;32]`, `file_nonce [u8;12]`, `file_type`, `filename`, `caption` |
| `DownloadState` | `None` / `Pending` / `Downloaded(PathBuf)` / `Failed(String)` |
| `PendingAttachment` | Staged attachment with path, filename, file_type, caption, pixel_preview |
| `AuthState` | Runtime auth: `token`, `device_id`, `user_id`, `username`, identity keypair |
| `AppSettings` | `api_base_url`, `theme_color`, `run_in_background` |
| `WsFrame` | `History { messages }`, `NewMessage { message }`, `MessageDeleted { message_id }`, `Pong`, `CallInvite { call_id, caller_username, initiated_at }` |
| `PreviewCell` | `(char, (u8,u8,u8), (u8,u8,u8))` — Unicode half-block with fg/bg RGB |
| `CallState` | `Idle \| Ringing { call_id, caller } \| Calling { call_id, peer } \| Active { call_id, peer, start_time } \| Ended { reason }` |
| `FilterPreset` | `QuietRoom \| Office \| Outdoor \| HeavyNoise \| Custom` — with `params()` returning default `FilterParams` per preset |
| `FilterParams` | `{ preset, suppression: f32, gate_db: f32, highpass_hz: f32 }` — passed to `CallService::set_filter()` |

### `services/crypto.rs`

Full end-to-end crypto pipeline compatible with the Omnis web and mobile clients.

| Function | Algorithm | Purpose |
|---|---|---|
| `generate_identity_key_pair()` | NIST P-384 | Creates a new long-term identity keypair |
| `encrypt_identity_private_key()` | PBKDF2-HMAC-SHA-256 (600k iter) + AES-256-GCM | Wraps private key with user's passphrase |
| `decrypt_identity_private_key()` | same | Unwraps on login |
| `generate_aes_key()` | CSPRNG 256-bit | Creates a new epoch (message) key |
| `wrap_epoch_key()` | ECDH(P-384) + HKDF-SHA-256 + AES-256-GCM | Wraps an epoch key for a recipient |
| `unwrap_epoch_key()` | same | Unwraps a received epoch key |
| `aes_gcm_encrypt_message()` | AES-256-GCM | Encrypts message plaintext with epoch key |
| `aes_gcm_decrypt_message()` | AES-256-GCM | Decrypts message ciphertext |
| `generate_file_key()` | CSPRNG 256-bit | Creates a per-file encryption key |
| `encrypt_file()` | AES-256-GCM | Encrypts a media blob |
| `decrypt_file()` | AES-256-GCM | Decrypts a downloaded media blob |
| `build_media_plaintext()` | JSON serialisation | Builds the media envelope JSON that gets epoch-encrypted as the message body |
| `derive_call_key()` | ECDH(P-384) + HKDF-SHA-256 (`info="call-key"`) | Derives a 32-byte symmetric key from two identity keypairs for VoIP audio encryption |
| `aes_gcm_encrypt_raw()` | AES-256-GCM | Encrypts a raw audio frame; returns `(nonce, ciphertext)` |
| `aes_gcm_decrypt_raw()` | AES-256-GCM | Decrypts a raw audio frame given nonce + ciphertext |

### `services/api.rs`

Typed REST client wrapping `reqwest`.

| Method | Endpoint |
|---|---|
| `login()` | `POST /auth/login` |
| `signup()` | `POST /auth/signup` |
| `me()` | `GET /auth/me` |
| `get_keyblob()` | `GET /auth/keyblob` |
| `list_chats()` | `GET /chat/list` |
| `create_chat()` | `POST /chat/create` |
| `fetch_messages()` | `GET /chat/fetch/{chat_id}` |
| `fetch_epoch_key()` | `GET /chat/{chat_id}/{epoch_id}/fetch` |
| `create_epoch()` | `POST /chat/{chat_id}/epoch` |
| `send_message()` | `POST /chat/{chat_id}/message` |
| `init_media_upload()` | `POST /media/upload/init` |
| `upload_chunk()` | `PUT /media/upload/{id}/chunk/{i}` |
| `finalize_upload()` | `POST /media/upload/{id}/finalize` |
| `download_media()` | `GET /media/{media_id}` |
| `get_user_pubkey()` | `GET /user/pkey/get?username=…` |
| `list_sessions()` | `GET /users/sessions` |
| `revoke_session()` | `DELETE /users/sessions/revoke/{id}` |
| `revoke_other_sessions()` | `DELETE /users/sessions/revoke_other` |
| `logout()` | `POST /auth/logout` |
| `initiate_call()` | `POST /call/initiate` |
| `answer_call()` | `POST /call/answer` |
| `reject_call()` | `POST /call/reject` |
| `end_call()` | `POST /call/end` |
| `ws_url()` | Convert an `http(s)://` base URL to `ws(s)://` for WebSocket connections |

### `services/database.rs`

Bundled SQLite (no external install needed) accessed via `rusqlite`.

Schema tables:

| Table | Columns of interest |
|---|---|
| `messages` | `id`, `chat_id`, `sender_id`, `epoch_id`, `reply_id`, `ciphertext`, `nonce`, `plaintext`, `created_at`, `synced`, `expires_at` |
| `epoch_keys` | `chat_id`, `epoch_id`, `epoch_index`, `key` (plaintext AES key) |
| `chats` | `chat_id`, `with_user`, `with_user_id`, `last_message`, `last_message_time` |

Key methods: `upsert_message()`, `get_messages()`, `get_epoch_key()`, `save_epoch_key()`, `get_latest_epoch_key()`.

### `services/attachment.rs`

| Function | Purpose |
|---|---|
| `build_image_preview_pub()` | Decodes image bytes, scales to fit terminal columns/rows, converts to half-block `▀` cells |
| `format_file_size()` | Human-readable size string ("11.1 KB", "4.2 MB") |

Image preview algorithm:
1. Decode with `image` crate (JPEG, PNG, GIF, WebP, BMP supported)
2. Thumbnail to `max_cols × max_rows*2` pixels
3. For each pair of pixel rows, emit one terminal row of `▀` characters with `fg = top pixel RGB`, `bg = bottom pixel RGB`
4. Result: true-colour image at 2× vertical resolution in any Truecolor terminal

### `services/websocket.rs`

Connects to `ws(s)://<host>/chat/ws/{chat_id}?token=&device_id=` and streams `WsEvent`:
- `WsEvent::Connected`
- `WsEvent::Frame(WsFrame)` — deserialized server frames
- `WsEvent::Disconnected`

Automatic heartbeat ping every 30 s.

### `services/call.rs`

Full VoIP call service. Handles the complete audio pipeline and call lifecycle.

**Audio pipeline (send path):**
```
cpal input (OS audio thread, WASAPI/ALSA/CoreAudio)
  └─ tokio channel
     └─ Audio processing task
          ├─ Rubato resample to 48 kHz (if device ≠ 48 kHz)
          ├─ nnnoiseless RNNoise ML denoising (480-sample / 10 ms blocks)
          ├─ First-order IIR high-pass filter
          ├─ RMS noise gate
          └─ AES-256-GCM encrypt → binary WS frame → /call/audio/ws
```

**Wire frame format:** `[seq: u64 LE 8 B][nonce: 12 B][ciphertext + GCM tag]`  
Frame size: 1920 samples × 2 bytes + 36 header/tag = 3876 B @ 25 fps ≈ 97 kbps.

| Symbol | Purpose |
|---|---|
| `CallService` | Owns cpal streams + Tokio task handles; returned from `CallService::start()` |
| `CallService::start()` | Connects audio + signaling WS, starts cpal I/O, spawns three tasks; returns `(service, event_rx)` |
| `CallService::set_filter()` | Hot-swaps `FilterParams`; picked up by the audio processing task on the next frame |
| `CallService::shutdown()` | Sets the shutdown atomic flag; all tasks and streams stop gracefully |
| `CallEvent` | `Answered \| Rejected \| Ended \| Error(String) \| LocalLevel(f32) \| RemoteLevel(f32) \| HoldChanged(bool)` |
| `connect_presence()` | Connects to `WS /user/ws`; returns an `UnboundedReceiver<WsFrame>` with automatic reconnect and exponential backoff |

### `services/notification.rs`

| Function | Purpose |
|---|---|
| `notify()` | OS toast: "New message from &lt;user&gt;" |
| `notify_incoming_call()` | OS toast "&lt;user&gt; is calling you" + synthesised dual-tone ringtone (440 Hz / 490 Hz); returns `Arc<AtomicBool>` stop flag |
| `stop_ringtone()` | Sets the stop flag returned by `notify_incoming_call()` |
| `play_fah()` | Plays bundled `fah.mp3` on a detached thread |

- **Windows / Linux**: `tray-icon` crate, runs in a Tokio task alongside the TUI thread
- **macOS**: `NSStatusItem` created on the main thread; TUI runs on a background thread. `macos_main()` in `main.rs` drives `NSApp::run()` on the main thread while TUI runs in a spawned thread.

Tray actions piped to `App` via `TrayAction` enum: `Show`, `Quit`.

### `services/tray.rs`

Renders messages as a `ratatui::List`. For each `LocalMessage`:
1. Shows sender name + timestamp
2. For text messages: word-wraps `plaintext`
3. For media messages: shows `info.caption` (if any), then `[ATTACHMENT: type | filename]` in accent colour
4. If `pixel_preview` is populated: renders the coloured half-block image rows inline
5. Shows ephemeral countdown (`⏳ Xs`) in red for messages with `expires_at`

### `components/message_list.rs`

| Field | Purpose |
|---|---|
| `ephemeral_secs: u32` | Self-destruct timer preset (0 = off) |
| `pending_attachment` | Staged `PendingAttachment` shown in preview panel |
| `reply_to` | `LocalMessage` being quoted |
| `ws_rx` | WS event receiver owned by this screen |

Timer presets cycled by `Ctrl+T`: `0 → 10 → 30 → 60 → 300 → 3600 → 86400 → 0`  
Displayed as: "10s", "30s", "1m", "5m", "1h", "24h"

---

## Cryptography

All cryptography is performed entirely on the client. The server stores only encrypted blobs.

### VoIP Call Encryption

- **Call key derivation**: `ECDH(myPriv, peerPub)` → shared x-coordinate → `HKDF-SHA-256(ikm=shared_x, salt=[0×32], info=b"call-key")` → 32-byte AES key
- **Per-packet encryption**: AES-256-GCM; each 40 ms audio frame gets a fresh 12-byte CSPRNG nonce — no nonce reuse even across seq counter wrap
- **Wire format**: `[seq: 8 B][nonce: 12 B][ciphertext + 16 B GCM tag]` — the server forwards the binary blob without inspection
- **Audio codec**: Raw i16 LE mono PCM at 48 kHz — no codec dependency; frame = 1920 samples (40 ms)
- **Noise filters**: [nnnoiseless](https://crates.io/crates/nnnoiseless) (pure-Rust RNNoise port), first-order IIR high-pass, RMS noise gate — all tunable at runtime via `FilterParams`

### Identity Keys

- Algorithm: **NIST P-384** (ECDH)
- Each user generates one long-term keypair at signup
- Public key: published to server (base64 SPKI DER)
- Private key: encrypted with user's passphrase using **PBKDF2-HMAC-SHA-256** (600,000 iterations) → **AES-256-GCM**, stored on server as `encrypted_identity_priv` + `kdf_salt` + `aead_nonce`
- On login the private key is decrypted in-memory and stored in `AuthState.identity_private_key`; it is never written to disk unencrypted

### Epoch (Message) Keys

- Each chat has rotating epochs, each with a 256-bit AES key
- Key wrapping: `ECDH(myPriv, peerPub)` → `HKDF-SHA-256` shared secret → `AES-256-GCM` wraps the epoch key
- The server stores two copies of each wrapped epoch key (one for each participant, wrapped for their identity key)
- TUI fetches and unwraps epoch keys on demand; epoch keys are cached in SQLite

### Message Encryption

- Algorithm: **AES-256-GCM**
- Plaintext is the raw message body (UTF-8) or a JSON media envelope
- A fresh 12-byte nonce is generated per message via CSPRNG

### Media Encryption

- Algorithm: **AES-256-GCM** with a fresh per-file 256-bit key and 12-byte nonce
- The encrypted blob is chunked and uploaded via the media API
- The file key and nonce are embedded inside the (epoch-encrypted) message envelope — the server never has access to media keys
- Sender's file bytes are cached locally immediately; recipients auto-download and build pixel preview on receipt

### Key Zeroization

`MediaInfo` derives `Zeroize` + `ZeroizeOnDrop` — `file_key` and `file_nonce` are zeroed from memory when the struct is dropped.

---

## Data Storage

| What | Where | How |
|---|---|---|
| Auth token | OS keychain (`keyring`) | `storage::save_auth_token()` |
| User password | OS keychain | Stored only to support silent re-login on startup |
| Identity private key | Memory only | Never written to disk unencrypted |
| Identity public key | `~/.local/share/omnis/` (XDG) / `%APPDATA%\omnis\` (Windows) / `~/Library/Application Support/omnis/` (macOS) | Plaintext (public key, not secret) |
| SQLite database | `<data_dir>/omnis.db` | Messages, epoch keys, chats |
| Media cache | `<data_dir>/media/` | Decrypted blobs named by original filename |
| Device ID | `<data_dir>/device_id` | Persistent UUID v4 |
| Settings (API URL, theme, background-run) | `<data_dir>/settings.json` | JSON |

---

## Configuration & Settings

Open the Settings screen with `s` from the Home screen.

| Setting | Default | Description |
|---|---|---|
| API Base URL | `http://localhost:8000` | HTTP(S) URL of the Omnis backend |
| Theme colour | `#6C63FF` | Hex accent colour applied to borders, headers, buttons |
| Run in background | Off | When on, `Ctrl+Q` hides to tray instead of quitting |

---

## Building from Source

### Prerequisites (all platforms)

- [Rust](https://rustup.rs) stable toolchain (`rustup update stable`)
- `cargo` in `$PATH`

### Windows

```powershell
# Clone and build
git clone https://github.com/your-org/omnis-app
cd omnis-app\omnis-tui
cargo build --release

# Binary location
.\target\release\omnis.exe
```

Required: **Visual Studio C++ Build Tools** (for `cc` linker and Windows SDK headers)  
Optional: [WiX Toolset](https://wixtoolset.org/) if you want to produce an installer

### Arch Linux

```bash
# Install system dependencies
sudo pacman -S base-devel pkg-config openssl libxcb libdbus libayatana-appindicator

# Build
git clone https://github.com/your-org/omnis-app
cd omnis-app/omnis-tui
cargo build --release

# Binary location
./target/release/omnis
```

> **Note:** `keyring` uses `libdbus`/`libsecret` on Linux. Ensure `libsecret` (or equivalent) is installed:  
> `sudo pacman -S gnome-keyring libsecret`  
> If running headless/no keyring daemon, set `OMNIS_NO_KEYRING=1` (falls back to plaintext file in `<data_dir>`).

### macOS

```bash
# Install Xcode command-line tools
xcode-select --install

# Build
git clone https://github.com/your-org/omnis-app
cd omnis-app/omnis-tui
cargo build --release

# Binary location
./target/release/omnis
```

> **macOS tray note:** On macOS the system tray icon (`NSStatusItem`) must run on the main thread. `main.rs` handles this automatically: the TUI runs on a background thread while the main thread drives the `NSApp` event loop.

---

## Cross-Platform Notes

| Platform | Terminal | Image preview | System tray | Notifications |
|---|---|---|---|---|
| **Windows** | Windows Terminal, ConEmu | Requires Truecolor support | ✅ Windows tray | ✅ Toast |
| **Arch Linux** | Any VTE-based terminal (Alacritty, Kitty, GNOME Terminal) | Requires Truecolor support | ✅ via AppIndicator / SNI | ✅ via libnotify |
| **macOS** | iTerm2, Terminal.app, Kitty | Requires Truecolor support | ✅ NSStatusItem (native) | ✅ UNUserNotification |

Image preview requires **Truecolor** (24-bit colour) terminal support. Most modern terminals support this. Terminals without Truecolor will show garbled colours on image preview rows.

### Cross-Compiling from Windows

Using [`cross`](https://github.com/cross-rs/cross) (requires Docker):

```powershell
cargo install cross --git https://github.com/cross-rs/cross
# Linux x86_64
cross build --release --target x86_64-unknown-linux-gnu
# Linux ARM64
cross build --release --target aarch64-unknown-linux-gnu
```

macOS cross-compilation from Windows requires [osxcross](https://github.com/tpoechtrager/osxcross) (advanced; typically easier to build natively on a Mac).

---

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `OMNIS_NO_KEYRING` | unset | Set to `1` to disable OS keychain and fall back to plaintext file storage for credentials (useful in headless/container environments) |
| `OMNIS_DATA_DIR` | OS-appropriate app data dir | Override the directory used for the SQLite DB, media cache, and config files |
| `RUST_LOG` | unset | Enable log output (e.g. `RUST_LOG=debug`) — currently prints to stderr; redirect with `2>omnis.log` |
