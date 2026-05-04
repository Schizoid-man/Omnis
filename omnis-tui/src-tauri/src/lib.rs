use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use reqwest::{multipart, Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tauri::{Manager, State};
use uuid::Uuid;

const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:6767";

#[derive(Serialize, Deserialize, Default)]
struct PersistedConfig {
  backend_url: Option<String>,
  device_id: Option<String>,
}

struct OmnisState {
  backend_url: RwLock<String>,
  auth_token: RwLock<Option<String>>,
  device_id: RwLock<String>,
  client: Client,
  config_path: RwLock<Option<PathBuf>>,
}

impl OmnisState {
  fn new() -> Result<Self, String> {
    let client = Client::builder()
      .danger_accept_invalid_certs(true)
      .user_agent("omnis-desktop/0.1.0")
      .build()
      .map_err(|e| format!("failed to build HTTP client: {e}"))?;

    Ok(Self {
      backend_url: RwLock::new(DEFAULT_BACKEND_URL.to_string()),
      auth_token: RwLock::new(None),
      device_id: RwLock::new(Uuid::new_v4().to_string().to_lowercase()),
      client,
      config_path: RwLock::new(None),
    })
  }

  fn backend_url(&self) -> String {
    self
      .backend_url
      .read()
      .map(|g| g.clone())
      .unwrap_or_else(|_| DEFAULT_BACKEND_URL.to_string())
  }

  fn device_id(&self) -> String {
    self
      .device_id
      .read()
      .map(|g| g.clone())
      .unwrap_or_else(|_| "unknown".to_string())
  }

  fn set_backend_url(&self, url: String) -> Result<(), String> {
    {
      let mut guard = self
        .backend_url
        .write()
        .map_err(|_| "failed to lock backend URL state".to_string())?;
      *guard = url;
    }
    self.save_config();
    Ok(())
  }

  fn init_config(&self, config_dir: &Path) {
    let config_path = config_dir.join("config.json");

    if let Ok(contents) = std::fs::read_to_string(&config_path) {
      if let Ok(persisted) = serde_json::from_str::<PersistedConfig>(&contents) {
        if let Some(url) = persisted.backend_url.filter(|u| !u.is_empty()) {
          if let Ok(mut guard) = self.backend_url.write() {
            *guard = url;
          }
        }
        if let Some(did) = persisted.device_id.filter(|d| !d.is_empty()) {
          if let Ok(mut guard) = self.device_id.write() {
            *guard = did;
          }
        }
      }
    } else {
      self.save_config_to(&config_path);
    }

    if let Ok(mut guard) = self.config_path.write() {
      *guard = Some(config_path);
    }
  }

  fn save_config(&self) {
    let path = self
      .config_path
      .read()
      .ok()
      .and_then(|g| g.clone());
    if let Some(path) = path {
      self.save_config_to(&path);
    }
  }

  fn save_config_to(&self, path: &Path) {
    let config = PersistedConfig {
      backend_url: Some(self.backend_url()),
      device_id: Some(self.device_id()),
    };
    if let Ok(json) = serde_json::to_string_pretty(&config) {
      if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
      }
      let _ = std::fs::write(path, json);
    }
  }

  fn reset_config(&self) {
    if let Ok(mut guard) = self.backend_url.write() {
      *guard = DEFAULT_BACKEND_URL.to_string();
    }
    self.save_config();
  }

  fn token(&self) -> Result<Option<String>, String> {
    self
      .auth_token
      .read()
      .map(|g| g.clone())
      .map_err(|_| "failed to lock auth token state".to_string())
  }

  fn set_token(&self, token: Option<String>) -> Result<(), String> {
    let mut guard = self
      .auth_token
      .write()
      .map_err(|_| "failed to lock auth token state".to_string())?;
    *guard = token;
    Ok(())
  }
}

fn normalize_backend_url(url: &str) -> String {
  url.trim().trim_end_matches('/').to_string()
}

fn build_url(base: &str, path: &str) -> String {
  format!("{}{}", base.trim_end_matches('/'), path)
}

fn validate_backend_url(url: &str) -> Result<(), String> {
  let parsed = Url::parse(url).map_err(|e| format!("invalid backend URL: {e}"))?;
  if parsed.scheme() != "https" && parsed.scheme() != "http" {
    return Err("backend URL must use http:// or https://".to_string());
  }
  if parsed.host_str().is_none() {
    return Err("backend URL must include a host".to_string());
  }

  Ok(())
}

fn with_auth_headers(
  state: &OmnisState,
  request: reqwest::RequestBuilder,
) -> Result<reqwest::RequestBuilder, String> {
  let token = state
    .token()?
    .ok_or_else(|| "not authenticated".to_string())?;
  Ok(
    request
      .header("Authorization", format!("Bearer {token}"))
      .header("X-Device-ID", state.device_id()),
  )
}

fn detail_to_string(value: Value) -> String {
  match value {
    Value::String(s) => s,
    other => other.to_string(),
  }
}

async fn ensure_success(response: reqwest::Response) -> Result<reqwest::Response, String> {
  if response.status().is_success() {
    return Ok(response);
  }
  Err(http_error(response).await)
}

async fn http_error(response: reqwest::Response) -> String {
  #[derive(Deserialize)]
  struct ApiErrorBody {
    detail: Option<Value>,
    message: Option<String>,
  }

  #[derive(Serialize)]
  #[serde(rename_all = "camelCase")]
  struct ApiError {
    status: u16,
    message: String,
  }

  let status = response.status();
  let body = response
    .text()
    .await
    .unwrap_or_else(|_| "<no response body>".to_string());
  let message = serde_json::from_str::<ApiErrorBody>(&body)
    .ok()
    .and_then(|parsed| parsed.detail.map(detail_to_string).or(parsed.message))
    .unwrap_or(body);
  let payload = ApiError {
    status: status.as_u16(),
    message,
  };
  serde_json::to_string(&payload).unwrap_or_else(|_| format!("HTTP {}", status.as_u16()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackendConfig {
  backend_url: String,
  device_id: String,
  has_token: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthRuntime {
  backend_url: String,
  device_id: String,
  token: Option<String>,
}

#[derive(Serialize)]
struct HealthResponse {
  ping: String,
  version: String,
}

#[derive(Deserialize)]
struct PingPayload {
  ping: Option<String>,
  #[serde(rename = "PING")]
  ping_upper: Option<String>,
}

#[derive(Deserialize)]
struct VersionPayload {
  version: String,
}

#[derive(Deserialize)]
struct LoginPayload {
  token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthSession {
  token: String,
  device_id: String,
  user_id: i64,
  username: String,
}

#[derive(Serialize, Deserialize)]
struct MePayload {
  id: i64,
  username: String,
}

#[derive(Serialize, Deserialize)]
struct KeyBlob {
  identity_pub: String,
  encrypted_identity_priv: String,
  kdf_salt: String,
  aead_nonce: String,
}

#[derive(Serialize, Deserialize)]
struct SearchUser {
  id: i64,
  username: String,
}

#[derive(Serialize, Deserialize)]
struct ChatSummary {
  chat_id: i64,
  with_user: String,
}

#[derive(Serialize)]
struct CreateChatRequest<'a> {
  username: &'a str,
}

#[derive(Serialize, Deserialize)]
struct CreateChatResponse {
  chat_id: i64,
}

#[derive(Serialize, Deserialize)]
struct MediaChunk {
  media_id: i64,
  chunk_index: i64,
  file_size: i64,
}

#[derive(Serialize, Deserialize)]
struct MessageAttachment {
  upload_id: String,
  mime_type: String,
  nonce: String,
  total_chunks: i64,
  total_size: i64,
  chunks: Vec<MediaChunk>,
}

#[derive(Serialize, Deserialize)]
struct WireMessage {
  id: i64,
  sender_id: i64,
  epoch_id: i64,
  reply_id: Option<i64>,
  ciphertext: String,
  nonce: String,
  #[serde(default)]
  deleted: bool,
  created_at: String,
  #[serde(default)]
  attachments: Vec<MessageAttachment>,
}

#[derive(Serialize, Deserialize)]
struct ChatFetchResponse {
  messages: Vec<WireMessage>,
  next_cursor: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct EpochFetchResponse {
  epoch_id: i64,
  epoch_index: i64,
  wrapped_key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateEpochInput {
  wrapped_key_a: String,
  wrapped_key_b: String,
}

#[derive(Serialize)]
struct CreateEpochPayload<'a> {
  wrapped_key_a: &'a str,
  wrapped_key_b: &'a str,
}

#[derive(Serialize, Deserialize)]
struct CreateEpochResponse {
  epoch_id: i64,
  epoch_index: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendMessageInput {
  epoch_id: i64,
  ciphertext: String,
  nonce: String,
  reply_id: Option<i64>,
  media_ids: Option<Vec<i64>>,
}

#[derive(Serialize)]
struct SendMessagePayload<'a> {
  epoch_id: i64,
  ciphertext: &'a str,
  nonce: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  reply_id: Option<i64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  media_ids: Option<&'a [i64]>,
}

#[derive(Serialize, Deserialize)]
struct SendMessageResponse {
  id: i64,
  epoch_id: i64,
  created_at: String,
  #[serde(default)]
  attachments: Vec<MessageAttachment>,
}

#[derive(Serialize, Deserialize)]
struct DeleteMessageResponse {
  status: String,
  message_id: i64,
}

#[derive(Serialize, Deserialize)]
struct SessionPayload {
  id: i64,
  device_id: String,
  user_agent: Option<String>,
  last_accessed: String,
  created_at: String,
  expires_at: String,
  current: bool,
}

#[derive(Serialize, Deserialize)]
struct StatusResponse {
  status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaUploadInput {
  chat_id: i64,
  mime_type: String,
  nonce: String,
  chunk_index: i64,
  total_chunks: i64,
  upload_id: String,
  file_name: Option<String>,
  bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct MediaUploadResponse {
  media_id: i64,
  upload_id: String,
  chunk_index: i64,
  chunks_uploaded: i64,
  total_chunks: i64,
  complete: bool,
}

#[derive(Serialize, Deserialize)]
struct MediaMetaResponse {
  upload_id: String,
  mime_type: String,
  total_chunks: i64,
  nonce: String,
  chunks: Vec<MediaChunk>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MediaDownloadResponse {
  media_id: i64,
  byte_len: usize,
  bytes_b64: String,
}

#[tauri::command]
fn get_backend_config(state: State<'_, OmnisState>) -> Result<BackendConfig, String> {
  let has_token = state.token()?.is_some();
  Ok(BackendConfig {
    backend_url: state.backend_url(),
    device_id: state.device_id(),
    has_token,
  })
}

#[tauri::command]
fn set_backend_url(state: State<'_, OmnisState>, url: String) -> Result<BackendConfig, String> {
  let normalized = normalize_backend_url(&url);
  validate_backend_url(&normalized)?;
  state.set_backend_url(normalized)?;
  get_backend_config(state)
}

#[tauri::command]
fn auth_runtime(state: State<'_, OmnisState>) -> Result<AuthRuntime, String> {
  Ok(AuthRuntime {
    backend_url: state.backend_url(),
    device_id: state.device_id(),
    token: state.token()?,
  })
}

#[tauri::command]
async fn backend_health(state: State<'_, OmnisState>) -> Result<HealthResponse, String> {
  let base = state.backend_url();

  let ping_resp = state
    .client
    .get(build_url(&base, "/"))
    .send()
    .await
    .map_err(|e| format!("backend ping failed: {e}"))?;
  let ping_resp = ensure_success(ping_resp).await?;
  let ping_payload: PingPayload = ping_resp
    .json()
    .await
    .map_err(|e| format!("invalid ping payload: {e}"))?;
  let ping = ping_payload
    .ping
    .or(ping_payload.ping_upper)
    .unwrap_or_else(|| "unknown".to_string());

  let version_resp = state
    .client
    .get(build_url(&base, "/version"))
    .send()
    .await
    .map_err(|e| format!("backend version check failed: {e}"))?;
  let version_resp = ensure_success(version_resp).await?;
  let version_payload: VersionPayload = version_resp
    .json()
    .await
    .map_err(|e| format!("invalid version payload: {e}"))?;

  Ok(HealthResponse {
    ping,
    version: version_payload.version,
  })
}

#[tauri::command]
async fn auth_login(
  state: State<'_, OmnisState>,
  username: String,
  password: String,
) -> Result<AuthSession, String> {
  #[derive(Serialize)]
  struct LoginRequest<'a> {
    username: &'a str,
    password: &'a str,
  }

  let base = state.backend_url();
  let login_resp = state
    .client
    .post(build_url(&base, "/auth/login"))
    .header("X-Device-ID", state.device_id())
    .json(&LoginRequest {
      username: &username,
      password: &password,
    })
    .send()
    .await
    .map_err(|e| format!("login request failed: {e}"))?;

  let login_resp = ensure_success(login_resp).await?;
  let login: LoginPayload = login_resp
    .json()
    .await
    .map_err(|e| format!("invalid login response: {e}"))?;
  state.set_token(Some(login.token.clone()))?;

  let me_resp = state
    .client
    .get(build_url(&base, "/auth/me"))
    .header("Authorization", format!("Bearer {}", login.token))
    .header("X-Device-ID", state.device_id())
    .send()
    .await
    .map_err(|e| format!("me request failed: {e}"))?;

  let me_resp = match ensure_success(me_resp).await {
    Ok(response) => response,
    Err(error) => {
      state.set_token(None)?;
      return Err(error);
    }
  };

  let me: MePayload = me_resp
    .json()
    .await
    .map_err(|e| format!("invalid me response: {e}"))?;

  Ok(AuthSession {
    token: state.token()?.unwrap_or_default(),
    device_id: state.device_id(),
    user_id: me.id,
    username: me.username,
  })
}

#[tauri::command]
async fn auth_me(state: State<'_, OmnisState>) -> Result<MePayload, String> {
  let base = state.backend_url();
  let request = state.client.get(build_url(&base, "/auth/me"));
  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("me request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<MePayload>()
    .await
    .map_err(|e| format!("invalid me payload: {e}"))
}

#[tauri::command]
async fn auth_keyblob(state: State<'_, OmnisState>) -> Result<KeyBlob, String> {
  let base = state.backend_url();
  let request = state.client.get(build_url(&base, "/auth/keyblob"));
  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("keyblob request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<KeyBlob>()
    .await
    .map_err(|e| format!("invalid keyblob payload: {e}"))
}

#[tauri::command]
async fn auth_logout(state: State<'_, OmnisState>) -> Result<(), String> {
  let token = match state.token()? {
    Some(token) => token,
    None => return Ok(()),
  };

  let base = state.backend_url();
  let response = state
    .client
    .post(build_url(&base, "/auth/logout"))
    .header("Authorization", format!("Bearer {token}"))
    .header("X-Device-ID", state.device_id())
    .send()
    .await
    .map_err(|e| format!("logout request failed: {e}"))?;

  let _ = ensure_success(response).await?;
  state.set_token(None)?;
  Ok(())
}

#[tauri::command]
async fn users_search(state: State<'_, OmnisState>, q: String) -> Result<Vec<SearchUser>, String> {
  let query = q.trim();
  if query.len() < 3 {
    return Err("search query must be at least 3 characters".to_string());
  }

  let base = state.backend_url();
  let request = state
    .client
    .get(build_url(&base, "/users/search"))
    .query(&[("q", query)]);

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("users search request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<Vec<SearchUser>>()
    .await
    .map_err(|e| format!("invalid users search payload: {e}"))
}

#[tauri::command]
async fn chat_list(state: State<'_, OmnisState>) -> Result<Vec<ChatSummary>, String> {
  let base = state.backend_url();
  let request = state.client.get(build_url(&base, "/chat/list"));
  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("chat list request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<Vec<ChatSummary>>()
    .await
    .map_err(|e| format!("invalid chat list payload: {e}"))
}

#[tauri::command]
async fn chat_create(state: State<'_, OmnisState>, username: String) -> Result<CreateChatResponse, String> {
  let trimmed_username = username.trim();
  if trimmed_username.is_empty() {
    return Err("username is required".to_string());
  }

  let base = state.backend_url();
  let request = state
    .client
    .post(build_url(&base, "/chat/create"))
    .json(&CreateChatRequest {
      username: trimmed_username,
    });

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("chat create request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<CreateChatResponse>()
    .await
    .map_err(|e| format!("invalid chat create payload: {e}"))
}

#[tauri::command]
async fn chat_fetch(
  state: State<'_, OmnisState>,
  chat_id: i64,
  before_id: Option<i64>,
  limit: Option<u32>,
) -> Result<ChatFetchResponse, String> {
  let clamped_limit = limit.unwrap_or(50).clamp(1, 100);
  let base = state.backend_url();
  let path = format!("/chat/fetch/{chat_id}");
  let mut request = state
    .client
    .get(build_url(&base, &path))
    .query(&[("limit", clamped_limit)]);
  if let Some(cursor) = before_id {
    request = request.query(&[("before_id", cursor)]);
  }

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("chat fetch request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<ChatFetchResponse>()
    .await
    .map_err(|e| format!("invalid chat fetch payload: {e}"))
}

#[tauri::command]
async fn chat_fetch_epoch(
  state: State<'_, OmnisState>,
  chat_id: i64,
  epoch_id: i64,
) -> Result<EpochFetchResponse, String> {
  let base = state.backend_url();
  let path = format!("/chat/{chat_id}/{epoch_id}/fetch");
  let request = state.client.get(build_url(&base, &path));

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("fetch epoch request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<EpochFetchResponse>()
    .await
    .map_err(|e| format!("invalid fetch epoch payload: {e}"))
}

#[tauri::command]
async fn chat_create_epoch(
  state: State<'_, OmnisState>,
  chat_id: i64,
  input: CreateEpochInput,
) -> Result<CreateEpochResponse, String> {
  if input.wrapped_key_a.trim().is_empty() || input.wrapped_key_b.trim().is_empty() {
    return Err("wrapped keys are required".to_string());
  }

  let base = state.backend_url();
  let path = format!("/chat/{chat_id}/epoch");
  let request = state
    .client
    .post(build_url(&base, &path))
    .json(&CreateEpochPayload {
      wrapped_key_a: input.wrapped_key_a.trim(),
      wrapped_key_b: input.wrapped_key_b.trim(),
    });

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("create epoch request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<CreateEpochResponse>()
    .await
    .map_err(|e| format!("invalid create epoch payload: {e}"))
}

#[tauri::command]
async fn chat_send_message(
  state: State<'_, OmnisState>,
  chat_id: i64,
  input: SendMessageInput,
) -> Result<SendMessageResponse, String> {
  if input.ciphertext.trim().is_empty() {
    return Err("ciphertext is required".to_string());
  }
  if input.nonce.trim().is_empty() {
    return Err("nonce is required".to_string());
  }

  let base = state.backend_url();
  let path = format!("/chat/{chat_id}/message");
  let request = state
    .client
    .post(build_url(&base, &path))
    .json(&SendMessagePayload {
      epoch_id: input.epoch_id,
      ciphertext: input.ciphertext.trim(),
      nonce: input.nonce.trim(),
      reply_id: input.reply_id,
      media_ids: input.media_ids.as_deref(),
    });

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("send message request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<SendMessageResponse>()
    .await
    .map_err(|e| format!("invalid send message payload: {e}"))
}

#[tauri::command]
async fn chat_delete_message(
  state: State<'_, OmnisState>,
  chat_id: i64,
  message_id: i64,
) -> Result<DeleteMessageResponse, String> {
  let base = state.backend_url();
  let path = format!("/chat/{chat_id}/message/{message_id}");
  let request = state.client.delete(build_url(&base, &path));

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("delete message request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<DeleteMessageResponse>()
    .await
    .map_err(|e| format!("invalid delete message payload: {e}"))
}

#[tauri::command]
async fn sessions_list(state: State<'_, OmnisState>) -> Result<Vec<SessionPayload>, String> {
  let base = state.backend_url();
  let request = state.client.get(build_url(&base, "/users/sessions"));

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("list sessions request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<Vec<SessionPayload>>()
    .await
    .map_err(|e| format!("invalid list sessions payload: {e}"))
}

#[tauri::command]
async fn sessions_revoke(
  state: State<'_, OmnisState>,
  session_id: i64,
) -> Result<StatusResponse, String> {
  let base = state.backend_url();
  let path = format!("/users/sessions/revoke/{session_id}");
  let request = state.client.delete(build_url(&base, &path));

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("revoke session request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<StatusResponse>()
    .await
    .map_err(|e| format!("invalid revoke session payload: {e}"))
}

#[tauri::command]
async fn sessions_revoke_other(state: State<'_, OmnisState>) -> Result<StatusResponse, String> {
  let base = state.backend_url();
  let request = state
    .client
    .delete(build_url(&base, "/users/sessions/revoke_other"));

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("revoke other sessions request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<StatusResponse>()
    .await
    .map_err(|e| format!("invalid revoke other sessions payload: {e}"))
}

#[tauri::command]
async fn media_upload_chunk(
  state: State<'_, OmnisState>,
  input: MediaUploadInput,
) -> Result<MediaUploadResponse, String> {
  let MediaUploadInput {
    chat_id,
    mime_type,
    nonce,
    chunk_index,
    total_chunks,
    upload_id,
    file_name,
    bytes,
  } = input;

  if chat_id <= 0 {
    return Err("chat_id must be greater than zero".to_string());
  }
  if chunk_index < 0 {
    return Err("chunk_index must be >= 0".to_string());
  }
  if total_chunks < 1 {
    return Err("total_chunks must be >= 1".to_string());
  }
  if upload_id.trim().is_empty() {
    return Err("upload_id is required".to_string());
  }
  if mime_type.trim().is_empty() {
    return Err("mime_type is required".to_string());
  }
  if nonce.trim().is_empty() {
    return Err("nonce is required".to_string());
  }
  if bytes.is_empty() {
    return Err("bytes payload is empty".to_string());
  }

  let file_name = file_name
    .filter(|name| !name.trim().is_empty())
    .unwrap_or_else(|| format!("chunk-{chunk_index}"));
  let file_part = multipart::Part::bytes(bytes)
    .file_name(file_name)
    .mime_str("application/octet-stream")
    .map_err(|e| format!("failed to prepare upload part: {e}"))?;

  let form = multipart::Form::new()
    .part("file", file_part)
    .text("chat_id", chat_id.to_string())
    .text("mime_type", mime_type)
    .text("nonce", nonce)
    .text("chunk_index", chunk_index.to_string())
    .text("total_chunks", total_chunks.to_string())
    .text("upload_id", upload_id);

  let base = state.backend_url();
  let request = state
    .client
    .post(build_url(&base, "/media/upload"))
    .multipart(form);

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("media upload request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<MediaUploadResponse>()
    .await
    .map_err(|e| format!("invalid media upload payload: {e}"))
}

#[tauri::command]
async fn media_get_meta(state: State<'_, OmnisState>, media_id: i64) -> Result<MediaMetaResponse, String> {
  let base = state.backend_url();
  let path = format!("/media/{media_id}/meta");
  let request = state.client.get(build_url(&base, &path));

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("media meta request failed: {e}"))?;

  let response = ensure_success(response).await?;
  response
    .json::<MediaMetaResponse>()
    .await
    .map_err(|e| format!("invalid media meta payload: {e}"))
}

#[tauri::command]
async fn media_download(
  state: State<'_, OmnisState>,
  media_id: i64,
) -> Result<MediaDownloadResponse, String> {
  let base = state.backend_url();
  let path = format!("/media/download/{media_id}");
  let request = state.client.get(build_url(&base, &path));

  let response = with_auth_headers(state.inner(), request)?
    .send()
    .await
    .map_err(|e| format!("media download request failed: {e}"))?;

  let response = ensure_success(response).await?;
  let bytes = response
    .bytes()
    .await
    .map_err(|e| format!("failed reading media bytes: {e}"))?;

  Ok(MediaDownloadResponse {
    media_id,
    byte_len: bytes.len(),
    bytes_b64: B64.encode(bytes.as_ref()),
  })
}

#[tauri::command]
fn reset_config(state: State<'_, OmnisState>) -> Result<(), String> {
  state.reset_config();
  Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  let state = OmnisState::new().expect("failed to initialize Omnis desktop state");

  tauri::Builder::default()
    .manage(state)
    .invoke_handler(tauri::generate_handler![
      get_backend_config,
      set_backend_url,
      reset_config,
      auth_runtime,
      backend_health,
      auth_login,
      auth_me,
      auth_keyblob,
      auth_logout,
      users_search,
      chat_list,
      chat_create,
      chat_fetch,
      chat_fetch_epoch,
      chat_create_epoch,
      chat_send_message,
      chat_delete_message,
      sessions_list,
      sessions_revoke,
      sessions_revoke_other,
      media_upload_chunk,
      media_get_meta,
      media_download
    ])
    .setup(|app| {
      if let Ok(config_dir) = app.path().app_config_dir() {
        let omnis_state = app.state::<OmnisState>();
        omnis_state.init_config(&config_dir);
      }
      app.handle().plugin(tauri_plugin_notification::init())?;
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}