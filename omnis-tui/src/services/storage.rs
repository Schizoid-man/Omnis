/// Persistent secure storage using the OS keychain / credential manager.
///
/// Windows → Credential Manager
/// Linux   → Secret Service (libsecret) or fallback to XDG config file
/// macOS   → Keychain
use keyring::Entry;

use crate::error::{AppError, Result};

const SERVICE: &str = "omnis-tui";

fn entry(key: &str) -> std::result::Result<Entry, keyring::Error> {
    Entry::new(SERVICE, key)
}

fn set(key: &str, value: &str) -> Result<()> {
    entry(key)
        .and_then(|e| e.set_password(value))
        .map_err(|e| AppError::Storage(e.to_string()))
}

fn get(key: &str) -> Result<Option<String>> {
    match entry(key).and_then(|e| e.get_password()) {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Storage(e.to_string())),
    }
}

fn delete(key: &str) -> Result<()> {
    match entry(key).and_then(|e| e.delete_credential()) {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Storage(e.to_string())),
    }
}

// ── Named accessors ──────────────────────────────────────────────────────────

pub fn save_auth_token(token: &str) -> Result<()> { set("auth_token", token) }
pub fn load_auth_token() -> Result<Option<String>> { get("auth_token") }

pub fn save_device_id(id: &str) -> Result<()> { set("device_id", id) }
pub fn load_device_id() -> Result<Option<String>> { get("device_id") }

pub fn save_user_id(id: i64) -> Result<()> { set("user_id", &id.to_string()) }
pub fn load_user_id() -> Result<Option<i64>> {
    get("user_id").map(|o| o.and_then(|s| s.parse().ok()))
}

pub fn save_username(name: &str) -> Result<()> { set("username", name) }
pub fn load_username() -> Result<Option<String>> { get("username") }

pub fn save_api_base_url(url: &str) -> Result<()> { set("api_base_url", url) }
pub fn load_api_base_url() -> Result<Option<String>> { get("api_base_url") }

pub fn save_theme_color(color: &str) -> Result<()> { set("theme_color", color) }
pub fn load_theme_color() -> Result<Option<String>> { get("theme_color") }

/// Store the encrypted private key blob so the user doesn't have to re-enter
/// their password on every launch (the blob itself requires the password to use).
pub fn save_identity_pub(pub_key: &str) -> Result<()> { set("identity_pub", pub_key) }
pub fn load_identity_pub() -> Result<Option<String>> { get("identity_pub") }

pub fn save_identity_priv_encrypted(blob: &str) -> Result<()> {
    set("identity_priv_enc", blob)
}
pub fn load_identity_priv_encrypted() -> Result<Option<String>> {
    get("identity_priv_enc")
}
pub fn save_kdf_salt(salt: &str) -> Result<()> { set("kdf_salt", salt) }
pub fn load_kdf_salt() -> Result<Option<String>> { get("kdf_salt") }
pub fn save_aead_nonce(nonce: &str) -> Result<()> { set("aead_nonce", nonce) }
pub fn load_aead_nonce() -> Result<Option<String>> { get("aead_nonce") }

/// Store the login password in the OS keychain so the private key can be
/// auto-decrypted on next launch without prompting the user.
pub fn save_password(pw: &str) -> Result<()> { set("password", pw) }
pub fn load_password() -> Result<Option<String>> { get("password") }

/// Whether the app should hide to the system tray instead of quitting.
pub fn save_run_in_background(val: bool) -> Result<()> {
    set("run_in_background", if val { "1" } else { "0" })
}
pub fn load_run_in_background() -> bool {
    get("run_in_background").ok().flatten().map_or(false, |v| v == "1")
}

/// Returns the directory used to cache decrypted media downloads.
///
/// Creates the directory on first call. Falls back to the current working
/// directory if the OS cache directory cannot be determined.
pub fn media_cache_dir() -> std::path::PathBuf {
    let dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("omnis")
        .join("media");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Wipe all stored credentials (called on 401 / logout).
pub fn clear_all() -> Result<()> {
    for key in &[
        "auth_token",
        "device_id",
        "user_id",
        "username",
        "identity_pub",
        "identity_priv_enc",
        "kdf_salt",
        "aead_nonce",
        "password",
    ] {
        delete(key)?;
    }
    Ok(())
}
