/// Top-level application state and event loop.
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::Terminal;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use crate::error::Result;
use crate::screens::{
    call::CallScreen,
    chat::ChatScreen, home::HomeScreen, onboarding::OnboardingScreen, profile::ProfileScreen,
    settings::SettingsScreen, AppAction,
};
use crate::services::{api::ApiClient, call as call_svc, crypto, database::Database, notification, storage, tray, websocket};
use crate::theme::Theme;
use crate::types::{AppSettings, AuthState, CallState, DownloadState, LocalChat, LocalMessage, MediaInfo, WireSession};

// ── Screen enum ──────────────────────────────────────────────────────────────

enum Screen {
    Onboarding(OnboardingScreen),
    Home(HomeScreen),
    Chat(ChatScreen),
    Call(CallScreen),
    Settings(SettingsScreen, Box<Screen>),   // settings + the screen underneath
    Profile(ProfileScreen, Box<Screen>),      // profile + the screen underneath
}

// ── Async result channel ─────────────────────────────────────────────────────

#[derive(Debug)]
enum AsyncResult {
    LoginSuccess {
        token: String,
        user_id: i64,
        username: String,
        pub_key: String,
        priv_key: String,
        password: String,
    },
    AutoLoginSuccess { priv_key: String },
    /// Silent re-authentication on fresh launch (stored credentials).
    SilentLoginSuccess { token: String, user_id: i64, username: String, priv_key: String },
    SignupSuccess,
    AuthError(String),
    ChatListLoaded(Vec<LocalChat>),
    MessagesLoaded {
        chat_id: i64,
        messages: Vec<LocalMessage>,
        /// (epoch_id, epoch_index, plaintext_key) — already unwrapped
        epoch_keys: Vec<(i64, i64, String)>,
    },
    MessageSent {
        chat_id: i64,
        message: LocalMessage,
    },
    MessageError(String),
    ChatCreated { chat_id: i64, with_user: String },
    EpochCreated { chat_id: i64, epoch_id: i64, epoch_index: i64, key: String },
    ConnectionResult(bool),
    SessionsLoaded(Vec<WireSession>),
    LogOutDone,
    Error(String),
    /// Upload progress for the in-progress media upload.
    UploadProgress { chunk: usize, total: usize },
    /// A media download+decrypt completed successfully.
    MediaDownloaded {
        /// Server-side media_id, used to match the message.
        media_id: i64,
        /// Path where the decrypted file was saved.
        path: std::path::PathBuf,
    },
    /// A media operation failed.
    MediaError(String),
    /// A file attachment has been prepared and is ready to stage.
    AttachmentReady(crate::types::PendingAttachment),
    /// Call was successfully initiated — carry call_id, peer details and their public key.
    CallInitiated { call_id: String, peer_username: String, peer_pub_key: String },
    /// Outgoing call REST failed.
    CallError(String),
    /// An event from the running CallService.
    CallServiceEvent(call_svc::CallEvent),
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    screen: Option<Screen>,
    auth: AuthState,
    settings: AppSettings,
    theme: Theme,
    chats: Vec<LocalChat>,
    db: Arc<Mutex<Database>>,
    api: ApiClient,
    tx: mpsc::UnboundedSender<AsyncResult>,
    rx: mpsc::UnboundedReceiver<AsyncResult>,
    logs: Vec<String>,
    should_quit: bool,
    tray_rx: mpsc::UnboundedReceiver<tray::TrayAction>,
    tray_handle: tray::TrayHandle,
    console_hidden: bool,
    // ── VoIP call state ──────────────────────────────────────────────────────
    call_state:    CallState,
    call_service:  Option<call_svc::CallService>,
    call_event_rx: Option<mpsc::UnboundedReceiver<call_svc::CallEvent>>,
    presence_rx:   Option<mpsc::UnboundedReceiver<crate::types::WsFrame>>,
    ringtone_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl App {
    pub fn new() -> Result<Self> {
        let db = Database::open()?;

        // Load persisted settings
        let mut settings = AppSettings::default();
        if let Ok(Some(url)) = storage::load_api_base_url() {
            settings.api_base_url = url;
        }
        if let Ok(Some(color)) = storage::load_theme_color() {
            settings.theme_color = color;
        }
        settings.run_in_background = storage::load_run_in_background();

        let theme = Theme::from_hex(&settings.theme_color);

        // Load or generate device_id
        let device_id = storage::load_device_id()
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                let id = Uuid::new_v4().to_string();
                let _ = storage::save_device_id(&id);
                id
            });

        let mut auth = AuthState {
            device_id: device_id.clone(),
            ..Default::default()
        };

        // Try to restore auth from storage
        let mut api = ApiClient::new(&settings.api_base_url, &device_id);
        let initial_screen;

        if let (Ok(Some(token)), Ok(Some(uid)), Ok(Some(uname))) = (
            storage::load_auth_token(),
            storage::load_user_id(),
            storage::load_username(),
        ) {
            auth.token = token.clone();
            auth.user_id = uid;
            auth.username = uname;
            auth.identity_public_key = storage::load_identity_pub().ok().flatten();
            api.set_token(&token);
            // Note: private key stays None until user enters password again
            // For now, attempt a session-less home (decryption will show [encrypted] until re-auth)
            initial_screen = Screen::Home(HomeScreen::new());
        } else {
            initial_screen = Screen::Onboarding(OnboardingScreen::new(&settings.api_base_url));
        }

        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn system-tray icon
        let (tray_rx, tray_handle) = tray::spawn(settings.run_in_background);

        // Start presence WS if already logged in
        let presence_rx = if auth.user_id != 0 && !auth.token.is_empty() {
            Some(call_svc::connect_presence(
                settings.api_base_url.clone(),
                auth.token.clone(),
                auth.device_id.clone(),
            ))
        } else {
            None
        };

        Ok(Self {
            screen: Some(initial_screen),
            auth,
            settings,
            theme,
            chats: Vec::new(),
            db: Arc::new(Mutex::new(db)),
            api,
            tx,
            rx,
            logs: Vec::new(),
            should_quit: false,
            tray_rx,
            tray_handle,
            console_hidden: false,
            call_state:    CallState::Idle,
            call_service:  None,
            call_event_rx: None,
            presence_rx,
            ringtone_stop: None,
        })
    }

    /// Extract the macOS tray init data so `main()` can run the event loop on the
    /// main thread.  Only meaningful on macOS; call once immediately after `new()`.
    #[cfg(target_os = "macos")]
    pub fn take_macos_tray_init(&mut self) -> Option<crate::services::tray::MacosInit> {
        self.tray_handle.take_macos_init()
    }

    // ── Main run loop ──────────────────────────────────────────────────────────

    pub async fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        let mut events = EventStream::new();
        let mut tick = tokio::time::interval(Duration::from_millis(50));

        // If already logged in, kick off an initial chat sync and auto-decrypt the private key
        if self.auth.user_id != 0 {
            self.spawn_list_chats();
            self.spawn_auto_login();
        }

        loop {
            terminal.draw(|frame| self.render(frame))?;

            tokio::select! {
                _ = tick.tick() => {
                    self.poll_ws_and_decrypt().await;
                }
                Some(Ok(event)) = events.next() => {
                    self.handle_terminal_event(event).await;
                }
                Some(result) = self.rx.recv() => {
                    self.handle_async_result(result).await;
                }
                Some(action) = self.tray_rx.recv() => {
                    self.handle_tray_action(action, terminal);
                }
                // ── VoIP: incoming call / ping frames on presence WS ──────
                Some(frame) = async {
                    if let Some(rx) = self.presence_rx.as_mut() { rx.recv().await } else { None }
                } => {
                    self.handle_presence_frame(frame).await;
                }
                // ── VoIP: call audio/signal events ────────────────────────
                Some(ev) = async {
                    if let Some(rx) = self.call_event_rx.as_mut() { rx.recv().await } else { None }
                } => {
                    self.handle_call_event(ev);
                }
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    // ── Terminal event dispatch ────────────────────────────────────────────────

    async fn handle_terminal_event(&mut self, event: Event) {
        // Global Ctrl+C / Ctrl+Q
        if let Event::Key(key) = &event {
            if key.kind != KeyEventKind::Press { return; }
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                self.should_quit = true;
                return;
            }
        }

        let action = self.dispatch_event_to_screen(&event);
        if let Some(action) = action {
            self.handle_action(action).await;
        }

        // Handle onboarding async submit separately
        if let Some(Screen::Onboarding(ob)) = self.screen.as_mut() {
            if let Event::Key(key) = &event {
                if key.kind == KeyEventKind::Press && ob.is_submit_ready(key) {
                    let (url, username, password, confirm) = ob.form_values();
                    let is_login = ob.is_login_mode();
                    ob.set_loading();
                    // Update base url immediately
                    self.settings.api_base_url = url.clone();
                    self.api = ApiClient::new(&url, &self.auth.device_id);
                    if let Ok(Some(t)) = storage::load_auth_token() {
                        self.api.set_token(&t);
                    }
                    let _ = storage::save_api_base_url(&url);
                    if is_login {
                        self.spawn_login(username, password);
                    } else {
                        self.spawn_signup(username, password, confirm);
                    }
                }
            }
        }

        // Handle onboarding "Test Connection" button
        if let Some(Screen::Onboarding(ob)) = self.screen.as_mut() {
            if let Event::Key(key) = &event {
                if key.kind == KeyEventKind::Press && ob.is_test_ready(key) {
                    let url = ob.api_url_value();
                    ob.set_loading();
                    self.settings.api_base_url = url.clone();
                    self.api = ApiClient::new(&url, &self.auth.device_id);
                    self.spawn_connection_test();
                }
            }
        }
    }

    fn dispatch_event_to_screen(&mut self, event: &Event) -> Option<AppAction> {
        // Mouse events: only routed to the Chat screen (attach-button hit-test)
        if let Event::Mouse(mouse) = event {
            if let Some(Screen::Chat(chat)) = self.screen.as_mut() {
                let (action, _) = chat.handle_mouse(*mouse);
                return action;
            }
            return None;
        }

        let Event::Key(key) = event else { return None };
        // On Windows, Crossterm emits Press + Release for every key — ignore non-Press.
        if key.kind != KeyEventKind::Press { return None; }
        match self.screen.as_mut()? {
            Screen::Onboarding(ob) => ob.handle_key(*key),
            Screen::Home(home) => {
                let chats = self.chats.clone();
                home.handle_key(*key, &chats)
            }
            Screen::Chat(chat) => {
                let (action, send_req) = chat.handle_key(*key);
                if let Some((text, reply_id)) = send_req {
                    let chat_id = chat.chat_id;
                    let with_user = chat.with_user.clone();
                    let ephemeral_secs = chat.ephemeral_secs;
                    self.spawn_send_message(chat_id, text, reply_id, with_user, ephemeral_secs);
                }
                action
            }
            Screen::Call(call_sc) => {
                let (action, filter) = call_sc.handle_key(*key);
                if let Some(fp) = filter {
                    if let Some(svc) = &self.call_service {
                        svc.set_filter(fp);
                    }
                }
                action
            }
            Screen::Settings(settings, _) => {
                let settings_clone = self.settings.clone();
                let (action, new_settings) = settings.handle_key(*key, &settings_clone);
                // Handle settings-triggered async ops
                if key.code == KeyCode::Enter {
                    match settings.last_activated() {
                        crate::screens::settings::SettingsItem::TestConnection => {
                            self.spawn_connection_test();
                        }
                        crate::screens::settings::SettingsItem::Sessions => {
                            self.spawn_list_sessions();
                        }
                        crate::screens::settings::SettingsItem::RevokeOther => {
                            self.spawn_revoke_other();
                        }
                        _ => {}
                    }
                }
                if let Some(ns) = new_settings {
                    // sync run-in-background if toggled
                    if ns.run_in_background != self.settings.run_in_background {
                        let _ = storage::save_run_in_background(ns.run_in_background);
                        self.tray_handle.set_run_in_background(ns.run_in_background);
                    }
                    self.settings = ns;
                    self.theme = Theme::from_hex(&self.settings.theme_color);
                    let _ = storage::save_api_base_url(&self.settings.api_base_url);
                    let _ = storage::save_theme_color(&self.settings.theme_color);
                }
                action
            }
            Screen::Profile(profile, _) => profile.handle_key(*key),
        }
    }

    // ── AppAction handler ──────────────────────────────────────────────────────

    fn handle_tray_action<B: ratatui::backend::Backend>(
        &mut self,
        action: tray::TrayAction,
        terminal: &mut Terminal<B>,
    ) {
        match action {
            tray::TrayAction::Show => {
                tray::show_console_window();
                self.console_hidden = false;
                // Force a redraw so the screen isn't stale
                let _ = terminal.draw(|frame| self.render(frame));
            }
            tray::TrayAction::ToggleBackground(checked) => {
                self.settings.run_in_background = checked;
                let _ = storage::save_run_in_background(checked);
            }
            tray::TrayAction::Quit => {
                // Hard quit — even if run_in_background is on
                self.should_quit = true;
                tray::show_console_window();
                self.console_hidden = false;
            }
        }
    }

    async fn handle_action(&mut self, action: AppAction) {
        match action {
            AppAction::Quit => {
                if self.settings.run_in_background {
                    // Hide to tray instead of quitting
                    tray::hide_console_window();
                    self.console_hidden = true;
                } else {
                    self.should_quit = true;
                }
            }
            AppAction::Back => {
                let new_screen = match self.screen.take() {
                    Some(Screen::Settings(_, under)) | Some(Screen::Profile(_, under)) => {
                        Some(*under)
                    }
                    Some(Screen::Chat(_)) => Some(Screen::Home(HomeScreen::new())),
                    other => other,
                };
                self.screen = new_screen;
                // Refresh chat list when returning to Home
                if matches!(self.screen, Some(Screen::Home(_))) {
                    self.spawn_list_chats();
                }
            }
            AppAction::GoHome => {
                self.screen = Some(Screen::Home(HomeScreen::new()));
                self.spawn_list_chats();
            }
            AppAction::OpenChat { chat_id, with_user } => {
                if chat_id == -1 {
                    // New chat — create it first
                    self.spawn_create_chat(with_user);
                } else {
                    self.open_chat(chat_id, &with_user).await;
                }
            }
            AppAction::OpenSettings => {
                let under = self.screen.take().unwrap_or(Screen::Home(HomeScreen::new()));
                let mut settings_screen = SettingsScreen::new();
                settings_screen.logs = self.logs.clone();
                self.screen = Some(Screen::Settings(settings_screen, Box::new(under)));
            }
            AppAction::OpenProfile => {
                let under = self.screen.take().unwrap_or(Screen::Home(HomeScreen::new()));
                self.screen = Some(Screen::Profile(ProfileScreen, Box::new(under)));
            }
            AppAction::ApiUrlChanged(url) => {
                self.api = ApiClient::new(&url, &self.auth.device_id);
                if !self.auth.token.is_empty() {
                    self.api.set_token(&self.auth.token);
                }
                let _ = storage::save_api_base_url(&url);
            }
            AppAction::ThemeChanged(color) => {
                self.theme = Theme::from_hex(&color);
                let _ = storage::save_theme_color(&color);
            }
            AppAction::Logout => {
                self.spawn_logout();
            }
            AppAction::SendMedia { path, caption, reply_id, ephemeral_secs } => {
                // Look up chat context from the current screen
                if let Some(Screen::Chat(cs)) = self.screen.as_ref() {
                    let chat_id  = cs.chat_id;
                    let with_user = cs.with_user.clone();
                    self.spawn_send_media(chat_id, path, caption, reply_id, with_user, ephemeral_secs);
                    // Show progress in the status bar
                    if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
                        cs.status = "Uploading…".into();
                        cs.upload_progress = Some((0, 1));
                        cs.pending_send = true;
                    }
                }
            }
            AppAction::DownloadMedia { media_id, file_key, file_nonce, filename } => {
                // Mark the message as pending download
                if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
                    if let Some(msg) = cs.messages.iter_mut().find(|m| {
                        m.media_info.as_ref().map_or(false, |i| i.media_id == media_id)
                    }) {
                        msg.download_state = DownloadState::Pending;
                    }
                    cs.status = format!("Downloading media…");
                }
                self.spawn_download_media(media_id, file_key, file_nonce, filename);
            }
            AppAction::OpenFilePicker => {
                self.spawn_file_picker();
            }
            AppAction::PasteFromClipboard => {
                self.spawn_clipboard_read();
            }
            AppAction::CancelAttachment => {
                if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
                    cs.pending_attachment = None;
                    cs.status = String::new();
                }
            }
            // ── VoIP call actions ────────────────────────────────────────────
            AppAction::InitiateCall { peer_username } => {
                self.spawn_initiate_call(peer_username);
            }
            AppAction::AnswerCall { call_id } => {
                // Stop ringtone
                if let Some(flag) = self.ringtone_stop.take() {
                    notification::stop_ringtone(&flag);
                }
                let caller = match &self.call_state {
                    CallState::Ringing { caller, .. } => caller.clone(),
                    _ => return,
                };
                let tx    = self.tx.clone();
                let api   = self.api.clone();
                tokio::spawn(async move {
                    // Fetch peer public key
                    let peer_pub_key = match api.get_user_pubkey(&caller).await {
                        Ok(r) => r.identity_pub,
                        Err(e) => {
                            let _ = tx.send(AsyncResult::CallError(format!("Cannot reach peer: {}", e)));
                            return;
                        }
                    };
                    // Answer on the server
                    let _ = api.answer_call(&call_id).await;
                    // Signal back to start the audio service
                    let _ = tx.send(AsyncResult::CallInitiated {
                        call_id,
                        peer_username: caller,
                        peer_pub_key,
                    });
                });
            }
            AppAction::RejectCall { call_id } => {
                if let Some(flag) = self.ringtone_stop.take() {
                    notification::stop_ringtone(&flag);
                }
                self.call_state = CallState::Idle;
                self.screen = Some(Screen::Home(HomeScreen::new()));
                let api = self.api.clone();
                tokio::spawn(async move {
                    let _ = api.reject_call(&call_id).await;
                });
            }
            AppAction::EndCall => {
                if let Some(flag) = self.ringtone_stop.take() {
                    notification::stop_ringtone(&flag);
                }
                let call_id = match &self.call_state {
                    CallState::Active { call_id, .. }
                    | CallState::Calling { call_id, .. }
                    | CallState::Ringing { call_id, .. } => call_id.clone(),
                    _ => String::new(),
                };
                if let Some(svc) = self.call_service.take() {
                    svc.shutdown();
                }
                self.call_event_rx = None;
                self.call_state = CallState::Idle;
                self.screen = Some(Screen::Home(HomeScreen::new()));
                if !call_id.is_empty() {
                    let api = self.api.clone();
                    tokio::spawn(async move {
                        let _ = api.end_call(&call_id).await;
                    });
                }
            }
        }
    }

    async fn open_chat(&mut self, chat_id: i64, with_user: &str) {
        let chat_screen =
            ChatScreen::new(chat_id, with_user, self.auth.user_id);
        let ws_rx = websocket::connect(
            self.settings.api_base_url.clone(),
            chat_id,
            self.auth.token.clone(),
            self.auth.device_id.clone(),
        );

        let mut cs = chat_screen;
        // Load local messages first
        let local_msgs = {
            let db = self.db.lock().await;
            db.get_messages(chat_id, None, 50).unwrap_or_default()
        };
        // Decrypt what we can
        let decrypted = self.decrypt_messages(chat_id, local_msgs).await;
        cs.merge_messages(decrypted);
        cs.ws_rx = Some(ws_rx);

        self.screen = Some(Screen::Chat(cs));

        // Also fetch fresh from server (with_user needed for epoch key unwrapping)
        self.spawn_fetch_messages(chat_id, None, with_user.to_string());
    }

    // ── WS + decryption tick ───────────────────────────────────────────────────

    async fn poll_ws_and_decrypt(&mut self) {
        // Phase 0: prune expired ephemeral messages
        {
            use chrono::Utc;
            if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
                let now = Utc::now();
                cs.messages.retain(|m| {
                    m.expires_at.map_or(true, |t| t > now)
                });
            }
        }
        // Phase 1: poll the WS receiver and collect messages needing decryption
        // (drop the mutable borrow on self.screen before any async calls)
        let (chat_id, to_decrypt) = {
            let Some(Screen::Chat(cs)) = self.screen.as_mut() else { return };
            let chat_id = cs.chat_id;
            let changed = cs.poll_ws();
            if !changed {
                return;
            }
            let to_decrypt: Vec<LocalMessage> = cs
                .messages
                .iter()
                .filter(|m| m.plaintext.is_none())
                .cloned()
                .collect();
            (chat_id, to_decrypt)
        }; // mutable borrow of self.screen ends here

        if to_decrypt.is_empty() {
            return;
        }

        // Phase 2: decrypt (borrows &self, no conflict now)
        let decrypted = self.decrypt_messages(chat_id, to_decrypt).await;

        // Phase 3: write results back
        if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
            for dm in &decrypted {
                if let Some(m) = cs.messages.iter_mut().find(|m| m.id == dm.id) {
                    m.plaintext  = dm.plaintext.clone();
                    m.media_info = dm.media_info.clone();
                }
            }
        }
        let db = self.db.lock().await;
        for dm in &decrypted {
            let _ = db.upsert_message(dm);
        }
        drop(db);

        // Phase 4: /fah command — play the sound for messages from the other party
        let my_uid = self.auth.user_id;
        for dm in &decrypted {
            if dm.sender_id != my_uid {
                if dm.plaintext.as_deref()
                    .map(|p| p.trim().to_ascii_lowercase().starts_with("/fah"))
                    .unwrap_or(false)
                {
                    notification::play_fah();
                }
            }
        }

        // Phase 5: auto-download newly decrypted image attachments
        let auto_dl: Vec<(i64, [u8; 32], [u8; 12], String)> = {
            if let Some(Screen::Chat(cs)) = self.screen.as_ref() {
                cs.messages.iter()
                    .filter(|m| matches!(m.download_state, DownloadState::None))
                    .filter_map(|m| m.media_info.as_ref()
                        .filter(|i| i.file_type == "image")
                        .map(|i| (i.media_id, i.file_key, i.file_nonce, i.filename.clone())))
                    .collect()
            } else { vec![] }
        };
        if !auto_dl.is_empty() {
            if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
                for (mid, _, _, _) in &auto_dl {
                    if let Some(msg) = cs.messages.iter_mut().find(|m| {
                        m.media_info.as_ref().map_or(false, |i| i.media_id == *mid)
                    }) {
                        msg.download_state = DownloadState::Pending;
                    }
                }
            }
            for (media_id, key, nonce, filename) in auto_dl {
                self.spawn_download_media(media_id, key.to_vec(), nonce.to_vec(), filename);
            }
        }
    }

    async fn decrypt_messages(&self, chat_id: i64, messages: Vec<LocalMessage>) -> Vec<LocalMessage> {
        if self.auth.identity_private_key.is_none() {
            return messages; // can't decrypt without the identity key
        }

        let mut result = Vec::with_capacity(messages.len());
        for mut msg in messages {
            if msg.plaintext.is_some() {
                // Already decrypted — populate media_info if the plaintext is a media envelope
                if msg.media_info.is_none() {
                    if let Some(ref pt) = msg.plaintext.clone() {
                        msg.media_info = extract_media_info(pt);
                    }
                }
                result.push(msg);
                continue;
            }
            // Get epoch key
            let epoch_key = {
                let db = self.db.lock().await;
                db.get_epoch_key(chat_id, msg.epoch_id).unwrap_or(None)
            };
            if let Some(ek) = epoch_key {
                match crypto::aes_gcm_decrypt_message(&msg.ciphertext, &msg.nonce, &ek) {
                    Ok(pt) => {
                        msg.media_info = extract_media_info(&pt);
                        msg.plaintext  = Some(pt);
                    }
                    Err(e) => {
                        self.log(&format!("Decrypt error msg {}: {}", msg.id, e));
                    }
                }
            }
            // If no epoch key available yet, message stays encrypted until
            // spawn_fetch_messages retrieves and stores the key.
            result.push(msg);
        }
        result
    }

    // ── Async spawn helpers ────────────────────────────────────────────────────

    fn log(&self, msg: &str) {
        // We can't mutate self here easily; caller should use tx channel for logs
        // for now this is a no-op stub (App.logs is appended in handle_async_result)
    }

    fn spawn_login(&self, username: String, password: String) {
        let tx = self.tx.clone();
        let api = self.api.clone();
        tokio::spawn(async move {
            let login_resp = api
                .login(crate::services::api::LoginRequest {
                    username: &username,
                    password: &password,
                })
                .await;
            match login_resp {
                Err(e) => {
                    let _ = tx.send(AsyncResult::AuthError(e.to_string()));
                }
                Ok(lr) => {
                    let authed_api = api.clone().with_token(&lr.token);
                    // Fetch user info
                    let me = match authed_api.me().await {
                        Ok(m) => m,
                        Err(e) => {
                            let _ = tx.send(AsyncResult::AuthError(format!("Failed to fetch user info: {}", e)));
                            return;
                        }
                    };
                    // Fetch keyblob and decrypt private key
                    match authed_api.get_keyblob().await {
                        Err(e) => {
                            let _ = tx.send(AsyncResult::AuthError(format!("Keyblob fetch failed: {}", e)));
                        }
                        Ok(blob) => {
                            match crypto::decrypt_identity_private_key(
                                &blob.encrypted_identity_priv,
                                &blob.kdf_salt,
                                &blob.aead_nonce,
                                &password,
                            ) {
                                Err(e) => {
                                    let _ = tx.send(AsyncResult::AuthError(format!("Key decrypt failed: {}", e)));
                                }
                                Ok(priv_key) => {
                                    let _ = tx.send(AsyncResult::LoginSuccess {
                                        token: lr.token,
                                        user_id: me.id,
                                        username: me.username,
                                        pub_key: blob.identity_pub,
                                        priv_key,
                                        password,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    /// On startup, silently re-authenticate using stored credentials so the
    /// private key is available and the session token is always fresh.
    fn spawn_auto_login(&self) {
        let tx = self.tx.clone();
        let mut api = self.api.clone();
        let username = self.auth.username.clone();
        let user_id  = self.auth.user_id;
        let base_url = api.base_url.clone();

        tokio::spawn(async move {
            // Nothing to do if the password was never stored.
            let password = match storage::load_password() {
                Ok(Some(pw)) => pw,
                _ => return,
            };

            // Always call login to get a fresh token (handles token expiry).
            let token = match api.login(crate::services::api::LoginRequest {
                username: &username,
                password: &password,
            }).await {
                Ok(r) => r.token,
                Err(e) => {
                    let msg = friendly_network_error(&e, &base_url);
                    let _ = tx.send(AsyncResult::Error(msg));
                    return;
                }
            };
            api.set_token(&token);

            // Fetch the encrypted key blob with the fresh token.
            let blob = match api.get_keyblob().await {
                Ok(b) => b,
                Err(e) => {
                    let _ = tx.send(AsyncResult::Error(
                        format!("Keyblob fetch failed: {}", e)
                    ));
                    return;
                }
            };

            // Decrypt the identity private key locally.
            match crypto::decrypt_identity_private_key(
                &blob.encrypted_identity_priv,
                &blob.kdf_salt,
                &blob.aead_nonce,
                &password,
            ) {
                Ok(priv_key) => {
                    let _ = tx.send(AsyncResult::SilentLoginSuccess {
                        token, user_id, username, priv_key,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::Error(
                        format!("Key decrypt failed: {}", e)
                    ));
                }
            }
        });
    }

    fn spawn_signup(&self, username: String, password: String, confirm: String) {
        let tx = self.tx.clone();
        let api = self.api.clone();
        tokio::spawn(async move {
            if password != confirm {
                let _ = tx.send(AsyncResult::AuthError("Passwords do not match".into()));
                return;
            }
            if username.is_empty() || password.is_empty() {
                let _ = tx.send(AsyncResult::AuthError("Username and password are required".into()));
                return;
            }

            // Generate identity keypair
            let (pub_key, priv_key) = match crypto::generate_identity_key_pair() {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(AsyncResult::AuthError(format!("Keypair gen failed: {}", e)));
                    return;
                }
            };

            // Encrypt private key with password
            let (enc, salt, nonce) = match crypto::encrypt_identity_private_key(&priv_key, &password) {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(AsyncResult::AuthError(format!("Key encrypt failed: {}", e)));
                    return;
                }
            };

            let result = api
                .signup(crate::services::api::SignupRequest {
                    username: &username,
                    password: &password,
                    identity_pub: &pub_key,
                    encrypted_identity_priv: &enc,
                    kdf_salt: &salt,
                    aead_nonce: &nonce,
                })
                .await;

            match result {
                Err(e) => {
                    let _ = tx.send(AsyncResult::AuthError(e.to_string()));
                }
                Ok(()) => {
                    // Auto-login after signup
                    match api
                        .login(crate::services::api::LoginRequest {
                            username: &username,
                            password: &password,
                        })
                        .await
                    {
                        Err(e) => {
                            let _ = tx.send(AsyncResult::AuthError(format!("Auto-login failed: {}", e)));
                        }
                        Ok(lr) => {
                            let authed_api = api.clone().with_token(&lr.token);
                            let me = match authed_api.me().await {
                                Ok(m) => m,
                                Err(e) => {
                                    let _ = tx.send(AsyncResult::AuthError(format!("Failed to fetch user info: {}", e)));
                                    return;
                                }
                            };
                            let _ = tx.send(AsyncResult::LoginSuccess {
                                token: lr.token,
                                user_id: me.id,
                                username: me.username,
                                pub_key,
                                priv_key,
                                password,
                            });
                        }
                    }
                }
            }
        });
    }

    fn spawn_connection_test(&self) {
        let tx = self.tx.clone();
        let api = self.api.clone();
        tokio::spawn(async move {
            let ok = api.test_connection().await;
            let _ = tx.send(AsyncResult::ConnectionResult(ok));
        });
    }

    fn spawn_list_chats(&self) {
        let tx = self.tx.clone();
        let api = self.api.clone();
        tokio::spawn(async move {
            match api.list_chats().await {
                Ok(r) => {
                    let chats = r
                        .into_iter()
                        .map(|c| LocalChat {
                            chat_id: c.chat_id,
                            with_user: c.with_user,
                            with_user_id: c.with_user_id,
                            last_message: c.last_message,
                            last_message_time: c.last_message_time,
                            unread_count: 0,
                        })
                        .collect();
                    let _ = tx.send(AsyncResult::ChatListLoaded(chats));
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::Error(format!("Chat list: {}", e)));
                }
            }
        });
    }

    fn spawn_fetch_messages(&self, chat_id: i64, before_id: Option<i64>, with_user: String) {
        let tx = self.tx.clone();
        let api = self.api.clone();
        let priv_key = self.auth.identity_private_key.clone();
        let my_pub_key = self.auth.identity_public_key.clone();
        tokio::spawn(async move {
            // 1. Fetch messages
            let wire_messages = match api.fetch_messages(chat_id, before_id, 50).await {
                Ok(r) => r.messages,
                Err(e) => {
                    let _ = tx.send(AsyncResult::Error(format!("Fetch messages: {}", e)));
                    return;
                }
            };

            // 2. For every unique epoch_id, fetch and unwrap the epoch key.
            //    The backend returns the key wrapped specifically for the authenticated user.
            //    If this user is "user_a" (creator), it was wrapped with ECDH(myPriv, myPub);
            //    if "user_b" it was wrapped with ECDH(creatorPriv, myPub) = ECDH(myPriv, creatorPub).
            //    So we try both the peer's pubkey AND our own pubkey.
            let mut epoch_keys: Vec<(i64, i64, String)> = Vec::new();
            if let Some(priv_k) = priv_key {
                let peer_pub = api.get_user_pubkey(&with_user).await.ok().map(|r| r.identity_pub);

                let unique_epoch_ids: std::collections::HashSet<i64> =
                    wire_messages.iter().map(|m| m.epoch_id).collect();

                for eid in unique_epoch_ids {
                    if let Ok(er) = api.fetch_epoch_key(chat_id, eid).await {
                        if er.wrapped_key.is_empty() {
                            continue;
                        }
                        // Try peer pubkey first (correct when we are user_b)
                        let unwrapped = peer_pub.as_deref()
                            .and_then(|ppk| crypto::unwrap_epoch_key(&er.wrapped_key, &priv_k, ppk).ok())
                            // Fall back to our own pubkey (correct when we are user_a)
                            .or_else(|| {
                                my_pub_key.as_deref().and_then(|mpk| {
                                    crypto::unwrap_epoch_key(&er.wrapped_key, &priv_k, mpk).ok()
                                })
                            });

                        if let Some(key) = unwrapped {
                            epoch_keys.push((eid, er.epoch_index, key));
                        }
                    }
                }
            }

            // 3. Convert wire messages to local messages
            let messages: Vec<LocalMessage> = wire_messages
                .into_iter()
                .map(|m| LocalMessage {
                    id: m.id,
                    chat_id,
                    sender_id: m.sender_id,
                    epoch_id: m.epoch_id,
                    reply_id: m.reply_id,
                    ciphertext: m.ciphertext,
                    nonce: m.nonce,
                    plaintext: None,
                    media_info: None,
                    download_state: DownloadState::None,
                    created_at: m.created_at,
                    synced: true,
                    expires_at: m.expires_at,
                    pixel_preview: None,
                })
                .collect();

            let _ = tx.send(AsyncResult::MessagesLoaded { chat_id, messages, epoch_keys });
        });
    }

    fn spawn_send_message(&self, chat_id: i64, plaintext: String, reply_id: Option<i64>, with_user: String, ephemeral_secs: u32) {
        let tx = self.tx.clone();
        let api = self.api.clone();
        let db = self.db.clone();
        let priv_key = self.auth.identity_private_key.clone();
        let my_pub_key = self.auth.identity_public_key.clone();
        let my_user_id = self.auth.user_id;

        tokio::spawn(async move {
            let Some(priv_k) = priv_key else {
                let _ = tx.send(AsyncResult::MessageError("Not authenticated — please log in again".into()));
                return;
            };

            // 1. Look for the latest epoch key in the local DB
            let epoch_key_opt = {
                let db = db.lock().await;
                db.get_latest_epoch_key(chat_id).ok().flatten()
            };

            let (epoch_id, epoch_key) = match epoch_key_opt {
                Some(v) => v,
                None => {
                    // 2. DB miss — fetch the peer's public key (needed for both paths below)
                    let peer_pub = match api.get_user_pubkey(&with_user).await {
                        Ok(r) => r.identity_pub,
                        Err(e) => {
                            let _ = tx.send(AsyncResult::MessageError(
                                format!("Could not fetch peer public key: {}", e),
                            ));
                            return;
                        }
                    };

                    // 3. Fetch the latest message to determine the current epoch
                    let latest_msg = match api.fetch_messages(chat_id, None, 1).await {
                        Ok(r) => r.messages.into_iter().last(),
                        Err(e) => {
                            let _ = tx.send(AsyncResult::MessageError(
                                format!("Failed to load chat history: {}", e),
                            ));
                            return;
                        }
                    };

                    match latest_msg {
                        Some(m) => {
                            // Existing chat — fetch and unwrap the epoch key for the latest epoch.
                            // The backend returns our copy of wrapped_key (a or b depending on role).
                            // Both the web app and mobile app wrap with ECDH(senderPriv, peerPub),
                            // so we always unwrap with ECDH(myPriv, peerPub) regardless of role.
                            let eid = m.epoch_id;
                            let er = match api.fetch_epoch_key(chat_id, eid).await {
                                Ok(e) if !e.wrapped_key.is_empty() => e,
                                Ok(_) => {
                                    let _ = tx.send(AsyncResult::MessageError(
                                        "Epoch key not yet initialised on server".into(),
                                    ));
                                    return;
                                }
                                Err(e) => {
                                    let _ = tx.send(AsyncResult::MessageError(
                                        format!("Could not fetch epoch key: {}", e),
                                    ));
                                    return;
                                }
                            };

                            // Primary: ECDH(myPriv, peerPub) — works for both user_a and user_b
                            // because web/mobile always store the same wrapped key for both sides.
                            // Fallback: ECDH(myPriv, myPub) in case of a legacy key format.
                            let unwrapped = crypto::unwrap_epoch_key(&er.wrapped_key, &priv_k, &peer_pub)
                                .ok()
                                .or_else(|| {
                                    my_pub_key.as_deref().and_then(|mpk| {
                                        crypto::unwrap_epoch_key(&er.wrapped_key, &priv_k, mpk).ok()
                                    })
                                });

                            match unwrapped {
                                Some(key) => {
                                    let db = db.lock().await;
                                    let _ = db.save_epoch_key(chat_id, eid, er.epoch_index, &key);
                                    (eid, key)
                                }
                                None => {
                                    let _ = tx.send(AsyncResult::MessageError(
                                        "Failed to unwrap epoch key — wrong key or corrupted data".into(),
                                    ));
                                    return;
                                }
                            }
                        }
                        None => {
                            // No messages yet — this is the very first send on a fresh chat.
                            // Mirror the web/mobile pattern exactly:
                            //   wrapped_key_a = wrapped_key_b = wrap(epoch_key, myPriv, peerPub)
                            // ECDH symmetry means the peer can also unwrap using their private key.
                            let epoch_key_raw = crypto::generate_aes_key();

                            let wrapped = match crypto::wrap_epoch_key(&epoch_key_raw, &priv_k, &peer_pub) {
                                Ok(w) => w,
                                Err(e) => {
                                    let _ = tx.send(AsyncResult::MessageError(
                                        format!("Failed to wrap epoch key: {}", e),
                                    ));
                                    return;
                                }
                            };

                            // Same wrapped key for both slots (matches web app and mobile app behaviour)
                            let epoch_resp = match api.create_epoch(chat_id, &wrapped, &wrapped).await {
                                Ok(r) => r,
                                Err(e) => {
                                    let _ = tx.send(AsyncResult::MessageError(
                                        format!("Failed to create epoch: {}", e),
                                    ));
                                    return;
                                }
                            };

                            {
                                let db = db.lock().await;
                                let _ = db.save_epoch_key(chat_id, epoch_resp.epoch_id, epoch_resp.epoch_index, &epoch_key_raw);
                            }

                            (epoch_resp.epoch_id, epoch_key_raw)
                        }
                    }
                }
            };

            // 3. Encrypt the message
            let (ciphertext, nonce) = match crypto::aes_gcm_encrypt_message(&plaintext, &epoch_key) {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(AsyncResult::MessageError(e.to_string()));
                    return;
                }
            };

            // 4. Compute optional expiry timestamp
            let expires_at_str: Option<String> = if ephemeral_secs > 0 {
                use chrono::{Duration, Utc};
                Some((Utc::now() + Duration::seconds(ephemeral_secs as i64)).to_rfc3339())
            } else {
                None
            };

            match api
                .send_message(
                    chat_id,
                    crate::services::api::SendMessageRequest {
                        ciphertext: &ciphertext,
                        nonce: &nonce,
                        epoch_id,
                        reply_id,
                        media_id: None,
                        expires_at: expires_at_str,
                    },
                )
                .await
            {
                Ok(resp) => {
                    let local = LocalMessage {
                        id: resp.id,
                        chat_id,
                        sender_id: my_user_id,
                        epoch_id: resp.epoch_id,
                        reply_id,
                        ciphertext,
                        nonce,
                        plaintext: Some(plaintext),
                        media_info: None,
                        download_state: DownloadState::None,
                        created_at: resp.created_at,
                        synced: true,
                        expires_at: None,
                        pixel_preview: None,
                    };
                    let _ = tx.send(AsyncResult::MessageSent { chat_id, message: local });
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::MessageError(e.to_string()));
                }
            }
        });
    }

    /// Encrypt, upload in chunks, and send a media attachment message.
    fn spawn_send_media(
        &self,
        chat_id: i64,
        path: String,
        caption: String,
        reply_id: Option<i64>,
        with_user: String,
        ephemeral_secs: u32,
    ) {
        let tx          = self.tx.clone();
        let api         = self.api.clone();
        let db          = self.db.clone();
        let priv_key    = self.auth.identity_private_key.clone();
        let my_pub_key  = self.auth.identity_public_key.clone();
        let my_user_id  = self.auth.user_id;

        tokio::spawn(async move {
            let Some(priv_k) = priv_key else {
                let _ = tx.send(AsyncResult::MessageError("Not authenticated".into()));
                return;
            };

            // 1. Read the file from disk
            let file_bytes = match tokio::fs::read(&path).await {
                Ok(b) => b,
                Err(e) => {
                    let _ = tx.send(AsyncResult::MessageError(
                        format!("Cannot read file {}: {}", path, e)
                    ));
                    return;
                }
            };

            // 2. Classify by extension
            let filename = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "attachment".to_string());
            let ext = std::path::Path::new(&path)
                .extension()
                .map(|e| e.to_ascii_lowercase().to_string_lossy().to_string())
                .unwrap_or_default();
            let file_type = match ext.as_str() {
                "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "avif" => "image",
                "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v"                   => "video",
                "mp3" | "m4a" | "ogg" | "opus" | "flac" | "wav" | "aac"          => "audio",
                "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx"
                | "txt" | "md" | "csv" | "rtf"                                   => "document",
                _                                                                  => "file",
            };

            // 3. Encrypt the file with a fresh per-file key
            let file_key = crypto::generate_file_key();
            let (enc_blob, file_nonce) = match crypto::encrypt_file(&file_bytes, &file_key) {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(AsyncResult::MessageError(format!("File encrypt: {}", e)));
                    return;
                }
            };

            // 4. Initialise the upload session
            const CHUNK_SIZE: usize = 5 * 1024 * 1024; // 5 MiB
            let total_size   = enc_blob.len() as u64;
            let total_chunks = ((enc_blob.len() + CHUNK_SIZE - 1) / CHUNK_SIZE) as u64;
            let init_resp = match api.init_media_upload(&crate::services::api::MediaInitRequest {
                chat_id,
                total_size,
                chunk_size: CHUNK_SIZE as u64,
                total_chunks,
                file_type: file_type.to_string(),
            }).await {
                Ok(r) => r,
                Err(e) => {
                    let _ = tx.send(AsyncResult::MessageError(format!("Upload init: {}", e)));
                    return;
                }
            };
            let upload_id = init_resp.upload_id;

            // 5. Send chunks
            for (i, chunk) in enc_blob.chunks(CHUNK_SIZE).enumerate() {
                if let Err(e) = api.upload_chunk(&upload_id, i, chunk.to_vec()).await {
                    let _ = tx.send(AsyncResult::MessageError(
                        format!("Chunk {}/{} upload failed: {}", i + 1, total_chunks, e)
                    ));
                    return;
                }
                let _ = tx.send(AsyncResult::UploadProgress {
                    chunk: i + 1,
                    total: total_chunks as usize,
                });
            }

            // 6. Finalise → obtain media_id
            let media_id = match api.finalize_upload(&upload_id).await {
                Ok(r) => r.media_id,
                Err(e) => {
                    let _ = tx.send(AsyncResult::MessageError(format!("Finalize: {}", e)));
                    return;
                }
            };

            // 6a. Cache original bytes locally so sender sees image immediately
            let cache_dir = storage::media_cache_dir();
            let cache_path = cache_dir.join(&filename);
            let _ = std::fs::write(&cache_path, &file_bytes);
            let sender_pixel_preview = if file_type == "image" {
                crate::services::attachment::build_image_preview_pub(&file_bytes, 52, 10)
            } else {
                None
            };

            // 7. Build encrypted message envelope carrying the file key
            let msg_plaintext = crypto::build_media_plaintext(
                &caption, media_id, &file_key, &file_nonce, file_type, &filename,
            );

            // 8. Epoch key lookup (same path as spawn_send_message)
            let epoch_key_opt = {
                let db = db.lock().await;
                db.get_latest_epoch_key(chat_id).ok().flatten()
            };

            let (epoch_id, epoch_key) = match epoch_key_opt {
                Some(v) => v,
                None => {
                    let peer_pub = match api.get_user_pubkey(&with_user).await {
                        Ok(r) => r.identity_pub,
                        Err(e) => {
                            let _ = tx.send(AsyncResult::MessageError(
                                format!("Peer pubkey: {}", e)
                            ));
                            return;
                        }
                    };
                    let latest = match api.fetch_messages(chat_id, None, 1).await {
                        Ok(r) => r.messages.into_iter().last(),
                        Err(e) => {
                            let _ = tx.send(AsyncResult::MessageError(
                                format!("Fetch messages: {}", e)
                            ));
                            return;
                        }
                    };
                    match latest {
                        Some(m) => {
                            let eid = m.epoch_id;
                            let er = match api.fetch_epoch_key(chat_id, eid).await {
                                Ok(e) if !e.wrapped_key.is_empty() => e,
                                _ => {
                                    let _ = tx.send(AsyncResult::MessageError("Epoch not ready".into()));
                                    return;
                                }
                            };
                            let unwrapped = crypto::unwrap_epoch_key(&er.wrapped_key, &priv_k, &peer_pub)
                                .ok()
                                .or_else(|| my_pub_key.as_deref().and_then(|mpk| {
                                    crypto::unwrap_epoch_key(&er.wrapped_key, &priv_k, mpk).ok()
                                }));
                            match unwrapped {
                                Some(key) => {
                                    let db = db.lock().await;
                                    let _ = db.save_epoch_key(chat_id, eid, er.epoch_index, &key);
                                    (eid, key)
                                }
                                None => {
                                    let _ = tx.send(AsyncResult::MessageError("Epoch unwrap failed".into()));
                                    return;
                                }
                            }
                        }
                        None => {
                            let epoch_key_raw = crypto::generate_aes_key();
                            let wrapped = match crypto::wrap_epoch_key(&epoch_key_raw, &priv_k, &peer_pub) {
                                Ok(w) => w,
                                Err(e) => {
                                    let _ = tx.send(AsyncResult::MessageError(format!("Wrap epoch: {}", e)));
                                    return;
                                }
                            };
                            let epoch_resp = match api.create_epoch(chat_id, &wrapped, &wrapped).await {
                                Ok(r) => r,
                                Err(e) => {
                                    let _ = tx.send(AsyncResult::MessageError(format!("Create epoch: {}", e)));
                                    return;
                                }
                            };
                            {
                                let db = db.lock().await;
                                let _ = db.save_epoch_key(chat_id, epoch_resp.epoch_id, epoch_resp.epoch_index, &epoch_key_raw);
                            }
                            (epoch_resp.epoch_id, epoch_key_raw)
                        }
                    }
                }
            };

            // 9. Encrypt the message envelope
            let (ciphertext, nonce) = match crypto::aes_gcm_encrypt_message(&msg_plaintext, &epoch_key) {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(AsyncResult::MessageError(e.to_string()));
                    return;
                }
            };

            // 10a. Compute optional expiry timestamp
            let expires_at_str: Option<String> = if ephemeral_secs > 0 {
                use chrono::{Duration, Utc};
                Some((Utc::now() + Duration::seconds(ephemeral_secs as i64)).to_rfc3339())
            } else {
                None
            };

            // 10. Send the message
            match api.send_message(
                chat_id,
                crate::services::api::SendMessageRequest {
                    ciphertext: &ciphertext,
                    nonce: &nonce,
                    epoch_id,
                    reply_id,
                    media_id: Some(media_id),
                    expires_at: expires_at_str,
                },
            ).await {
                Ok(resp) => {
                    let minfo = MediaInfo {
                        media_id,
                        file_key,
                        file_nonce,
                        file_type: file_type.to_string(),
                        filename,
                        caption: caption.clone(),
                    };
                    let local = LocalMessage {
                        id: resp.id,
                        chat_id,
                        sender_id: my_user_id,
                        epoch_id: resp.epoch_id,
                        reply_id,
                        ciphertext,
                        nonce,
                        plaintext: Some(msg_plaintext),
                        media_info: Some(minfo),
                        download_state: DownloadState::Downloaded(cache_path),
                        created_at: resp.created_at,
                        synced: true,
                        expires_at: None,
                        pixel_preview: sender_pixel_preview,
                    };
                    let _ = tx.send(AsyncResult::MessageSent { chat_id, message: local });
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::MessageError(e.to_string()));
                }
            }
        });
    }

    /// Download an encrypted media blob from the server, decrypt it, and save
    /// the plaintext to the local media cache directory.
    fn spawn_download_media(
        &self,
        media_id: i64,
        file_key: Vec<u8>,
        file_nonce: Vec<u8>,
        filename: String,
    ) {
        let tx  = self.tx.clone();
        let api = self.api.clone();

        tokio::spawn(async move {
            // Download encrypted blob
            let enc_blob = match api.download_media(media_id).await {
                Ok(b) => b,
                Err(e) => {
                    let _ = tx.send(AsyncResult::MediaError(format!("Download: {}", e)));
                    return;
                }
            };

            // Convert key/nonce vecs to fixed-size arrays
            if file_key.len() != 32 || file_nonce.len() != 12 {
                let _ = tx.send(AsyncResult::MediaError("Invalid key/nonce length".into()));
                return;
            }
            let mut key   = [0u8; 32];
            let mut nonce = [0u8; 12];
            key.copy_from_slice(&file_key);
            nonce.copy_from_slice(&file_nonce);

            // Decrypt
            let plaintext = match crypto::decrypt_file(&enc_blob, &nonce, &key) {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(AsyncResult::MediaError(format!("Decrypt: {}", e)));
                    return;
                }
            };

            // Save to media cache
            let cache_dir = storage::media_cache_dir();
            let out_path  = cache_dir.join(&filename);
            if let Err(e) = std::fs::write(&out_path, &plaintext) {
                let _ = tx.send(AsyncResult::MediaError(format!("Save: {}", e)));
                return;
            }

            let _ = tx.send(AsyncResult::MediaDownloaded { media_id, path: out_path });
        });
    }

    /// Open the native OS file-picker dialog (blocking) and build a
    /// `PendingAttachment` from the selected file.
    fn spawn_file_picker(&self) {
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let path = rfd::FileDialog::new()
                .set_title("Attach a file")
                .pick_file()
                .map(|p| p.to_string_lossy().to_string());

            if let Some(p) = path {
                match crate::services::attachment::build_pending_attachment(&p) {
                    Ok(att)  => { let _ = tx.send(AsyncResult::AttachmentReady(att)); }
                    Err(e)   => { let _ = tx.send(AsyncResult::MediaError(
                        format!("Cannot prepare attachment: {}", e)
                    )); }
                }
            }
            // User cancelled — do nothing
        });
    }

    /// Read the system clipboard.  If it contains an image, save it to a temp
    /// PNG and build a `PendingAttachment`; if it contains text that is a valid
    /// path, use that file directly.
    fn spawn_clipboard_read(&self) {
        let tx = self.tx.clone();
        tokio::task::spawn_blocking(move || {
            let mut clipboard = match arboard::Clipboard::new() {
                Ok(c)  => c,
                Err(e) => {
                    let _ = tx.send(AsyncResult::MediaError(
                        format!("Clipboard unavailable: {}", e)
                    ));
                    return;
                }
            };

            // ── Try image first ─────────────────────────────────────────────────
            if let Ok(img_data) = clipboard.get_image() {
                let w = img_data.width  as u32;
                let h = img_data.height as u32;
                let bytes: Vec<u8> = img_data.bytes.into_owned();

                if let Some(rgba) = image::RgbaImage::from_raw(w, h, bytes) {
                    let uuid = uuid::Uuid::new_v4();
                    let temp_path = std::env::temp_dir()
                        .join(format!("omnis_paste_{}.png", uuid));

                    if rgba.save(&temp_path).is_ok() {
                        let p = temp_path.to_string_lossy().to_string();
                        match crate::services::attachment::build_pending_attachment(&p) {
                            Ok(att)  => { let _ = tx.send(AsyncResult::AttachmentReady(att)); }
                            Err(e)   => { let _ = tx.send(AsyncResult::MediaError(e.to_string())); }
                        }
                        return;
                    }
                }
            }

            // ── Try text (may be a file path) ──────────────────────────────────
            if let Ok(text) = clipboard.get_text() {
                let p = text.trim().to_string();
                // Handle Windows paths that may be quoted
                let p = p.trim_matches('"').trim_matches('\'').to_string();
                if std::path::Path::new(&p).exists() {
                    match crate::services::attachment::build_pending_attachment(&p) {
                        Ok(att)  => { let _ = tx.send(AsyncResult::AttachmentReady(att)); }
                        Err(e)   => { let _ = tx.send(AsyncResult::MediaError(e.to_string())); }
                    }
                    return;
                }
            }

            let _ = tx.send(AsyncResult::MediaError(
                "Clipboard does not contain an image or a file path".into()
            ));
        });
    }

    fn spawn_create_chat(&self, with_user: String) {
        let tx = self.tx.clone();
        let api = self.api.clone();
        let my_priv_key = self.auth.identity_private_key.clone();
        tokio::spawn(async move {
            match api.create_chat(&with_user).await {
                Ok(resp) => {
                    let chat_id = resp.chat_id;
                    // Create the initial epoch if we have keys.
                    // Match the web/mobile pattern: wrap_key = ECDH(myPriv, peerPub),
                    // store the same wrapped key for both slots (ECDH symmetry lets either
                    // party unwrap with ECDH(theirPriv, otherPub)).
                    if let Some(priv_k) = my_priv_key {
                        if let Ok(peer) = api.get_user_pubkey(&with_user).await {
                            let epoch_key = crypto::generate_aes_key();
                            if let Ok(wrapped) = crypto::wrap_epoch_key(&epoch_key, &priv_k, &peer.identity_pub) {
                                if let Ok(epoch_resp) = api.create_epoch(chat_id, &wrapped, &wrapped).await {
                                    let _ = tx.send(AsyncResult::EpochCreated {
                                        chat_id,
                                        epoch_id: epoch_resp.epoch_id,
                                        epoch_index: epoch_resp.epoch_index,
                                        key: epoch_key,
                                    });
                                }
                            }
                        }
                    }
                    let _ = tx.send(AsyncResult::ChatCreated { chat_id, with_user });
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::Error(format!("Create chat: {}", e)));
                }
            }
        });
    }

    fn spawn_logout(&self) {
        let tx = self.tx.clone();
        let api = self.api.clone();
        tokio::spawn(async move {
            let _ = api.logout().await;
            let _ = tx.send(AsyncResult::LogOutDone);
        });
    }

    fn spawn_list_sessions(&self) {
        let tx = self.tx.clone();
        let api = self.api.clone();
        tokio::spawn(async move {
            match api.list_sessions().await {
                Ok(sessions) => {
                    let _ = tx.send(AsyncResult::SessionsLoaded(sessions));
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::Error(e.to_string()));
                }
            }
        });
    }

    fn spawn_revoke_other(&self) {
        let tx = self.tx.clone();
        let api = self.api.clone();
        tokio::spawn(async move {
            match api.revoke_other_sessions().await {
                Ok(()) => {
                    let _ = tx.send(AsyncResult::Error("Other sessions revoked.".into()));
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::Error(e.to_string()));
                }
            }
        });
    }

    // ── Async result handler ──────────────────────────────────────────────────

    async fn handle_async_result(&mut self, result: AsyncResult) {
        match result {
            AsyncResult::LoginSuccess { token, user_id, username, pub_key, priv_key, password } => {
                self.auth.token = token.clone();
                self.auth.user_id = user_id;
                self.auth.username = username.clone();
                self.auth.identity_public_key = Some(pub_key.clone());
                self.auth.identity_private_key = Some(priv_key);
                self.api.set_token(&token);

                let _ = storage::save_auth_token(&token);
                let _ = storage::save_user_id(user_id);
                let _ = storage::save_username(&username);
                let _ = storage::save_identity_pub(&pub_key);
                let _ = storage::save_password(&password);

                self.logs.push(format!("Logged in as {}", username));
                self.screen = Some(Screen::Home(HomeScreen::new()));
                self.spawn_list_chats();
                // Start presence WS now we have a valid token
                self.presence_rx = Some(call_svc::connect_presence(
                    self.settings.api_base_url.clone(),
                    token.clone(),
                    self.auth.device_id.clone(),
                ));
            }

            AsyncResult::AutoLoginSuccess { priv_key } => {
                self.auth.identity_private_key = Some(priv_key);
                self.logs.push("Auto-login: private key decrypted".into());
            }

            AsyncResult::SilentLoginSuccess { token, user_id, username, priv_key } => {
                self.auth.token = token.clone();
                self.auth.user_id = user_id;
                self.auth.username = username.clone();
                self.auth.identity_private_key = Some(priv_key);
                self.api.set_token(&token);
                let _ = storage::save_auth_token(&token);
                self.logs.push(format!("Silent sign-in as {} succeeded", username));
                // Refresh chats now that we have a valid token
                self.spawn_list_chats();
                // (Re-)start presence WS with the refreshed token
                self.presence_rx = Some(call_svc::connect_presence(
                    self.settings.api_base_url.clone(),
                    token.clone(),
                    self.auth.device_id.clone(),
                ));
                if let Some(Screen::Home(h)) = self.screen.as_mut() {
                    h.status = String::new(); // clear any previous error
                }
                // If a chat is already open, re-fetch messages so epoch keys are
                // unwrapped and stored now that the private key is available.
                // (The first fetch ran before SilentLoginSuccess and had priv_key = None.)
                if let Some(Screen::Chat(cs)) = self.screen.as_ref() {
                    let chat_id = cs.chat_id;
                    let with_user = cs.with_user.clone();
                    self.spawn_fetch_messages(chat_id, None, with_user);
                }
            }

            AsyncResult::SignupSuccess => {
                if let Some(Screen::Onboarding(ob)) = self.screen.as_mut() {
                    ob.set_status("Success! Logging in…");
                }
            }

            AsyncResult::AuthError(msg) => {
                if let Some(Screen::Onboarding(ob)) = self.screen.as_mut() {
                    ob.set_status(format!("Error: {}", msg));
                }
                self.logs.push(format!("Auth error: {}", msg));
            }

            AsyncResult::ChatListLoaded(chats) => {
                // Persist to DB and update in-memory list
                {
                    let db = self.db.lock().await;
                    for c in &chats {
                        let _ = db.upsert_chat(c);
                    }
                }
                self.chats = chats;
                if let Some(Screen::Home(home)) = self.screen.as_mut() {
                    home.status = String::new();
                }
            }

            AsyncResult::MessagesLoaded { chat_id, messages, epoch_keys } => {
                // Store epoch keys (already unwrapped by spawn_fetch_messages)
                {
                    let db = self.db.lock().await;
                    for (epoch_id, epoch_index, key) in &epoch_keys {
                        let _ = db.save_epoch_key(chat_id, *epoch_id, *epoch_index, key);
                    }
                    for msg in &messages {
                        let _ = db.upsert_message(msg);
                    }
                }

                // Decrypt and merge into chat screen if open
                let decrypted = self.decrypt_messages(chat_id, messages).await;
                let auto_dl: Vec<(i64, [u8; 32], [u8; 12], String)> =
                    if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
                        if cs.chat_id == chat_id {
                            cs.merge_messages(decrypted);
                            cs.pending_send = false;
                            let imgs = cs.messages.iter()
                                .filter(|m| matches!(m.download_state, DownloadState::None))
                                .filter_map(|m| m.media_info.as_ref()
                                    .filter(|i| i.file_type == "image")
                                    .map(|i| (i.media_id, i.file_key, i.file_nonce, i.filename.clone())))
                                .collect::<Vec<_>>();
                            for (mid, _, _, _) in &imgs {
                                if let Some(msg) = cs.messages.iter_mut().find(|m| {
                                    m.media_info.as_ref().map_or(false, |i| i.media_id == *mid)
                                }) {
                                    msg.download_state = DownloadState::Pending;
                                }
                            }
                            imgs
                        } else { vec![] }
                    } else { vec![] };
                for (media_id, key, nonce, filename) in auto_dl {
                    self.spawn_download_media(media_id, key.to_vec(), nonce.to_vec(), filename);
                }
            }

            AsyncResult::MessageSent { chat_id, message } => {
                {
                    let db = self.db.lock().await;
                    let _ = db.upsert_message(&message);
                }
                if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
                    if cs.chat_id == chat_id {
                        cs.merge_messages(vec![message]);
                        cs.pending_send    = false;
                        cs.upload_progress = None;
                        cs.status          = String::new();
                    }
                }
            }

            AsyncResult::MessageError(msg) => {
                if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
                    cs.status = format!("Send error: {}", msg);
                    cs.pending_send = false;
                    cs.upload_progress = None;
                }
                self.logs.push(format!("Message error: {}", msg));
            }

            AsyncResult::UploadProgress { chunk, total } => {
                if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
                    cs.upload_progress = Some((chunk, total));
                    let pct = chunk * 100 / total.max(1);
                    cs.status = format!("Uploading… {}/{} chunks ({}%)", chunk, total, pct);
                }
            }

            AsyncResult::MediaDownloaded { media_id, path } => {
                let path_str = path.display().to_string();
                if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
                    // Update the download_state of the matching message and build pixel preview
                    if let Some(msg) = cs.messages.iter_mut().find(|m| {
                        m.media_info.as_ref().map_or(false, |i| i.media_id == media_id)
                    }) {
                        msg.download_state = DownloadState::Downloaded(path.clone());
                        // Build a terminal pixel preview for image attachments
                        if msg.pixel_preview.is_none() {
                            if let Some(ref info) = msg.media_info {
                                if info.file_type == "image" {
                                    if let Ok(bytes) = std::fs::read(&path) {
                                        msg.pixel_preview = crate::services::attachment::build_image_preview_pub(&bytes, 52, 10);
                                    }
                                }
                            }
                        }
                    }
                    cs.status = format!("Saved to {}", path_str);
                }
                self.logs.push(format!("Media {} saved to {}", media_id, path_str));
            }

            AsyncResult::MediaError(msg) => {
                if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
                    // Mark any Pending messages as Failed
                    for m in cs.messages.iter_mut() {
                        if matches!(m.download_state, DownloadState::Pending) {
                            m.download_state = DownloadState::Failed(msg.clone());
                        }
                    }
                    cs.status = format!("Media error: {}", msg);
                }
                self.logs.push(format!("Media error: {}", msg));
            }

            AsyncResult::AttachmentReady(att) => {
                if let Some(Screen::Chat(cs)) = self.screen.as_mut() {
                    let label = format!(
                        "Attached: {}  ({})  — type a caption then Enter, or Esc to cancel",
                        att.filename,
                        crate::services::attachment::format_file_size(att.file_size),
                    );
                    cs.pending_attachment = Some(att);
                    cs.status = label;
                }
            }

            AsyncResult::ChatCreated { chat_id, with_user } => {
                self.open_chat(chat_id, &with_user).await;
                self.spawn_list_chats();
            }

            AsyncResult::EpochCreated { chat_id, epoch_id, epoch_index, key } => {
                let db = self.db.lock().await;
                let _ = db.save_epoch_key(chat_id, epoch_id, epoch_index, &key);
                self.logs.push(format!("Epoch {} created for chat {}", epoch_index, chat_id));
            }

            AsyncResult::LogOutDone => {
                let _ = storage::clear_all();
                let _ = {
                    let db = self.db.lock().await;
                    db.delete_all()
                };
                self.auth = AuthState {
                    device_id: self.auth.device_id.clone(),
                    ..Default::default()
                };
                self.chats.clear();
                self.api.clear_token();
                self.screen = Some(Screen::Onboarding(OnboardingScreen::new(&self.settings.api_base_url)));
            }

            AsyncResult::ConnectionResult(ok) => {
                let msg = if ok { "✓ Connection OK" } else { "✗ Connection failed — check the URL" };
                if let Some(Screen::Settings(s, _)) = self.screen.as_mut() {
                    s.status = msg.to_string();
                }
                if let Some(Screen::Onboarding(ob)) = self.screen.as_mut() {
                    ob.set_status(msg);
                }
            }

            AsyncResult::SessionsLoaded(sessions) => {
                if let Some(Screen::Settings(s, _)) = self.screen.as_mut() {
                    s.sessions = sessions.clone();
                    s.status = format!("{} active session(s)", sessions.len());
                }
            }

            AsyncResult::Error(msg) => {
                self.logs.push(msg.clone());
                // Show on whatever screen is currently active
                match self.screen.as_mut() {
                    Some(Screen::Home(h)) => h.status = msg.clone(),
                    Some(Screen::Settings(s, _)) => s.status = msg.clone(),
                    _ => {}
                }
            }

            // ── VoIP ──────────────────────────────────────────────────────────────────

            AsyncResult::CallInitiated { call_id, peer_username, peer_pub_key } => {
                let my_priv = match self.auth.identity_private_key.clone() {
                    Some(k) => k,
                    None => {
                        self.logs.push("Cannot start call: private key not loaded".into());
                        return;
                    }
                };
                match call_svc::CallService::start(
                    call_id.clone(),
                    peer_username.clone(),
                    peer_pub_key,
                    my_priv,
                    self.settings.api_base_url.clone(),
                    self.auth.token.clone(),
                    self.auth.device_id.clone(),
                ).await {
                    Ok((svc, ev_rx)) => {
                        self.call_service  = Some(svc);
                        self.call_event_rx = Some(ev_rx);
                        self.call_state = CallState::Calling {
                            call_id: call_id.clone(),
                            peer: peer_username.clone(),
                        };
                        let cs = CallScreen::new(self.call_state.clone());
                        self.screen = Some(Screen::Call(cs));
                    }
                    Err(e) => {
                        self.logs.push(format!("Call audio init failed: {}", e));
                        self.call_state = CallState::Idle;
                        if let Some(f) = self.ringtone_stop.take() {
                            notification::stop_ringtone(&f);
                        }
                        self.screen = Some(Screen::Home(HomeScreen::new()));
                    }
                }
            }

            AsyncResult::CallError(msg) => {
                self.logs.push(format!("Call error: {}", msg));
                self.call_state = CallState::Idle;
                if let Some(flag) = self.ringtone_stop.take() {
                    notification::stop_ringtone(&flag);
                }
                if let Some(svc) = self.call_service.take() { svc.shutdown(); }
                self.call_event_rx = None;
                self.screen = Some(Screen::Home(HomeScreen::new()));
            }

            AsyncResult::CallServiceEvent(ev) => {
                self.handle_call_event(ev);
            }
        }
    }

    // ── VoIP helpers ──────────────────────────────────────────────────────────

    /// Called when a frame arrives on the presence (push) WebSocket.
    async fn handle_presence_frame(&mut self, frame: crate::types::WsFrame) {
        use crate::types::WsFrame;
        match frame {
            WsFrame::CallInvite { call_id, caller_username, .. } => {
                // Stop any previous ringtone
                if let Some(f) = self.ringtone_stop.take() {
                    notification::stop_ringtone(&f);
                }
                // OS notification + ringtone
                self.ringtone_stop = Some(notification::notify_incoming_call(&caller_username));
                // Update call state and show call screen
                self.call_state = CallState::Ringing {
                    call_id: call_id.clone(),
                    caller: caller_username.clone(),
                };
                let cs = CallScreen::new(self.call_state.clone());
                self.screen = Some(Screen::Call(cs));
            }
            _ => {} // pings and other frames ignored here
        }
    }

    /// Called when a [CallEvent] arrives from the running [call_svc::CallService].
    fn handle_call_event(&mut self, ev: call_svc::CallEvent) {
        match ev {
            call_svc::CallEvent::Answered => {
                // Stop ringtone (shouldn't be playing as caller, but just in case)
                if let Some(f) = self.ringtone_stop.take() {
                    notification::stop_ringtone(&f);
                }
                if let CallState::Calling { call_id, peer } = &self.call_state {
                    self.call_state = CallState::Active {
                        call_id:    call_id.clone(),
                        peer:       peer.clone(),
                        start_time: chrono::Utc::now(),
                    };
                }
                if let Some(Screen::Call(cs)) = self.screen.as_mut() {
                    cs.call_state  = self.call_state.clone();
                    cs.call_start  = Some(std::time::Instant::now());
                    cs.status      = "Connected".into();
                }
            }
            call_svc::CallEvent::Rejected => {
                if let Some(f) = self.ringtone_stop.take() {
                    notification::stop_ringtone(&f);
                }
                if let Some(svc) = self.call_service.take() { svc.shutdown(); }
                self.call_event_rx = None;
                self.call_state    = CallState::Ended { reason: "Rejected".into() };
                self.screen        = Some(Screen::Home(HomeScreen::new()));
            }
            call_svc::CallEvent::Ended => {
                if let Some(f) = self.ringtone_stop.take() {
                    notification::stop_ringtone(&f);
                }
                if let Some(svc) = self.call_service.take() { svc.shutdown(); }
                self.call_event_rx = None;
                self.call_state    = CallState::Ended { reason: "Call ended".into() };
                self.screen        = Some(Screen::Home(HomeScreen::new()));
            }
            call_svc::CallEvent::Error(msg) => {
                self.logs.push(format!("Call error: {}", msg));
                if let Some(f) = self.ringtone_stop.take() {
                    notification::stop_ringtone(&f);
                }
                if let Some(svc) = self.call_service.take() { svc.shutdown(); }
                self.call_event_rx = None;
                self.call_state    = CallState::Idle;
                self.screen        = Some(Screen::Home(HomeScreen::new()));
            }
            call_svc::CallEvent::LocalLevel(level) => {
                if let Some(Screen::Call(cs)) = self.screen.as_mut() {
                    cs.local_level = level;
                }
            }
            call_svc::CallEvent::RemoteLevel(level) => {
                if let Some(Screen::Call(cs)) = self.screen.as_mut() {
                    cs.remote_level = level;
                }
            }
            call_svc::CallEvent::HoldChanged(held) => {
                if let Some(Screen::Call(cs)) = self.screen.as_mut() {
                    cs.held = held;
                }
            }
        }
    }

    /// Spawns a task that:
    ///   1. Looks up the peer's public key from the server
    ///   2. POSTs /call/initiate
    ///   3. Sends AsyncResult::CallInitiated (or CallError) back to the app loop
    fn spawn_initiate_call(&self, peer_username: String) {
        let tx      = self.tx.clone();
        let api     = self.api.clone();
        tokio::spawn(async move {
            // 1. Get peer public key
            let peer_pub_key = match api.get_user_pubkey(&peer_username).await {
                Ok(r) => r.identity_pub,
                Err(e) => {
                    let _ = tx.send(AsyncResult::CallError(format!("Cannot reach peer: {}", e)));
                    return;
                }
            };
            // 2. Initiate call
            match api.initiate_call(&peer_username, None).await {
                Ok(resp) => {
                    let _ = tx.send(AsyncResult::CallInitiated {
                        call_id: resp.call_id,
                        peer_username,
                        peer_pub_key,
                    });
                }
                Err(e) => {
                    let _ = tx.send(AsyncResult::CallError(format!("Call initiation failed: {}", e)));
                }
            }
        });
    }

    // ── Render ────────────────────────────────────────────────────────────────

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let theme = self.theme.clone();

        match self.screen.as_mut().unwrap() {
            Screen::Onboarding(ob) => ob.render(frame, area, &theme),
            Screen::Home(home) => {
                let chats = self.chats.clone();
                home.render(frame, area, &chats, &theme);
            }
            Screen::Chat(cs) => cs.render(frame, area, &theme),
            Screen::Call(cs) => cs.render(frame, area, &theme),
            Screen::Settings(settings, _) => {
                let s = self.settings.clone();
                settings.render(frame, area, &s, &theme);
            }
            Screen::Profile(profile, _) => {
                let auth = self.auth.clone();
                profile.render(frame, area, &auth, &theme);
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse a message plaintext and extract `MediaInfo` if it is a media envelope.
fn extract_media_info(plaintext: &str) -> Option<crate::types::MediaInfo> {
    use crate::services::crypto::{parse_plaintext, ParsedMessage};
    match parse_plaintext(plaintext) {
        ParsedMessage::Media {
            caption,
            media_id,
            file_key,
            file_nonce,
            file_type,
            filename,
        } => Some(crate::types::MediaInfo {
            media_id,
            file_key,
            file_nonce,
            file_type,
            filename,
            caption,
        }),
        _ => None,
    }
}

/// Turn an `AppError` into a short, human-readable status line.
/// Distinguishes network/connectivity problems from credential issues.
fn friendly_network_error(e: &crate::error::AppError, base_url: &str) -> String {
    use crate::error::AppError;
    match e {
        AppError::Network(re) if re.is_connect() || re.is_timeout() => {
            format!(
                "Cannot reach server at {} — check API URL and your connection.",
                base_url
            )
        }
        AppError::Auth(_) => {
            "Saved credentials rejected by server — please log in again.".into()
        }
        AppError::Other(msg) if msg.contains("401") => {
            "Session expired — please log in again.".into()
        }
        _ => format!("Sign-in failed: {}", e),
    }
}
