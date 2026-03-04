use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;
use zeroize::{Zeroize, ZeroizeOnDrop};

// ── Flexible datetime deserialiser ──────────────────────────────────────────
//
// FastAPI serialises naive Python datetimes without a timezone suffix
// (e.g. "2026-01-01T12:00:00.123456"). chrono's default serde impl requires
// RFC-3339 (with +00:00 / Z).  This helper accepts both formats and
// treats naive timestamps as UTC.

pub fn de_dt<'de, D>(d: D) -> std::result::Result<DateTime<Utc>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    parse_dt_str(&s).map_err(|_| {
        serde::de::Error::custom(format!("cannot parse datetime: {s}"))
    })
}

fn de_dt_opt<'de, D>(d: D) -> std::result::Result<Option<DateTime<Utc>>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(d)?;
    match opt {
        None => Ok(None),
        Some(s) => parse_dt_str(&s)
            .map(Some)
            .map_err(|_| serde::de::Error::custom(format!("cannot parse datetime: {s}"))),
    }
}

fn parse_dt_str(s: &str) -> std::result::Result<DateTime<Utc>, ()> {
    // RFC-3339 / ISO-8601 with timezone — e.g. "2026-01-01T12:00:00Z"
    if let Ok(dt) = s.parse::<DateTime<Utc>>() {
        return Ok(dt);
    }
    // Try appending Z for naive timestamps — e.g. "2026-01-01T12:00:00.123456"
    if let Ok(dt) = format!("{s}Z").parse::<DateTime<Utc>>() {
        return Ok(dt);
    }
    // Fallback: parse as NaiveDateTime and assume UTC
    for fmt in &["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(DateTime::from_naive_utc_and_offset(ndt, Utc));
        }
    }
    Err(())
}

// ── Wire types (match server JSON exactly) ─────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireMessage {
    pub id: i64,
    pub sender_id: i64,
    pub epoch_id: i64,
    pub reply_id: Option<i64>,
    /// Set when this message has an encrypted media attachment.
    #[serde(default)]
    pub media_id: Option<i64>,
    pub ciphertext: String,
    pub nonce: String,
    #[serde(deserialize_with = "de_dt")]
    pub created_at: DateTime<Utc>,
    #[serde(default, deserialize_with = "de_dt_opt")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireChat {
    pub chat_id: i64,
    pub with_user: String,
    pub with_user_id: Option<i64>,
    pub last_message: Option<String>,
    #[serde(default, deserialize_with = "de_dt_opt")]
    pub last_message_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireChatEpoch {
    pub id: i64,
    pub chat_id: i64,
    pub epoch_index: i64,
    pub wrapped_key: String,
    #[serde(deserialize_with = "de_dt")]
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireSession {
    pub id: i64,
    pub device_id: String,
    pub user_agent: Option<String>,
    #[serde(deserialize_with = "de_dt")]
    pub last_accessed: DateTime<Utc>,
    #[serde(deserialize_with = "de_dt")]
    pub created_at: DateTime<Utc>,
    #[serde(deserialize_with = "de_dt")]
    pub expires_at: DateTime<Utc>,
    #[serde(rename = "current")]
    pub is_current: bool,
}

// ── Media ─────────────────────────────────────────────────────────────────

/// Metadata for an encrypted media attachment decoded from a message plaintext.
///
/// `file_key` and `file_nonce` are in-memory only and zeroed on drop.
/// They are embedded inside the epoch-encrypted ciphertext — the server
/// never receives them in plaintext.
#[derive(Debug, Clone, Zeroize, ZeroizeOnDrop)]
pub struct MediaInfo {
    pub media_id:   i64,
    /// 32-byte AES-256-GCM key used to encrypt the blob on the server.
    pub file_key:   [u8; 32],
    /// 12-byte AES-GCM nonce used when encrypting the blob.
    pub file_nonce: [u8; 12],
    pub file_type:  String,
    pub filename:   String,
    /// Caption / text extracted from the media envelope (not secret, just display text).
    pub caption:    String,
}

/// Runtime download state for a media attachment (not persisted).
#[derive(Debug, Clone, Default)]
pub enum DownloadState {
    /// No download attempted.
    #[default]
    None,
    /// Download + decryption in progress.
    Pending,
    /// Successfully decrypted and saved to this path.
    Downloaded(PathBuf),
    /// Last attempt failed.
    Failed(String),
}

// ── Local DB types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LocalChat {
    pub chat_id: i64,
    pub with_user: String,
    pub with_user_id: Option<i64>,
    pub last_message: Option<String>,
    pub last_message_time: Option<DateTime<Utc>>,
    pub unread_count: i64,
}

#[derive(Debug, Clone)]
pub struct LocalMessage {
    pub id: i64,
    pub chat_id: i64,
    pub sender_id: i64,
    pub epoch_id: i64,
    pub reply_id: Option<i64>,
    pub ciphertext: String,
    pub nonce: String,
    pub plaintext: Option<String>,
    /// Populated after decryption when the plaintext is a media JSON envelope.
    pub media_info: Option<MediaInfo>,
    /// Runtime download state (not stored in SQLite).
    pub download_state: DownloadState,
    pub created_at: DateTime<Utc>,
    pub synced: bool,
    /// Optional self-destruct time (mirrors server expires_at).
    pub expires_at: Option<DateTime<Utc>>,
    /// Terminal pixel-preview rows generated after image download.
    pub pixel_preview: Option<Vec<Vec<PreviewCell>>>,
}

// ── Auth ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct AuthState {
    pub token: String,
    pub device_id: String,
    pub user_id: i64,
    pub username: String,
    /// Decrypted PKCS8 private key bytes (base64), kept in memory only
    pub identity_private_key: Option<String>,
    pub identity_public_key: Option<String>,
}

// ── Attachment preview ──────────────────────────────────────────────────────

/// A single terminal cell in an image preview.
/// (Unicode character, foreground RGB, background RGB)
pub type PreviewCell = (char, (u8, u8, u8), (u8, u8, u8));

/// A file that the user has staged to send (from file picker or clipboard).
#[derive(Debug, Clone)]
pub struct PendingAttachment {
    /// Absolute path on disk (may be a temp file for clipboard images).
    pub path: String,
    pub filename: String,
    /// One of: "image", "video", "audio", "document", "file"
    pub file_type: String,
    pub file_size: u64,
    /// Optional caption typed by the user before sending.
    pub caption: String,
    /// Pre-rendered half-block pixel preview rows for image files.
    /// Each inner Vec is one terminal row of coloured '▀' cells.
    pub pixel_preview: Option<Vec<Vec<PreviewCell>>>,
}

// ── Key pair ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct KeyPair {
    /// base64-encoded SPKI DER public key
    pub public_key: String,
    /// base64-encoded PKCS8 DER private key
    pub private_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBlob {
    pub identity_pub: String,
    pub encrypted_identity_priv: String,
    pub kdf_salt: String,
    pub aead_nonce: String,
}

// ── WS frames ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsFrame {
    History {
        messages: Vec<WireMessage>,
        next_cursor: Option<i64>,
    },
    NewMessage {
        message: WireMessage,
    },
    MessageDeleted {
        message_id: i64,
    },
    Pong,
    /// An incoming VoIP call invite pushed to the callee's presence socket.
    CallInvite {
        call_id: String,
        caller_username: String,
        initiated_at: String,
    },
}

// ── VoIP call state ───────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum CallState {
    Idle,
    /// Incoming call waiting for user action.
    Ringing { call_id: String, caller: String },
    /// Outgoing call, waiting for the callee to answer.
    Calling { call_id: String, peer: String },
    /// Both parties connected, audio flowing.
    Active { call_id: String, peer: String, start_time: chrono::DateTime<chrono::Utc> },
    /// Call finished (ended, rejected, missed).
    Ended { reason: String },
}

impl Default for CallState {
    fn default() -> Self { Self::Idle }
}

/// Noise filter preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterPreset {
    #[default]
    QuietRoom,
    Office,
    Outdoor,
    HeavyNoise,
    Custom,
}

impl FilterPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::QuietRoom  => "Quiet room",
            Self::Office     => "Office",
            Self::Outdoor    => "Outdoor",
            Self::HeavyNoise => "Heavy noise",
            Self::Custom     => "Custom",
        }
    }

    /// Return the default FilterParams for this preset.
    pub fn params(self) -> FilterParams {
        match self {
            Self::QuietRoom  => FilterParams { preset: self, suppression: 0.3, gate_db: -55.0, highpass_hz: 80.0 },
            Self::Office     => FilterParams { preset: self, suppression: 0.6, gate_db: -50.0, highpass_hz: 150.0 },
            Self::Outdoor    => FilterParams { preset: self, suppression: 0.75, gate_db: -45.0, highpass_hz: 200.0 },
            Self::HeavyNoise => FilterParams { preset: self, suppression: 1.0, gate_db: -40.0, highpass_hz: 300.0 },
            Self::Custom     => FilterParams::default(),
        }
    }
}

/// Real-time noise filter parameters.
#[derive(Debug, Clone)]
pub struct FilterParams {
    pub preset:      FilterPreset,
    /// 0.0 = no suppression, 1.0 = full RNNoise suppression.
    pub suppression: f32,
    /// Noise gate threshold in dBFS. Samples below this level are silenced.
    pub gate_db:     f32,
    /// High-pass cutoff frequency in Hz. Removes sub-bass rumble.
    pub highpass_hz: f32,
}

impl Default for FilterParams {
    fn default() -> Self {
        Self {
            preset:      FilterPreset::QuietRoom,
            suppression: 0.3,
            gate_db:     -55.0,
            highpass_hz: 80.0,
        }
    }
}


// ── App settings ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AppSettings {
    pub api_base_url: String,
    pub theme_color: String,
    pub run_in_background: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            api_base_url: "http://localhost:8000".to_string(),
            theme_color: "#6C63FF".to_string(),
            run_in_background: false,
        }
    }
}
