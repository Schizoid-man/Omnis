/// REST API client mirroring engine/services/api.ts
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::types::{KeyBlob, WireChat, WireMessage, WireSession};

// ── Request / response shapes ────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SignupRequest<'a> {
    pub username: &'a str,
    pub password: &'a str,
    pub identity_pub: &'a str,
    pub encrypted_identity_priv: &'a str,
    pub kdf_salt: &'a str,
    pub aead_nonce: &'a str,
}

#[derive(Serialize)]
pub struct LoginRequest<'a> {
    pub username: &'a str,
    pub password: &'a str,
}

#[derive(Deserialize)]
pub struct LoginResponse {
    pub token: String,
}

#[derive(Deserialize)]
pub struct MeResponse {
    pub id: i64,
    pub username: String,
}

#[derive(Serialize)]
pub struct CreateChatRequest<'a> {
    pub username: &'a str,
}

// Backend returns {chat_id} only
#[derive(Deserialize)]
pub struct CreateChatResponse {
    pub chat_id: i64,
}

#[derive(Deserialize)]
pub struct FetchMessagesResponse {
    pub messages: Vec<WireMessage>,
    pub next_cursor: Option<i64>,
}

#[derive(Deserialize)]
pub struct EpochKeyResponse {
    pub wrapped_key: String,
    pub epoch_index: i64,
    pub epoch_id: i64,
}

#[derive(Serialize)]
pub struct CreateEpochRequest<'a> {
    pub wrapped_key_a: &'a str,
    pub wrapped_key_b: &'a str,
}

// Backend returns {epoch_id, epoch_index}
#[derive(Deserialize)]
pub struct CreateEpochResponse {
    pub epoch_id: i64,
    pub epoch_index: i64,
}

#[derive(Serialize)]
pub struct SendMessageRequest<'a> {
    pub ciphertext: &'a str,
    pub nonce: &'a str,
    pub epoch_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_id: Option<i64>,
    /// When set, links this message to an already-finalised MediaAttachment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<i64>,
    /// Optional ISO-8601 UTC expiry for ephemeral messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

// Backend returns {id, epoch_id, created_at} — NOT a wrapped WireMessage
#[derive(Deserialize)]
pub struct SendMessageResponse {
    pub id: i64,
    pub epoch_id: i64,
    #[serde(deserialize_with = "crate::types::de_dt")]
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// Backend returns {username, identity_pub}
#[derive(Deserialize)]
pub struct PubkeyResponse {
    pub username: String,
    pub identity_pub: String,
}

// ── Media upload / download types ─────────────────────────────────────────────────

#[derive(Serialize)]
pub struct MediaInitRequest {
    pub chat_id:      i64,
    pub total_size:   u64,
    pub chunk_size:   u64,
    pub total_chunks: u64,
    pub file_type:    String,
}

#[derive(Deserialize)]
pub struct MediaInitResponse {
    pub upload_id: String,
}
/// Response from `POST /media/upload/{id}/finalize`
#[derive(Deserialize)]
pub struct MediaFinalizeResponse {
    pub media_id: i64,
}

// ── VoIP call request / response types ────────────────────────────────────────

#[derive(Deserialize)]
pub struct CallInitiateResponse {
    pub call_id: String,
    pub status: String,
    pub caller_username: String,
    pub callee_username: String,
}

// ── API client ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ApiClient {
    pub base_url: String,
    pub token: Option<String>,
    pub device_id: String,
    client: Client,
}

impl ApiClient {
    pub fn new(base_url: impl Into<String>, device_id: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            token: None,
            device_id: device_id.into(),
            client: Client::new(),
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn set_token(&mut self, token: impl Into<String>) {
        self.token = Some(token.into());
    }

    pub fn clear_token(&mut self) {
        self.token = None;
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    fn auth_headers(&self) -> Result<[(&'static str, String); 2]> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| AppError::Auth("Not authenticated".into()))?;
        Ok([
            ("Authorization", format!("Bearer {}", token)),
            ("X-Device-ID", self.device_id.clone()),
        ])
    }

    /// Returns true if the status was 401 (caller should wipe auth).
    async fn check_status(resp: reqwest::Response) -> Result<reqwest::Response> {
        if resp.status() == StatusCode::UNAUTHORIZED {
            return Err(AppError::Auth("401 Unauthorized".into()));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AppError::Other(format!("HTTP {} — {}", status, body)));
        }
        Ok(resp)
    }

    // ── Auth ─────────────────────────────────────────────────────────────────

    pub async fn signup(&self, req: SignupRequest<'_>) -> Result<()> {
        let resp = self
            .client
            .post(self.url("/auth/signup"))
            .json(&req)
            .send()
            .await?;
        Self::check_status(resp).await?;
        Ok(())
    }

    pub async fn login(&self, req: LoginRequest<'_>) -> Result<LoginResponse> {
        let resp = self
            .client
            .post(self.url("/auth/login"))
            .header("X-Device-ID", &self.device_id)
            .json(&req)
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json::<LoginResponse>().await?)
    }

    pub async fn logout(&self) -> Result<()> {
        let [auth, dev] = self.auth_headers()?;
        self.client
            .post(self.url("/auth/logout"))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .send()
            .await?;
        Ok(())
    }

    pub async fn me(&self) -> Result<MeResponse> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .get(self.url("/auth/me"))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json::<MeResponse>().await?)
    }

    pub async fn get_keyblob(&self) -> Result<KeyBlob> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .get(self.url("/auth/keyblob"))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json::<KeyBlob>().await?)
    }

    // ── User keys ────────────────────────────────────────────────────────────

    pub async fn get_user_pubkey(&self, username: &str) -> Result<PubkeyResponse> {
        let resp = self
            .client
            .get(self.url("/user/pkey/get"))
            .query(&[("username", username)])
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json::<PubkeyResponse>().await?)
    }

    // ── Chats ─────────────────────────────────────────────────────────────────

    pub async fn list_chats(&self) -> Result<Vec<WireChat>> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .get(self.url("/chat/list"))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        // Backend returns a bare JSON array, not a wrapper object
        Ok(resp.json::<Vec<WireChat>>().await?)
    }

    pub async fn create_chat(&self, username: &str) -> Result<CreateChatResponse> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .post(self.url("/chat/create"))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .json(&CreateChatRequest { username })
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json::<CreateChatResponse>().await?)
    }

    pub async fn fetch_messages(
        &self,
        chat_id: i64,
        before_id: Option<i64>,
        limit: u32,
    ) -> Result<FetchMessagesResponse> {
        let [auth, dev] = self.auth_headers()?;
        let url = self.url(&format!("/chat/fetch/{}", chat_id));
        let mut req = self.client.get(&url).header(auth.0, auth.1).header(dev.0, dev.1);
        if let Some(bid) = before_id {
            req = req.query(&[("before_id", bid)]);
        }
        req = req.query(&[("limit", limit)]);
        let resp = req.send().await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json::<FetchMessagesResponse>().await?)
    }

    pub async fn fetch_epoch_key(&self, chat_id: i64, epoch_id: i64) -> Result<EpochKeyResponse> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .get(self.url(&format!("/chat/{}/{}/fetch", chat_id, epoch_id)))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json::<EpochKeyResponse>().await?)
    }

    pub async fn create_epoch(
        &self,
        chat_id: i64,
        wrapped_key_a: &str,
        wrapped_key_b: &str,
    ) -> Result<CreateEpochResponse> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .post(self.url(&format!("/chat/{}/epoch", chat_id)))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .json(&CreateEpochRequest { wrapped_key_a, wrapped_key_b })
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json::<CreateEpochResponse>().await?)
    }

    pub async fn send_message(
        &self,
        chat_id: i64,
        req: SendMessageRequest<'_>,
    ) -> Result<SendMessageResponse> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .post(self.url(&format!("/chat/{}/message", chat_id)))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .json(&req)
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json::<SendMessageResponse>().await?)
    }

    // ── Sessions ──────────────────────────────────────────────────────────────

    pub async fn list_sessions(&self) -> Result<Vec<WireSession>> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .get(self.url("/users/sessions"))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        // Backend returns a bare JSON array
        Ok(resp.json::<Vec<WireSession>>().await?)
    }

    pub async fn revoke_session(&self, session_id: i64) -> Result<()> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .delete(self.url(&format!("/users/sessions/revoke/{}", session_id)))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .send()
            .await?;
        Self::check_status(resp).await?;
        Ok(())
    }

    pub async fn revoke_other_sessions(&self) -> Result<()> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .delete(self.url("/users/sessions/revoke_other"))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .send()
            .await?;
        Self::check_status(resp).await?;
        Ok(())
    }

    // ── Media upload / download ───────────────────────────────────────────────────────────

    /// Initialise a chunked upload session. Returns the opaque `upload_id`.
    pub async fn init_media_upload(&self, req: &MediaInitRequest) -> Result<MediaInitResponse> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .post(self.url("/media/upload/init"))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .json(req)
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json::<MediaInitResponse>().await?)
    }

    /// Upload a single raw-bytes chunk. The data must already be encrypted.
    pub async fn upload_chunk(
        &self,
        upload_id: &str,
        chunk_index: usize,
        data: Vec<u8>,
    ) -> Result<()> {
        let [auth, dev] = self.auth_headers()?;
        let url = self.url(&format!("/media/upload/{}/chunk/{}", upload_id, chunk_index));
        let resp = self
            .client
            .put(&url)
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .header("Content-Type", "application/octet-stream")
            .body(data)
            .send()
            .await?;
        Self::check_status(resp).await?;
        Ok(())
    }

    /// Finalise the upload and obtain the `media_id` for use in a message.
    pub async fn finalize_upload(&self, upload_id: &str) -> Result<MediaFinalizeResponse> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .post(self.url(&format!("/media/upload/{}/finalize", upload_id)))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json::<MediaFinalizeResponse>().await?)
    }

    /// Download the encrypted blob. Returns raw ciphertext bytes.
    pub async fn download_media(&self, media_id: i64) -> Result<Vec<u8>> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .get(self.url(&format!("/media/{}", media_id)))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.bytes().await?.to_vec())
    }

    /// Delete a media attachment (uploader only).
    pub async fn delete_media(&self, media_id: i64) -> Result<()> {
        let [auth, dev] = self.auth_headers()?;
        let resp = self
            .client
            .delete(self.url(&format!("/media/{}", media_id)))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .send()
            .await?;
        Self::check_status(resp).await?;
        Ok(())
    }

    // ── Connectivity test ─────────────────────────────────────────────────────

    pub async fn test_connection(&self) -> bool {
        self.client
            .get(self.url("/auth/me"))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .map(|r| r.status() != StatusCode::SERVICE_UNAVAILABLE)
            .unwrap_or(false)
    }
    // ── VoIP call REST ───────────────────────────────────────────────────

    pub async fn initiate_call(&self, callee_username: &str, chat_id: Option<i64>) -> Result<CallInitiateResponse> {
        let [auth, dev] = self.auth_headers()?;
        let body = serde_json::json!({ "callee_username": callee_username, "chat_id": chat_id });
        let resp = self
            .client
            .post(self.url("/call/initiate"))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .json(&body)
            .send()
            .await?;
        let resp = Self::check_status(resp).await?;
        Ok(resp.json::<CallInitiateResponse>().await?)
    }

    pub async fn answer_call(&self, call_id: &str) -> Result<()> {
        let [auth, dev] = self.auth_headers()?;
        let body = serde_json::json!({ "call_id": call_id });
        let resp = self
            .client
            .post(self.url("/call/answer"))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .json(&body)
            .send()
            .await?;
        Self::check_status(resp).await?;
        Ok(())
    }

    pub async fn reject_call(&self, call_id: &str) -> Result<()> {
        let [auth, dev] = self.auth_headers()?;
        let body = serde_json::json!({ "call_id": call_id });
        let resp = self
            .client
            .post(self.url("/call/reject"))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .json(&body)
            .send()
            .await?;
        Self::check_status(resp).await?;
        Ok(())
    }

    pub async fn end_call(&self, call_id: &str) -> Result<()> {
        let [auth, dev] = self.auth_headers()?;
        let body = serde_json::json!({ "call_id": call_id });
        let resp = self
            .client
            .post(self.url("/call/end"))
            .header(auth.0, auth.1)
            .header(dev.0, dev.1)
            .json(&body)
            .send()
            .await?;
        Self::check_status(resp).await?;
        Ok(())
    }

    /// Build a `ws://` or `wss://` URL from the current base URL.
    pub fn ws_url(&self, path: &str) -> String {
        let ws_base = self.base_url
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1);
        format!("{}{}", ws_base.trim_end_matches('/'), path)
    }}
