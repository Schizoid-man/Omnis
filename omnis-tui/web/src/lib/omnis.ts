type TauriWindow = Window & {
  __TAURI_INTERNALS__?: unknown
}

const DEFAULT_BACKEND_URL = 'http://127.0.0.1:6767'
const STORAGE_KEY = 'omnis.browser.runtime'

const PBKDF2_ITERATIONS = 100000
const AES_KEY_LENGTH = 256
const EC_CURVE = 'P-384'
const MEDIA_CHUNK_MAX_SIZE = 8 * 1024 * 1024

type BrowserRuntime = {
  backendUrl: string
  deviceId: string
  token: string | null
}

type EpochKeyEntry = {
  key: CryptoKey
  index: number
}

type KeyBlobPayload = {
  identity_pub: string
  encrypted_identity_priv: string
  kdf_salt: string
  aead_nonce: string
}

type PublicKeyPayload = {
  username: string
  identity_pub: string
}

type EpochFetchPayload = {
  epoch_id: number
  epoch_index: number
  wrapped_key: string
}

export type ChatEpoch = {
  epochId: number
  epochIndex?: number
  key: CryptoKey
}

export type MediaChunk = {
  media_id: number
  chunk_index: number
  file_size: number
}

export type MediaAttachment = {
  upload_id: string
  mime_type: string
  nonce: string
  total_chunks: number
  total_size: number
  chunks: MediaChunk[]
}

export type BackendConfig = {
  backendUrl: string
  deviceId: string
  hasToken: boolean
}

export type AuthRuntime = {
  backendUrl: string
  deviceId: string
  token: string | null
}

export type HealthResponse = {
  ping: string
  version: string
}

export type AuthSession = {
  token: string
  deviceId: string
  userId: number
  username: string
}

export type MePayload = {
  id: number
  username: string
}

export type ChatSummary = {
  chat_id: number
  with_user: string
}

export type SignupPayload = {
  id: number
  username: string
}

export type ChatMessage = {
  id: number
  sender_id: number
  message_type?: string
  call_uuid?: string | null
  call_status?: string | null
  duration_seconds?: number | null
  epoch_id: number | null
  reply_id: number | null
  ciphertext: string
  nonce: string
  deleted: boolean
  created_at: string
  attachments?: MediaAttachment[]
}

export type ChatFetchPayload = {
  messages: ChatMessage[]
  next_cursor: number | null
}

export type EpochCreateInput = {
  wrapped_key_a: string
  wrapped_key_b: string
}

export type EpochCreatePayload = {
  epoch_id: number
  epoch_index: number
}

export type SendMessageInput = {
  epoch_id: number
  ciphertext: string
  nonce: string
  reply_id?: number | null
  media_ids?: number[]
}

export type SendMessagePayload = {
  id: number
  epoch_id: number
  created_at: string
  attachments?: MediaAttachment[]
}

export type CallStatus = 'initiated' | 'ringing' | 'accepted' | 'missed' | 'rejected' | 'ended'

export type CallInitiatePayload = {
  call_id: string
  chat_id: number
  initiator_id: number
  recipient_id: number
  status: CallStatus
  created_at: string
}

export type CallHistoryEntry = {
  call_id: string
  initiator_id: number
  recipient_id: number
  status: CallStatus
  created_at: string
  answered_at?: string | null
  ended_at?: string | null
  duration_seconds?: number | null
}

export type CallHistoryPayload = {
  calls: CallHistoryEntry[]
  next_cursor: string | null
}

export type CallOfferKeyMaterial = {
  wrappedCallKey: string
  callKey: CryptoKey
}

const KeyStore = {
  identityKeyPair: null as CryptoKeyPair | null,
  epochKeys: new Map<number, Map<number, EpochKeyEntry>>(),
  peerPublicKeys: new Map<string, CryptoKey>(),

  clear() {
    this.identityKeyPair = null
    this.epochKeys.clear()
    this.peerPublicKeys.clear()
  },

  setIdentityKeyPair(keyPair: CryptoKeyPair) {
    this.identityKeyPair = keyPair
  },

  getIdentityKeyPair() {
    return this.identityKeyPair
  },

  setEpochKey(chatId: number, epochId: number, epochIndex: number, key: CryptoKey) {
    if (!this.epochKeys.has(chatId)) {
      this.epochKeys.set(chatId, new Map())
    }
    this.epochKeys.get(chatId)!.set(epochId, { key, index: epochIndex })
  },

  getEpochKey(chatId: number, epochId: number) {
    const chatEpochs = this.epochKeys.get(chatId)
    if (!chatEpochs) {
      return null
    }
    return chatEpochs.get(epochId)?.key ?? null
  },

  getLatestEpoch(chatId: number) {
    const chatEpochs = this.epochKeys.get(chatId)
    if (!chatEpochs || chatEpochs.size === 0) {
      return null
    }

    let latestEpochId = -1
    let latest: EpochKeyEntry | null = null
    for (const [epochId, entry] of chatEpochs.entries()) {
      if (!latest || entry.index > latest.index) {
        latestEpochId = epochId
        latest = entry
      }
    }

    if (!latest) {
      return null
    }

    return {
      epochId: latestEpochId,
      epochIndex: latest.index,
      key: latest.key,
    }
  },

  invalidateChat(chatId: number) {
    this.epochKeys.delete(chatId)
  },

  setPeerPublicKey(username: string, publicKey: CryptoKey) {
    this.peerPublicKeys.set(username, publicKey)
  },

  getPeerPublicKey(username: string) {
    return this.peerPublicKeys.get(username) ?? null
  },
}

function randomDeviceId() {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID().toLowerCase()
  }
  return `device-${Date.now()}`
}

function randomOpaqueToken(byteCount = 16) {
  const bytes = new Uint8Array(byteCount)
  if (typeof crypto !== 'undefined' && typeof crypto.getRandomValues === 'function') {
    crypto.getRandomValues(bytes)
  } else {
    for (let index = 0; index < byteCount; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256)
    }
  }

  let binary = ''
  for (const value of bytes) {
    binary += String.fromCharCode(value)
  }

  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

function arrayBufferToBase64(buffer: ArrayBuffer | Uint8Array) {
  const bytes = buffer instanceof Uint8Array ? buffer : new Uint8Array(buffer)
  let binary = ''
  for (let i = 0; i < bytes.byteLength; i += 1) {
    binary += String.fromCharCode(bytes[i])
  }
  return btoa(binary)
}

function base64ToArrayBuffer(base64: string) {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i)
  }
  return bytes.buffer
}

function randomBytes(length: number) {
  return crypto.getRandomValues(new Uint8Array(length))
}

async function deriveKeyFromPassword(password: string, salt: Uint8Array) {
  const encoder = new TextEncoder()
  const passwordKey = await crypto.subtle.importKey('raw', encoder.encode(password), 'PBKDF2', false, ['deriveKey'])

  return crypto.subtle.deriveKey(
    {
      name: 'PBKDF2',
      salt: salt as unknown as BufferSource,
      iterations: PBKDF2_ITERATIONS,
      hash: 'SHA-256',
    },
    passwordKey,
    { name: 'AES-GCM', length: AES_KEY_LENGTH },
    false,
    ['encrypt', 'decrypt'],
  )
}

async function generateIdentityKeyPair() {
  return crypto.subtle.generateKey({ name: 'ECDH', namedCurve: EC_CURVE }, true, ['deriveKey', 'deriveBits'])
}

async function exportPublicKey(publicKey: CryptoKey) {
  const exported = await crypto.subtle.exportKey('spki', publicKey)
  return arrayBufferToBase64(exported)
}

async function importPublicKey(base64: string) {
  return crypto.subtle.importKey(
    'spki',
    base64ToArrayBuffer(base64),
    { name: 'ECDH', namedCurve: EC_CURVE },
    true,
    [],
  )
}

async function exportPrivateKey(privateKey: CryptoKey) {
  const exported = await crypto.subtle.exportKey('pkcs8', privateKey)
  return arrayBufferToBase64(exported)
}

async function importPrivateKey(base64: string) {
  return crypto.subtle.importKey(
    'pkcs8',
    base64ToArrayBuffer(base64),
    { name: 'ECDH', namedCurve: EC_CURVE },
    true,
    ['deriveKey', 'deriveBits'],
  )
}

async function encryptAESGCM(key: CryptoKey, plaintext: string | Uint8Array, nonce: Uint8Array) {
  const encoder = new TextEncoder()
  const data = typeof plaintext === 'string' ? encoder.encode(plaintext) : plaintext
  const ciphertext = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv: nonce as unknown as BufferSource },
    key,
    data as unknown as BufferSource,
  )
  return new Uint8Array(ciphertext)
}

async function decryptAESGCM(key: CryptoKey, ciphertext: BufferSource, nonce: Uint8Array) {
  return crypto.subtle.decrypt(
    { name: 'AES-GCM', iv: nonce as unknown as BufferSource },
    key,
    ciphertext,
  )
}

async function encryptIdentityPrivateKey(privateKey: CryptoKey, password: string) {
  const salt = randomBytes(32)
  const nonce = randomBytes(12)
  const derivedKey = await deriveKeyFromPassword(password, salt)
  const privateKeyBase64 = await exportPrivateKey(privateKey)
  const ciphertext = await encryptAESGCM(derivedKey, privateKeyBase64, nonce)

  return {
    encrypted_identity_priv: arrayBufferToBase64(ciphertext),
    kdf_salt: arrayBufferToBase64(salt),
    aead_nonce: arrayBufferToBase64(nonce),
  }
}

async function decryptIdentityPrivateKey(keyBlob: KeyBlobPayload, password: string) {
  const salt = new Uint8Array(base64ToArrayBuffer(keyBlob.kdf_salt))
  const nonce = new Uint8Array(base64ToArrayBuffer(keyBlob.aead_nonce))
  const ciphertext = base64ToArrayBuffer(keyBlob.encrypted_identity_priv)

  const derivedKey = await deriveKeyFromPassword(password, salt)
  const plaintextBuffer = await decryptAESGCM(derivedKey, ciphertext, nonce)
  const decoder = new TextDecoder()
  return importPrivateKey(decoder.decode(plaintextBuffer))
}

async function generateEpochKey() {
  return crypto.subtle.generateKey({ name: 'AES-GCM', length: AES_KEY_LENGTH }, true, ['encrypt', 'decrypt'])
}

async function exportEpochKey(key: CryptoKey) {
  return crypto.subtle.exportKey('raw', key)
}

async function importEpochKey(rawKey: BufferSource) {
  return crypto.subtle.importKey('raw', rawKey, { name: 'AES-GCM', length: AES_KEY_LENGTH }, true, ['encrypt', 'decrypt'])
}

async function wrapEpochKeyForRecipient(epochKey: CryptoKey, myPrivateKey: CryptoKey, recipientPublicKey: CryptoKey) {
  const sharedBits = await crypto.subtle.deriveBits({ name: 'ECDH', public: recipientPublicKey }, myPrivateKey, 384)
  const sharedSecret = await crypto.subtle.importKey('raw', sharedBits, 'HKDF', false, ['deriveKey'])

  const wrapKey = await crypto.subtle.deriveKey(
    {
      name: 'HKDF',
      salt: new Uint8Array(32),
      info: new TextEncoder().encode('epoch-key-wrap'),
      hash: 'SHA-256',
    },
    sharedSecret,
    { name: 'AES-GCM', length: 256 },
    false,
    ['encrypt', 'decrypt'],
  )

  const rawEpochKey = await exportEpochKey(epochKey)
  const nonce = randomBytes(12)
  const wrapped = await encryptAESGCM(wrapKey, new Uint8Array(rawEpochKey), nonce)

  const combined = new Uint8Array(nonce.length + wrapped.length)
  combined.set(nonce, 0)
  combined.set(wrapped, nonce.length)

  return arrayBufferToBase64(combined)
}

async function unwrapEpochKey(wrappedKeyBase64: string, myPrivateKey: CryptoKey, senderPublicKey: CryptoKey) {
  const wrappedData = new Uint8Array(base64ToArrayBuffer(wrappedKeyBase64))
  const nonce = wrappedData.slice(0, 12)
  const wrapped = wrappedData.slice(12)

  const sharedBits = await crypto.subtle.deriveBits({ name: 'ECDH', public: senderPublicKey }, myPrivateKey, 384)
  const sharedSecret = await crypto.subtle.importKey('raw', sharedBits, 'HKDF', false, ['deriveKey'])

  const wrapKey = await crypto.subtle.deriveKey(
    {
      name: 'HKDF',
      salt: new Uint8Array(32),
      info: new TextEncoder().encode('epoch-key-wrap'),
      hash: 'SHA-256',
    },
    sharedSecret,
    { name: 'AES-GCM', length: 256 },
    false,
    ['encrypt', 'decrypt'],
  )

  const rawEpochKey = await decryptAESGCM(wrapKey, wrapped, nonce)
  return importEpochKey(rawEpochKey)
}

async function encryptMessageWithEpoch(message: string, epochKey: CryptoKey) {
  const nonce = randomBytes(12)
  const ciphertext = await encryptAESGCM(epochKey, message, nonce)
  return {
    ciphertext: arrayBufferToBase64(ciphertext),
    nonce: arrayBufferToBase64(nonce),
  }
}

async function decryptMessageWithEpoch(ciphertextBase64: string, nonceBase64: string, epochKey: CryptoKey) {
  const ciphertext = base64ToArrayBuffer(ciphertextBase64)
  const nonce = new Uint8Array(base64ToArrayBuffer(nonceBase64))
  const plaintextBuffer = await decryptAESGCM(epochKey, ciphertext, nonce)
  return new TextDecoder().decode(plaintextBuffer)
}

async function encryptBinaryWithEpoch(buffer: ArrayBuffer, epochKey: CryptoKey) {
  const nonce = randomBytes(12)
  const ciphertext = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv: nonce as unknown as BufferSource },
    epochKey,
    buffer as unknown as BufferSource,
  )

  return {
    encrypted: new Uint8Array(ciphertext),
    nonce: arrayBufferToBase64(nonce),
  }
}

async function decryptBinaryWithEpoch(buffer: ArrayBuffer, nonceBase64: string, epochKey: CryptoKey) {
  const nonce = new Uint8Array(base64ToArrayBuffer(nonceBase64))
  const plaintext = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv: nonce as unknown as BufferSource },
    epochKey,
    buffer as unknown as BufferSource,
  )

  return new Uint8Array(plaintext)
}

function generateUploadId() {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }

  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (char) => {
    const rand = Math.floor(Math.random() * 16)
    const value = char === 'x' ? rand : (rand & 0x3) | 0x8
    return value.toString(16)
  })
}

function chunkArrayBuffer(buffer: ArrayBuffer, chunkSize = MEDIA_CHUNK_MAX_SIZE) {
  if (buffer.byteLength === 0) {
    return [buffer]
  }

  const chunks: ArrayBuffer[] = []
  const totalChunks = Math.max(1, Math.ceil(buffer.byteLength / chunkSize))

  for (let index = 0; index < totalChunks; index += 1) {
    const start = index * chunkSize
    const end = Math.min(start + chunkSize, buffer.byteLength)
    chunks.push(buffer.slice(start, end))
  }

  return chunks
}

function canUseBrowserStorage() {
  return typeof window !== 'undefined' && typeof window.localStorage !== 'undefined'
}

function loadBrowserRuntime(): BrowserRuntime {
  const fallback: BrowserRuntime = {
    backendUrl: DEFAULT_BACKEND_URL,
    deviceId: randomDeviceId(),
    token: null,
  }

  if (!canUseBrowserStorage()) {
    return fallback
  }

  const raw = window.localStorage.getItem(STORAGE_KEY)
  if (!raw) {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(fallback))
    return fallback
  }

  try {
    const parsed = JSON.parse(raw) as Partial<BrowserRuntime>
    const runtime: BrowserRuntime = {
      backendUrl: parsed.backendUrl?.trim() || DEFAULT_BACKEND_URL,
      deviceId: parsed.deviceId || randomDeviceId(),
      token: typeof parsed.token === 'string' ? parsed.token : null,
    }
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(runtime))
    return runtime
  } catch {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(fallback))
    return fallback
  }
}

function saveBrowserRuntime(partial: Partial<BrowserRuntime>): BrowserRuntime {
  const current = loadBrowserRuntime()
  const next: BrowserRuntime = {
    backendUrl: partial.backendUrl?.trim() || current.backendUrl,
    deviceId: partial.deviceId || current.deviceId,
    token: partial.token === undefined ? current.token : partial.token,
  }

  if (canUseBrowserStorage()) {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
  }
  return next
}

function normalizeBaseUrl(input: string) {
  return input.trim().replace(/\/$/, '')
}

export function isTauriRuntime(): boolean {
  if (typeof window === 'undefined') {
    return false
  }
  return Boolean((window as TauriWindow).__TAURI_INTERNALS__)
}

function canUseWindowNotifications() {
  return typeof window !== 'undefined' && typeof window.Notification !== 'undefined'
}

function isGrantedNotificationPermission(value: unknown) {
  if (typeof value === 'boolean') {
    return value
  }

  if (typeof value === 'string') {
    return value.trim().toLowerCase() === 'granted'
  }

  return false
}

async function tauriIsNotificationPermissionGranted() {
  try {
    const granted = await invokeTauri<boolean | null>('plugin:notification|is_permission_granted')
    if (typeof granted === 'boolean') {
      return granted
    }
  } catch {
    // fallback to browser Notification API check
  }

  return null
}

export async function isMessageNotificationPermissionGranted() {
  if (isTauriRuntime()) {
    const granted = await tauriIsNotificationPermissionGranted()
    if (typeof granted === 'boolean') {
      return granted
    }
  }

  if (!canUseWindowNotifications()) {
    return false
  }

  return window.Notification.permission === 'granted'
}

export async function requestMessageNotificationPermission() {
  if (await isMessageNotificationPermissionGranted()) {
    return true
  }

  if (isTauriRuntime()) {
    try {
      const permission = await invokeTauri<string>('plugin:notification|request_permission')
      if (isGrantedNotificationPermission(permission)) {
        return true
      }
    } catch {
      // fallback to browser Notification API request
    }
  }

  if (!canUseWindowNotifications()) {
    return false
  }

  try {
    const permission = await window.Notification.requestPermission()
    return permission === 'granted'
  } catch {
    return false
  }
}

export async function ensureMessageNotificationPermission() {
  return requestMessageNotificationPermission()
}

export async function notifyIncomingMessage(username: string) {
  const granted = await isMessageNotificationPermissionGranted()
  if (!granted) {
    return
  }

  const senderName = username.trim() || 'a contact'
  const title = `New message from ${senderName}`
  const body = 'Open Omnis to read it.'

  if (isTauriRuntime()) {
    try {
      await invokeTauri<void>('plugin:notification|notify', {
        options: {
          title,
          body,
        },
      })
      return
    } catch {
      // fallback to browser notification
    }
  }

  if (!canUseWindowNotifications()) {
    return
  }

  try {
    new window.Notification(title, { body })
  } catch {
    // no-op
  }
}

function normalizeError(error: unknown): string {
  if (typeof error === 'string') {
    try {
      const parsed = JSON.parse(error) as { status?: number; message?: string }
      if (parsed.message) {
        return `HTTP ${parsed.status ?? 'error'}: ${parsed.message}`
      }
      return error
    } catch {
      return error
    }
  }

  if (error instanceof Error) {
    return error.message
  }

  return String(error)
}

async function invokeTauri<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const api = await import('@tauri-apps/api/core')
  return api.invoke<T>(command, args)
}

async function resolveRuntimeContext(): Promise<BrowserRuntime> {
  const local = loadBrowserRuntime()

  if (!isTauriRuntime()) {
    return local
  }

  try {
    const native = await invokeTauri<AuthRuntime>('auth_runtime')
    const merged: BrowserRuntime = {
      backendUrl: native.backendUrl || local.backendUrl,
      deviceId: native.deviceId || local.deviceId,
      token: local.token ?? native.token ?? null,
    }

    const changed =
      merged.backendUrl !== local.backendUrl ||
      merged.deviceId !== local.deviceId ||
      merged.token !== local.token

    if (changed) {
      saveBrowserRuntime(merged)
    }

    return merged
  } catch {
    return local
  }
}

async function requestJson<T>(path: string, options: RequestInit = {}, requireAuth = false): Promise<T> {
  const runtime = await resolveRuntimeContext()
  const headers = new Headers(options.headers)
  headers.set('Content-Type', 'application/json')
  headers.set('X-Device-ID', runtime.deviceId)

  if (requireAuth) {
    if (!runtime.token) {
      throw new Error('Not authenticated')
    }
    headers.set('Authorization', `Bearer ${runtime.token}`)
  }

  const response = await fetch(`${normalizeBaseUrl(runtime.backendUrl)}${path}`, {
    ...options,
    headers,
  })

  if (!response.ok) {
    let message = response.statusText
    try {
      const payload = (await response.json()) as { detail?: string; message?: string }
      message = payload.detail || payload.message || message
    } catch {
      // no-op
    }
    throw new Error(`HTTP ${response.status}: ${message}`)
  }

  if (response.status === 204) {
    return undefined as T
  }

  return (await response.json()) as T
}

async function uploadAuthenticatedMultipart(path: string, body: FormData) {
  const runtime = await resolveRuntimeContext()
  if (!runtime.token) {
    throw new Error('Not authenticated')
  }

  const headers = new Headers()
  headers.set('Authorization', `Bearer ${runtime.token}`)
  headers.set('X-Device-ID', runtime.deviceId)

  const response = await fetch(`${normalizeBaseUrl(runtime.backendUrl)}${path}`, {
    method: 'POST',
    headers,
    body,
  })

  if (!response.ok) {
    let message = response.statusText
    try {
      const payload = (await response.json()) as { detail?: string; message?: string }
      message = payload.detail || payload.message || message
    } catch {
      // no-op
    }
    throw new Error(`HTTP ${response.status}: ${message}`)
  }

  return (await response.json()) as Record<string, unknown>
}

async function downloadAuthenticatedBinary(path: string) {
  const runtime = await resolveRuntimeContext()
  if (!runtime.token) {
    throw new Error('Not authenticated')
  }

  const headers = new Headers()
  headers.set('Authorization', `Bearer ${runtime.token}`)
  headers.set('X-Device-ID', runtime.deviceId)

  const response = await fetch(`${normalizeBaseUrl(runtime.backendUrl)}${path}`, {
    method: 'GET',
    headers,
  })

  if (!response.ok) {
    let message = response.statusText
    try {
      const payload = (await response.json()) as { detail?: string; message?: string }
      message = payload.detail || payload.message || message
    } catch {
      // no-op
    }
    throw new Error(`HTTP ${response.status}: ${message}`)
  }

  return response.arrayBuffer()
}

async function sleep(ms: number) {
  return new Promise<void>((resolve) => {
    setTimeout(resolve, ms)
  })
}

export function createMessageNonce() {
  return randomOpaqueToken(18)
}

export function hasUnlockedIdentityKeys() {
  return KeyStore.getIdentityKeyPair() !== null
}

export function clearCryptoState() {
  KeyStore.clear()
}

export async function getBackendConfig(): Promise<BackendConfig> {
  const local = loadBrowserRuntime()

  if (isTauriRuntime()) {
    try {
      const native = await invokeTauri<BackendConfig>('get_backend_config')
      const merged = saveBrowserRuntime({
        backendUrl: native.backendUrl,
        deviceId: native.deviceId,
      })
      return {
        backendUrl: merged.backendUrl,
        deviceId: merged.deviceId,
        hasToken: Boolean(merged.token || native.hasToken),
      }
    } catch {
      // fallback to local runtime
    }
  }

  return {
    backendUrl: local.backendUrl,
    deviceId: local.deviceId,
    hasToken: Boolean(local.token),
  }
}

export async function setBackendUrl(url: string): Promise<BackendConfig> {
  if (isTauriRuntime()) {
    try {
      const native = await invokeTauri<BackendConfig>('set_backend_url', { url })
      const merged = saveBrowserRuntime({ backendUrl: native.backendUrl, deviceId: native.deviceId })
      return {
        backendUrl: merged.backendUrl,
        deviceId: merged.deviceId,
        hasToken: Boolean(merged.token || native.hasToken),
      }
    } catch {
      // fallback to local runtime
    }
  }

  const runtime = saveBrowserRuntime({ backendUrl: url })
  return {
    backendUrl: runtime.backendUrl,
    deviceId: runtime.deviceId,
    hasToken: Boolean(runtime.token),
  }
}

export async function authRuntime(): Promise<AuthRuntime> {
  return resolveRuntimeContext()
}

async function authKeyBlob() {
  return requestJson<KeyBlobPayload>('/auth/keyblob', { method: 'GET' }, true)
}

export async function unlockIdentityKeys(password: string) {
  const keyBlob = await authKeyBlob()
  const privateKey = await decryptIdentityPrivateKey(keyBlob, password)
  const publicKey = await importPublicKey(keyBlob.identity_pub)
  KeyStore.setIdentityKeyPair({ privateKey, publicKey })
}

export async function authSignup(username: string, password: string): Promise<SignupPayload> {
  const trimmedUsername = username.trim()
  if (!trimmedUsername) {
    throw new Error('Username is required')
  }

  const keyPair = await generateIdentityKeyPair()
  const identityPub = await exportPublicKey(keyPair.publicKey)
  const encryptedBlob = await encryptIdentityPrivateKey(keyPair.privateKey, password)

  const payload = await requestJson<SignupPayload>(
    '/auth/signup',
    {
      method: 'POST',
      body: JSON.stringify({
        username: trimmedUsername,
        password,
        identity_pub: identityPub,
        encrypted_identity_priv: encryptedBlob.encrypted_identity_priv,
        kdf_salt: encryptedBlob.kdf_salt,
        aead_nonce: encryptedBlob.aead_nonce,
      }),
    },
    false,
  )

  KeyStore.setIdentityKeyPair(keyPair)
  return payload
}

export async function backendHealth(): Promise<HealthResponse> {
  const runtime = await resolveRuntimeContext()
  const base = normalizeBaseUrl(runtime.backendUrl)

  try {
    const pingResponse = await fetch(`${base}/`)
    if (!pingResponse.ok) {
      throw new Error(`HTTP ${pingResponse.status}: ${pingResponse.statusText}`)
    }
    const pingPayload = (await pingResponse.json()) as {
      ping?: string
      PING?: string
    }

    const versionResponse = await fetch(`${base}/version`)
    if (!versionResponse.ok) {
      throw new Error(`HTTP ${versionResponse.status}: ${versionResponse.statusText}`)
    }
    const versionPayload = (await versionResponse.json()) as { version: string }

    return {
      ping: pingPayload.ping || pingPayload.PING || 'ok',
      version: versionPayload.version,
    }
  } catch (error) {
    const message = normalizeError(error)
    const isLikelyTlsTrustIssue =
      base.startsWith('https://') &&
      (message.toLowerCase().includes('failed to fetch') || message.toLowerCase().includes('networkerror'))

    if (isLikelyTlsTrustIssue) {
      throw new Error(
        'TLS certificate is not trusted by this runtime. Install your self-signed CA/certificate into Windows Trusted Root Certification Authorities (and ensure the cert SAN matches the host), or use HTTP for local dev.',
      )
    }

    throw error
  }
}

export async function authLogin(username: string, password: string): Promise<AuthSession> {
  const login = await requestJson<{ token: string }>(
    '/auth/login',
    {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    },
    false,
  )

  const runtime = await resolveRuntimeContext()
  const updated = saveBrowserRuntime({ token: login.token, deviceId: runtime.deviceId })
  const me = await authMe()

  try {
    await unlockIdentityKeys(password)
  } catch (error) {
    try {
      await requestJson<void>('/auth/logout', { method: 'POST' }, true)
    } catch {
      // no-op
    }
    saveBrowserRuntime({ token: null })
    KeyStore.clear()
    throw new Error(`Failed to unlock identity keys: ${normalizeError(error)}`)
  }

  return {
    token: login.token,
    deviceId: updated.deviceId,
    userId: me.id,
    username: me.username,
  }
}

export async function authMe(): Promise<MePayload> {
  return requestJson<MePayload>('/auth/me', { method: 'GET' }, true)
}

export async function authLogout(): Promise<void> {
  const runtime = await resolveRuntimeContext()
  if (runtime.token) {
    try {
      await requestJson<void>('/auth/logout', { method: 'POST' }, true)
    } catch {
      // no-op
    }
  }
  saveBrowserRuntime({ token: null })
  KeyStore.clear()
}

export async function chatList(): Promise<ChatSummary[]> {
  return requestJson<ChatSummary[]>('/chat/list', { method: 'GET' }, true)
}

export async function chatCreate(username: string): Promise<{ chat_id: number }> {
  return requestJson<{ chat_id: number }>(
    '/chat/create',
    {
      method: 'POST',
      body: JSON.stringify({ username }),
    },
    true,
  )
}

export async function chatFetch(chatId: number, beforeId?: number, limit = 50): Promise<ChatFetchPayload> {
  const safeLimit = Math.min(100, Math.max(1, limit))
  const search = new URLSearchParams()
  search.set('limit', String(safeLimit))
  if (beforeId !== undefined) {
    search.set('before_id', String(beforeId))
  }
  const query = search.toString()

  return requestJson<ChatFetchPayload>(`/chat/fetch/${chatId}${query ? `?${query}` : ''}`, { method: 'GET' }, true)
}

export async function chatCreateEpoch(chatId: number, input: EpochCreateInput): Promise<EpochCreatePayload> {
  return requestJson<EpochCreatePayload>(
    `/chat/${chatId}/epoch`,
    {
      method: 'POST',
      body: JSON.stringify(input),
    },
    true,
  )
}

export async function chatSendMessage(chatId: number, input: SendMessageInput): Promise<SendMessagePayload> {
  return requestJson<SendMessagePayload>(
    `/chat/${chatId}/message`,
    {
      method: 'POST',
      body: JSON.stringify(input),
    },
    true,
  )
}

export async function callInitiate(chatId: number): Promise<CallInitiatePayload> {
  return requestJson<CallInitiatePayload>(`/call/${chatId}/initiate`, { method: 'POST' }, true)
}

export async function callHistory(chatId: number, before?: string, limit = 20): Promise<CallHistoryPayload> {
  const safeLimit = Math.min(50, Math.max(1, limit))
  const search = new URLSearchParams()
  search.set('limit', String(safeLimit))
  if (before) {
    search.set('before', before)
  }
  const query = search.toString()
  return requestJson<CallHistoryPayload>(`/call/${chatId}/history${query ? `?${query}` : ''}`, { method: 'GET' }, true)
}

export async function connectCallWebSocket(callId: string): Promise<WebSocket> {
  const runtime = await resolveRuntimeContext()
  if (!runtime.token) {
    throw new Error('Not authenticated')
  }

  const wsBase = normalizeBaseUrl(runtime.backendUrl)
    .replace(/^https:\/\//, 'wss://')
    .replace(/^http:\/\//, 'ws://')

  const url = `${wsBase}/call/ws/${encodeURIComponent(callId)}?token=${encodeURIComponent(runtime.token)}&device_id=${encodeURIComponent(runtime.deviceId)}`
  return new WebSocket(url)
}

export async function createWrappedCallKeyForPeer(peerUsername: string): Promise<CallOfferKeyMaterial> {
  const keyPair = KeyStore.getIdentityKeyPair()
  if (!keyPair) {
    throw new Error('Identity keys are locked. Please login again.')
  }

  const peerPublicKey = await getPeerPublicKey(peerUsername)
  const callKey = await generateEpochKey()

  const wrappedCallKey = await wrapEpochKeyForRecipient(callKey, keyPair.privateKey, peerPublicKey)

  return {
    wrappedCallKey,
    callKey,
  }
}

export async function unwrapCallKeyFromPeer(peerUsername: string, wrappedCallKey: string): Promise<CryptoKey> {
  const keyPair = KeyStore.getIdentityKeyPair()
  if (!keyPair) {
    throw new Error('Identity keys are locked. Please login again.')
  }

  const peerPublicKey = await getPeerPublicKey(peerUsername)
  return unwrapEpochKey(wrappedCallKey, keyPair.privateKey, peerPublicKey)
}

export async function encryptCallAudioFrame(
  callKey: CryptoKey,
  audioChunk: ArrayBuffer,
): Promise<{ data: string; nonce: string }> {
  const nonce = randomBytes(12)
  const encrypted = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv: nonce as unknown as BufferSource },
    callKey,
    audioChunk as unknown as BufferSource,
  )

  return {
    data: arrayBufferToBase64(encrypted),
    nonce: arrayBufferToBase64(nonce),
  }
}

export async function decryptCallAudioFrame(
  callKey: CryptoKey,
  encryptedBase64: string,
  nonceBase64: string,
): Promise<ArrayBuffer> {
  const encrypted = base64ToArrayBuffer(encryptedBase64)
  const nonce = new Uint8Array(base64ToArrayBuffer(nonceBase64))

  return crypto.subtle.decrypt(
    { name: 'AES-GCM', iv: nonce as unknown as BufferSource },
    callKey,
    encrypted as unknown as BufferSource,
  )
}

async function getPeerPublicKey(username: string) {
  const cached = KeyStore.getPeerPublicKey(username)
  if (cached) {
    return cached
  }

  const payload = await requestJson<PublicKeyPayload>(
    `/user/pkey/get?username=${encodeURIComponent(username)}`,
    { method: 'GET' },
    false,
  )
  const publicKey = await importPublicKey(payload.identity_pub)
  KeyStore.setPeerPublicKey(username, publicKey)
  return publicKey
}

async function createEpoch(chatId: number, peerUsername: string) {
  const keyPair = KeyStore.getIdentityKeyPair()
  if (!keyPair) {
    throw new Error('Identity keys are locked. Please login again.')
  }

  const peerPublicKey = await getPeerPublicKey(peerUsername)
  const epochKey = await generateEpochKey()

  const wrappedKeyForSelf = await wrapEpochKeyForRecipient(epochKey, keyPair.privateKey, peerPublicKey)
  const wrappedKeyForPeer = wrappedKeyForSelf

  const response = await chatCreateEpoch(chatId, {
    wrapped_key_a: wrappedKeyForSelf,
    wrapped_key_b: wrappedKeyForPeer,
  })

  KeyStore.setEpochKey(chatId, response.epoch_id, response.epoch_index, epochKey)

  return {
    epochId: response.epoch_id,
    epochIndex: response.epoch_index,
    key: epochKey,
  }
}

async function fetchEpochKey(chatId: number, epochId: number, peerUsername: string) {
  const cached = KeyStore.getEpochKey(chatId, epochId)
  if (cached) {
    return { epochId, key: cached }
  }

  const keyPair = KeyStore.getIdentityKeyPair()
  if (!keyPair) {
    throw new Error('Identity keys are locked. Please login again.')
  }

  const peerPublicKey = await getPeerPublicKey(peerUsername)
  const epoch = await requestJson<EpochFetchPayload>(`/chat/${chatId}/${epochId}/fetch`, { method: 'GET' }, true)

  if (!epoch.wrapped_key) {
    throw new Error('Epoch not initialized')
  }

  const epochKey = await unwrapEpochKey(epoch.wrapped_key, keyPair.privateKey, peerPublicKey)
  KeyStore.setEpochKey(chatId, epoch.epoch_id, epoch.epoch_index, epochKey)

  return {
    epochId: epoch.epoch_id,
    epochIndex: epoch.epoch_index,
    key: epochKey,
  }
}

async function getLatestEpochFromMessages(chatId: number, peerUsername: string) {
  const payload = await chatFetch(chatId, undefined, 50)
  if (!payload.messages || payload.messages.length === 0) {
    return null
  }

  for (let index = payload.messages.length - 1; index >= 0; index -= 1) {
    const candidate = payload.messages[index]
    if (candidate.message_type === 'call') {
      continue
    }
    if (typeof candidate.epoch_id !== 'number') {
      continue
    }
    return fetchEpochKey(chatId, candidate.epoch_id, peerUsername)
  }

  return null
}

async function getOrCreateEpoch(chatId: number, peerUsername: string, forceRefresh = false) {
  if (forceRefresh) {
    KeyStore.invalidateChat(chatId)
  }

  const cached = KeyStore.getLatestEpoch(chatId)
  if (cached) {
    return {
      epochId: cached.epochId,
      epochIndex: cached.epochIndex,
      key: cached.key,
    }
  }

  const latestEpoch = await getLatestEpochFromMessages(chatId, peerUsername)
  if (latestEpoch) {
    return latestEpoch
  }

  try {
    return await createEpoch(chatId, peerUsername)
  } catch (error) {
    const message = normalizeError(error)

    if (message.includes('Epoch creation throttled')) {
      await sleep(5200)
      return createEpoch(chatId, peerUsername)
    }

    if (message.includes('Epoch rotation not allowed yet')) {
      const fallback = await getLatestEpochFromMessages(chatId, peerUsername)
      if (fallback) {
        return fallback
      }
    }

    throw error
  }
}

export async function resolveChatEpoch(chatId: number, peerUsername: string, forceRefreshEpoch = false): Promise<ChatEpoch> {
  return getOrCreateEpoch(chatId, peerUsername, forceRefreshEpoch)
}

export async function encryptMessageForEpoch(plaintext: string, epoch: ChatEpoch): Promise<SendMessageInput> {
  const encrypted = await encryptMessageWithEpoch(plaintext, epoch.key)

  return {
    epoch_id: epoch.epochId,
    ciphertext: encrypted.ciphertext,
    nonce: encrypted.nonce,
  }
}

export async function uploadChatMedia(
  chatId: number,
  file: File,
  epoch: ChatEpoch,
  onProgress?: (progress: number) => void,
) {
  const fileBuffer = await file.arrayBuffer()
  const { encrypted, nonce } = await encryptBinaryWithEpoch(fileBuffer, epoch.key)

  const chunks = chunkArrayBuffer(encrypted.buffer)
  const totalChunks = chunks.length
  const uploadId = generateUploadId()
  let lastMediaId: number | null = null

  for (let index = 0; index < totalChunks; index += 1) {
    const formData = new FormData()
    formData.append('file', new Blob([chunks[index]]), file.name || `chunk-${index}`)
    formData.append('chat_id', String(chatId))
    formData.append('mime_type', file.type || 'application/octet-stream')
    formData.append('nonce', nonce)
    formData.append('chunk_index', String(index))
    formData.append('total_chunks', String(totalChunks))
    formData.append('upload_id', uploadId)

    const payload = await uploadAuthenticatedMultipart('/media/upload', formData)
    const mediaId = payload.media_id
    if (typeof mediaId !== 'number') {
      throw new Error('Upload failed: media id missing in response')
    }

    lastMediaId = mediaId
    onProgress?.((index + 1) / totalChunks)
  }

  if (lastMediaId === null) {
    throw new Error('Upload failed: no chunks uploaded')
  }

  return lastMediaId
}

export async function downloadChatAttachment(
  chatId: number,
  peerUsername: string,
  messageEpochId: number,
  attachment: MediaAttachment,
) {
  if (!attachment.chunks || attachment.chunks.length === 0) {
    throw new Error('Attachment has no chunks')
  }

  const epoch = await fetchEpochKey(chatId, messageEpochId, peerUsername)
  const sortedChunks = [...attachment.chunks].sort((left, right) => left.chunk_index - right.chunk_index)
  const downloadedChunks = await Promise.all(
    sortedChunks.map((chunk) => downloadAuthenticatedBinary(`/media/download/${chunk.media_id}`)),
  )

  const totalSize = downloadedChunks.reduce((sum, chunk) => sum + chunk.byteLength, 0)
  const encryptedBlob = new Uint8Array(totalSize)
  let offset = 0

  for (const chunk of downloadedChunks) {
    encryptedBlob.set(new Uint8Array(chunk), offset)
    offset += chunk.byteLength
  }

  const decrypted = await decryptBinaryWithEpoch(encryptedBlob.buffer, attachment.nonce, epoch.key)
  return new Blob([decrypted], { type: attachment.mime_type || 'application/octet-stream' })
}

export function invalidateChatEpochCache(chatId: number) {
  KeyStore.invalidateChat(chatId)
}

export async function primeEpochKeys(chatId: number, peerUsername: string, messages: ChatMessage[]) {
  const epochIds = new Set(
    messages
      .filter((message) => message.message_type !== 'call' && typeof message.epoch_id === 'number')
      .map((message) => message.epoch_id as number),
  )
  for (const epochId of epochIds) {
    if (!KeyStore.getEpochKey(chatId, epochId)) {
      await fetchEpochKey(chatId, epochId, peerUsername)
    }
  }
}

export async function decryptChatMessage(chatId: number, peerUsername: string, message: ChatMessage) {
  if (message.message_type === 'call') {
    return ''
  }
  if (typeof message.epoch_id !== 'number') {
    throw new Error('Message does not have an epoch id')
  }

  let epochKey = KeyStore.getEpochKey(chatId, message.epoch_id)
  if (!epochKey) {
    const fetched = await fetchEpochKey(chatId, message.epoch_id, peerUsername)
    epochKey = fetched.key
  }
  return decryptMessageWithEpoch(message.ciphertext, message.nonce, epochKey)
}

export async function encryptChatMessage(
  chatId: number,
  peerUsername: string,
  plaintext: string,
  forceRefreshEpoch = false,
): Promise<SendMessageInput> {
  const epoch = await getOrCreateEpoch(chatId, peerUsername, forceRefreshEpoch)
  return encryptMessageForEpoch(plaintext, epoch)
}
