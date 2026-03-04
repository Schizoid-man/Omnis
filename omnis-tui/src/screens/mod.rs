pub mod call;
pub mod chat;
pub mod home;
pub mod onboarding;
pub mod profile;
pub mod settings;

/// Actions that screens can emit to the top-level App router.
#[derive(Debug)]
pub enum AppAction {
    /// Navigate to Home after successful login/signup.
    GoHome,
    /// Open a specific chat.
    OpenChat { chat_id: i64, with_user: String },
    /// Go back to the previous screen.
    Back,
    /// Open settings overlay.
    OpenSettings,
    /// Open profile overlay.
    OpenProfile,
    /// The API base URL was changed in settings — rebuild the client.
    ApiUrlChanged(String),
    /// Theme colour changed.
    ThemeChanged(String),
    /// User logged out.
    Logout,
    /// Request app quit.
    Quit,
    /// User typed `/attach <path> [caption]` — start an encrypted media upload.
    SendMedia {
        path:          String,
        caption:       String,
        reply_id:      Option<i64>,
        /// Seconds until the message self-destructs (0 = no expiry).
        ephemeral_secs: u32,
    },
    /// User pressed Ctrl+D on a media message — download and decrypt the blob.
    DownloadMedia {
        media_id:   i64,
        file_key:   Vec<u8>,   // 32 bytes
        file_nonce: Vec<u8>,   // 12 bytes
        filename:   String,
    },
    /// Open the native OS file-picker dialog.
    OpenFilePicker,
    /// Paste an image or file path from the system clipboard.
    PasteFromClipboard,
    /// Discard the staged attachment without sending.
    CancelAttachment,
    // ── VoIP call actions ───────────────────────────────────────────────────
    /// Initiate a call to the peer user of the currently-selected chat.
    InitiateCall { peer_username: String },
    /// User accepted an incoming call.
    AnswerCall { call_id: String },
    /// User rejected an incoming call.
    RejectCall { call_id: String },
    /// Either party ended / the call was answered elsewhere.
    EndCall,
}
