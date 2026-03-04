/// Cryptographic service for Omnis TUI.
///
/// Must be byte-for-byte compatible with the mobile app's crypto pipeline:
///   - Identity keypair: P-384 ECDH (SPKI public, PKCS8 private, both DER base64)
///   - Private key rest-encryption: PBKDF2-HMAC-SHA256(100k) → AES-256-GCM
///   - Epoch key wrapping: P-384 ECDH → HKDF-SHA256 (salt=32×0, info="epoch-key-wrap") → AES-256-GCM
///   - Message encryption: random AES-256-GCM key per epoch
///   - File encryption: random AES-256-GCM key per file, key embedded in message envelope
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use elliptic_curve::pkcs8::{DecodePrivateKey, DecodePublicKey};
use hkdf::Hkdf;
use hmac::Hmac;
use p384::{
    ecdh::diffie_hellman,
    pkcs8::{EncodePrivateKey, EncodePublicKey},
    PublicKey, SecretKey,
};
use pbkdf2::pbkdf2;
use rand::{rngs::OsRng, RngCore};
use sha2::Sha256;

use crate::error::{AppError, Result};

// ── Constants (mirror engine/constants.ts) ──────────────────────────────────

const PBKDF2_ITERATIONS: u32 = 100_000;
const PBKDF2_SALT_LEN: usize = 32;
const AES_KEY_LEN: usize = 32;
const AES_NONCE_LEN: usize = 12;
const HKDF_INFO: &[u8] = b"epoch-key-wrap";

// ── Low-level helpers ────────────────────────────────────────────────────────

fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    OsRng.fill_bytes(&mut buf);
    buf
}

fn aes_gcm_encrypt(key: &[u8; 32], nonce: &[u8; 12], plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| AppError::Crypto(e.to_string()))
}

fn aes_gcm_decrypt(key: &[u8; 32], nonce: &[u8; 12], ciphertext: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| AppError::Crypto("Decryption failed — wrong key or corrupted data".into()))
}

fn pbkdf2_derive(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut key = [0u8; AES_KEY_LEN];
    pbkdf2::<Hmac<Sha256>>(password.as_bytes(), salt, PBKDF2_ITERATIONS, &mut key)
        .expect("PBKDF2 length is valid");
    key
}

/// Derive the 32-byte AES wrapping key from a P-384 ECDH shared secret via HKDF.
/// Matches TS: sharedX = sharedSecret[1..49], then HKDF(SHA-256, ikm=sharedX, salt=32×0, info="epoch-key-wrap")
fn ecdh_wrapping_key(my_secret: &SecretKey, peer_public: &PublicKey) -> Result<[u8; 32]> {
    let shared = diffie_hellman(my_secret.to_nonzero_scalar(), peer_public.as_affine());
    // raw_secret_bytes() is the affine X coordinate (48 bytes) — matches TS slice(1,49)
    let shared_x = shared.raw_secret_bytes();

    let salt = [0u8; 32];
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared_x.as_slice());
    let mut wrap_key = [0u8; 32];
    hk.expand(HKDF_INFO, &mut wrap_key)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    Ok(wrap_key)
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Generate a new P-384 ECDH identity key pair.
/// Returns (public_key_spki_base64, private_key_pkcs8_base64)
pub fn generate_identity_key_pair() -> Result<(String, String)> {
    let secret = SecretKey::random(&mut OsRng);
    let public = secret.public_key();

    let pkcs8_der = secret
        .to_pkcs8_der()
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let spki_der = public
        .to_public_key_der()
        .map_err(|e| AppError::Crypto(e.to_string()))?;

    Ok((B64.encode(spki_der.as_bytes()), B64.encode(pkcs8_der.as_bytes())))
}

/// Generate a random 32-byte AES-256 epoch key, returned as base64.
pub fn generate_aes_key() -> String {
    B64.encode(random_bytes(AES_KEY_LEN))
}

/// Encrypt the identity private key PKCS8 base64 string with a password.
///
/// Plaintext: UTF-8 bytes of the base64 private key string (matches TS exactly).
/// Returns (encrypted_base64, salt_base64, nonce_base64).
pub fn encrypt_identity_private_key(
    private_key_base64: &str,
    password: &str,
) -> Result<(String, String, String)> {
    let salt = random_bytes(PBKDF2_SALT_LEN);
    let nonce_bytes = random_bytes(AES_NONCE_LEN);

    let key = pbkdf2_derive(password, &salt);
    let nonce: [u8; 12] = nonce_bytes.clone().try_into().unwrap();

    // Encrypt the UTF-8 bytes of the base64 string (matches TS TextEncoder.encode(privateKeyBase64))
    let ciphertext = aes_gcm_encrypt(&key, &nonce, private_key_base64.as_bytes())?;

    Ok((B64.encode(&ciphertext), B64.encode(&salt), B64.encode(&nonce_bytes)))
}

/// Decrypt the identity private key. Returns the base64 PKCS8 string.
pub fn decrypt_identity_private_key(
    encrypted_base64: &str,
    salt_base64: &str,
    nonce_base64: &str,
    password: &str,
) -> Result<String> {
    let ciphertext = B64
        .decode(encrypted_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let salt = B64
        .decode(salt_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let nonce_vec = B64
        .decode(nonce_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;

    if nonce_vec.len() != AES_NONCE_LEN {
        return Err(AppError::Crypto("Invalid nonce length".into()));
    }

    let key = pbkdf2_derive(password, &salt);
    let nonce: [u8; 12] = nonce_vec.try_into().unwrap();
    let plaintext_bytes = aes_gcm_decrypt(&key, &nonce, &ciphertext)?;

    String::from_utf8(plaintext_bytes).map_err(|e| AppError::Crypto(e.to_string()))
}

/// Encrypt a message with an epoch key.
/// Returns (ciphertext_base64, nonce_base64).
pub fn aes_gcm_encrypt_message(plaintext: &str, epoch_key_base64: &str) -> Result<(String, String)> {
    let epoch_key_bytes = B64
        .decode(epoch_key_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    if epoch_key_bytes.len() != AES_KEY_LEN {
        return Err(AppError::Crypto("Invalid epoch key length".into()));
    }
    let key: [u8; 32] = epoch_key_bytes.try_into().unwrap();
    let nonce_vec = random_bytes(AES_NONCE_LEN);
    let nonce: [u8; 12] = nonce_vec.clone().try_into().unwrap();

    let ciphertext = aes_gcm_encrypt(&key, &nonce, plaintext.as_bytes())?;
    Ok((B64.encode(&ciphertext), B64.encode(&nonce_vec)))
}

/// Decrypt a message with an epoch key. Returns the plaintext string.
pub fn aes_gcm_decrypt_message(
    ciphertext_base64: &str,
    nonce_base64: &str,
    epoch_key_base64: &str,
) -> Result<String> {
    let ciphertext = B64
        .decode(ciphertext_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let nonce_vec = B64
        .decode(nonce_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let epoch_key_bytes = B64
        .decode(epoch_key_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;

    if nonce_vec.len() != AES_NONCE_LEN || epoch_key_bytes.len() != AES_KEY_LEN {
        return Err(AppError::Crypto("Invalid key or nonce length".into()));
    }

    let key: [u8; 32] = epoch_key_bytes.try_into().unwrap();
    let nonce: [u8; 12] = nonce_vec.try_into().unwrap();
    let plaintext_bytes = aes_gcm_decrypt(&key, &nonce, &ciphertext)?;

    String::from_utf8(plaintext_bytes).map_err(|e| AppError::Crypto(e.to_string()))
}

/// Wrap an epoch key for a recipient.
/// Output format (matches TS): base64(nonce[12] || aes_gcm_ciphertext_with_tag)
pub fn wrap_epoch_key(
    epoch_key_base64: &str,
    my_private_key_pkcs8_base64: &str,
    peer_public_key_spki_base64: &str,
) -> Result<String> {
    let my_pkcs8 = B64
        .decode(my_private_key_pkcs8_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let peer_spki = B64
        .decode(peer_public_key_spki_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let epoch_key = B64
        .decode(epoch_key_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;

    let my_secret = SecretKey::from_pkcs8_der(&my_pkcs8)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let peer_public = PublicKey::from_public_key_der(&peer_spki)
        .map_err(|e| AppError::Crypto(e.to_string()))?;

    let wrap_key = ecdh_wrapping_key(&my_secret, &peer_public)?;
    let nonce_vec = random_bytes(AES_NONCE_LEN);
    let nonce: [u8; 12] = nonce_vec.clone().try_into().unwrap();

    let wrapped = aes_gcm_encrypt(&wrap_key, &nonce, &epoch_key)?;

    // Concatenate nonce || wrapped (matches TS: result.set(nonce, 0); result.set(wrapped, nonce.length))
    let mut result = nonce_vec;
    result.extend(wrapped);
    Ok(B64.encode(&result))
}

/// Unwrap an epoch key received from sender.
pub fn unwrap_epoch_key(
    wrapped_key_base64: &str,
    my_private_key_pkcs8_base64: &str,
    sender_public_key_spki_base64: &str,
) -> Result<String> {
    let wrapped_data = B64
        .decode(wrapped_key_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    if wrapped_data.len() <= AES_NONCE_LEN {
        return Err(AppError::Crypto("Wrapped key too short".into()));
    }
    let nonce: [u8; 12] = wrapped_data[..AES_NONCE_LEN].try_into().unwrap();
    let wrapped = &wrapped_data[AES_NONCE_LEN..];

    let my_pkcs8 = B64
        .decode(my_private_key_pkcs8_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let sender_spki = B64
        .decode(sender_public_key_spki_base64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;

    let my_secret = SecretKey::from_pkcs8_der(&my_pkcs8)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let sender_public = PublicKey::from_public_key_der(&sender_spki)
        .map_err(|e| AppError::Crypto(e.to_string()))?;

    let wrap_key = ecdh_wrapping_key(&my_secret, &sender_public)?;
    let epoch_key = aes_gcm_decrypt(&wrap_key, &nonce, wrapped)?;
    Ok(B64.encode(&epoch_key))
}
/// Derive the 32-byte AES-256-GCM key used to encrypt VoIP audio frames.
///
/// Reuses the ECDH+HKDF infrastructure of the message encryption pipeline
/// but with a separate HKDF `info` string so call-key material is
/// domain-separated from epoch-key-wrap material.
///
/// Both sides of the call compute `ECDH(my_priv, peer_pub)` which yields the
/// same shared X coordinate, so the derived key is identical without any
/// additional key exchange.
pub fn derive_call_key(
    my_private_key_pkcs8_b64: &str,
    peer_public_key_spki_b64: &str,
) -> Result<[u8; 32]> {
    const CALL_KEY_INFO: &[u8] = b"call-key";

    let my_pkcs8 = B64
        .decode(my_private_key_pkcs8_b64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let peer_spki = B64
        .decode(peer_public_key_spki_b64)
        .map_err(|e| AppError::Crypto(e.to_string()))?;

    let my_secret = SecretKey::from_pkcs8_der(&my_pkcs8)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    let peer_public = PublicKey::from_public_key_der(&peer_spki)
        .map_err(|e| AppError::Crypto(e.to_string()))?;

    let shared = diffie_hellman(my_secret.to_nonzero_scalar(), peer_public.as_affine());
    let shared_x = shared.raw_secret_bytes();

    let salt = [0u8; 32];
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared_x.as_slice());
    let mut call_key = [0u8; 32];
    hk.expand(CALL_KEY_INFO, &mut call_key)
        .map_err(|e| AppError::Crypto(e.to_string()))?;
    Ok(call_key)
}

/// Encrypt a raw byte slice with AES-256-GCM.  Returns `(nonce_12, ciphertext_with_tag)`.
/// Used for per-audio-frame encryption in VoIP.
pub fn aes_gcm_encrypt_raw(key: &[u8; 32], plaintext: &[u8]) -> Result<([u8; 12], Vec<u8>)> {
    let nonce_vec = random_bytes(AES_NONCE_LEN);
    let nonce: [u8; 12] = nonce_vec.try_into().unwrap();
    let ct = aes_gcm_encrypt(key, &nonce, plaintext)?;
    Ok((nonce, ct))
}

/// Decrypt a raw byte slice encrypted with [`aes_gcm_encrypt_raw`].
pub fn aes_gcm_decrypt_raw(key: &[u8; 32], nonce: &[u8; 12], ciphertext: &[u8]) -> Result<Vec<u8>> {
    aes_gcm_decrypt(key, nonce, ciphertext)
}
// ── File / media encryption ─────────────────────────────────────────────────

/// Generate a random 32-byte per-file AES-256-GCM key.
///
/// Each file uploaded gets its own independent key. That key is then
/// embedded inside the epoch-encrypted message ciphertext so it never
/// leaves the client in plaintext.
pub fn generate_file_key() -> [u8; AES_KEY_LEN] {
    let mut key = [0u8; AES_KEY_LEN];
    OsRng.fill_bytes(&mut key);
    key
}

/// Encrypt raw bytes with a per-file key. Returns `(ciphertext, nonce_12_bytes)`.
pub fn encrypt_file(data: &[u8], key: &[u8; AES_KEY_LEN]) -> Result<(Vec<u8>, [u8; AES_NONCE_LEN])> {
    let nonce_vec = random_bytes(AES_NONCE_LEN);
    let nonce: [u8; AES_NONCE_LEN] = nonce_vec.try_into().unwrap();
    let ciphertext = aes_gcm_encrypt(key, &nonce, data)?;
    Ok((ciphertext, nonce))
}

/// Decrypt a file blob that was encrypted with [`encrypt_file`].
pub fn decrypt_file(
    ciphertext: &[u8],
    nonce: &[u8; AES_NONCE_LEN],
    key: &[u8; AES_KEY_LEN],
) -> Result<Vec<u8>> {
    aes_gcm_decrypt(key, nonce, ciphertext)
}

// ── Message plaintext envelope ────────────────────────────────────────────────

/// Build the JSON plaintext envelope for a media message.
///
/// This string is what gets AES-256-GCM encrypted to produce `ciphertext`.
/// The per-file `file_key` and `file_nonce` are embedded here so only the
/// chat participants (who can decrypt the message) can decrypt the blob.
pub fn build_media_plaintext(
    caption: &str,
    media_id: i64,
    file_key: &[u8; AES_KEY_LEN],
    file_nonce: &[u8; AES_NONCE_LEN],
    file_type: &str,
    filename: &str,
) -> String {
    serde_json::json!({
        "type": "media",
        "text": caption,
        "media_id": media_id,
        "file_key": B64.encode(file_key),
        "file_nonce": B64.encode(file_nonce),
        "file_type": file_type,
        "filename": filename,
    })
    .to_string()
}

/// Variants of a decoded message plaintext.
///
/// Old-style plain-text messages (no JSON) are decoded as `Text`.
/// New media messages carry an envelope with the per-file encryption key.
pub enum ParsedMessage {
    /// Regular text message (backwards-compatible with pre-media messages).
    Text(String),
    /// Media message with caption and embedded file-encryption key material.
    Media {
        caption:    String,
        media_id:   i64,
        file_key:   [u8; AES_KEY_LEN],
        file_nonce: [u8; AES_NONCE_LEN],
        file_type:  String,
        filename:   String,
    },
}

/// Parse a decrypted message plaintext string into its typed variant.
///
/// JSON envelopes with `type == "media"` are parsed as `ParsedMessage::Media`.
/// Everything else (plain strings, `type == "text"` objects, invalid JSON)
/// is treated as `ParsedMessage::Text` for backward compatibility.
pub fn parse_plaintext(plaintext: &str) -> ParsedMessage {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(plaintext) {
        match v.get("type").and_then(|t| t.as_str()) {
            Some("media") => {
                let caption   = v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let media_id  = v.get("media_id").and_then(|m| m.as_i64()).unwrap_or(0);
                let file_type = v.get("file_type").and_then(|t| t.as_str()).unwrap_or("file").to_string();
                let filename  = v.get("filename").and_then(|f| f.as_str()).unwrap_or("attachment").to_string();

                let key_b64   = v.get("file_key").and_then(|k| k.as_str()).unwrap_or("");
                let nonce_b64 = v.get("file_nonce").and_then(|n| n.as_str()).unwrap_or("");

                let key_bytes   = B64.decode(key_b64).unwrap_or_default();
                let nonce_bytes = B64.decode(nonce_b64).unwrap_or_default();

                if key_bytes.len() == AES_KEY_LEN && nonce_bytes.len() == AES_NONCE_LEN {
                    let mut file_key   = [0u8; AES_KEY_LEN];
                    let mut file_nonce = [0u8; AES_NONCE_LEN];
                    file_key.copy_from_slice(&key_bytes);
                    file_nonce.copy_from_slice(&nonce_bytes);
                    return ParsedMessage::Media {
                        caption, media_id, file_key, file_nonce, file_type, filename,
                    };
                }
            }
            Some("text") => {
                return ParsedMessage::Text(
                    v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                );
            }
            _ => {}
        }
    }
    // Backward compat: raw string (no JSON envelope)
    ParsedMessage::Text(plaintext.to_string())
}
