'use client'

import { useEffect, useMemo, useRef, useState } from 'react'
import {
  Activity,
  ArrowLeft,
  Bell,
  BellOff,
  ChevronDown,
  CheckCircle2,
  Clock3,
  CornerUpLeft,
  Download,
  Eye,
  LogIn,
  LogOut,
  MessageCircle,
  MessageSquarePlus,
  Paperclip,
  Phone,
  PhoneIncoming,
  PhoneMissed,
  PhoneOff,
  PlugZap,
  Search,
  Send,
  Server,
  Settings2,
  SlidersHorizontal,
  UserPlus,
  X,
} from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Textarea } from '@/components/ui/textarea'
import {
  authLogin,
  authLogout,
  authMe,
  authRuntime,
  authSignup,
  backendHealth,
  callHistory,
  callInitiate,
  chatCreate,
  chatDeleteMessage,
  chatFetch,
  DEFAULT_BACKEND_URL,
  chatList,
  chatSendMessage,
  connectCallWebSocket,
  createWrappedCallKeyForPeer,
  decryptCallAudioFrame,
  downloadChatAttachment,
  decryptChatMessage,
  encryptCallAudioFrame,
  encryptMessageForEpoch,
  getBackendConfig,
  hasUnlockedIdentityKeys,
  invalidateChatEpochCache,
  isMessageNotificationPermissionGranted,
  isTauriRuntime,
  notifyIncomingMessage,
  primeEpochKeys,
  resolveChatEpoch,
  requestMessageNotificationPermission,
  resetConfig,
  setBackendUrl,
  unwrapCallKeyFromPeer,
  uploadChatMedia,
  type AuthRuntime,
  type BackendConfig,
  type CallHistoryEntry,
  type ChatMessage,
  type ChatSummary,
  type HealthResponse,
  type MediaAttachment,
} from '@/lib/omnis'
import { cn } from '@/lib/utils'

type ScreenState = 'boot' | 'setup' | 'auth' | 'chat' | 'settings'
type AuthMode = 'login' | 'signup'
type ViewMessage = ChatMessage & { body: string | null; attachments: MediaAttachment[] }
type AttachmentPreviewState = {
  messageId: number
  uploadId: string
  fileName: string
  mimeType: string
  objectUrl: string
}
type ChatMuteMenuState = {
  chatId: number
  username: string
  x: number
  y: number
  customMinutes: string
}
type ServerDiagnostics = {
  pingValue: string
  pingLatencyMs: number
  version: string
  versionLatencyMs: number
  checkedAt: string
}

type CallDirection = 'incoming' | 'outgoing'

type ActiveCallState = {
  callId: string
  chatId: number
  state: 'incoming' | 'outgoing' | 'ringing' | 'active' | 'ended'
  direction: CallDirection
  peerUsername: string
  statusText: string
  rttMs: number | null
}

type CallSocketFrame = {
  type: string
  status?: string
  call_id?: string
  wrapped_call_key?: string
  data?: string
  nonce?: string
  seq?: number
  is_init?: boolean
  mime?: string
}

const VERIFIED_URL_KEY = 'omnis.ui.verifiedBackendUrl'
const SAVED_CREDENTIALS_KEY = 'omnis.ui.savedCredentials'
const ALERTS_ENABLED_KEY = 'omnis.ui.alertsEnabled'
const CHAT_MUTE_UNTIL_KEY = 'omnis.ui.chatMuteUntil'
const AUTO_REFRESH_MS = 3000
const COMPOSER_MIN_HEIGHT = 40
const COMPOSER_MAX_HEIGHT = 180

type SavedCredentials = {
  username: string
  password: string
}

function readVerifiedUrl() {
  if (typeof window === 'undefined') {
    return null
  }
  return window.localStorage.getItem(VERIFIED_URL_KEY)
}

function writeVerifiedUrl(url: string) {
  if (typeof window === 'undefined') {
    return
  }
  window.localStorage.setItem(VERIFIED_URL_KEY, url)
}

function readSavedCredentials(): SavedCredentials | null {
  if (typeof window === 'undefined') {
    return null
  }

  const raw = window.localStorage.getItem(SAVED_CREDENTIALS_KEY)
  if (!raw) {
    return null
  }

  try {
    const parsed = JSON.parse(raw) as Partial<SavedCredentials>
    if (!parsed.username || !parsed.password) {
      return null
    }

    return {
      username: parsed.username,
      password: parsed.password,
    }
  } catch {
    return null
  }
}

function writeSavedCredentials(credentials: SavedCredentials) {
  if (typeof window === 'undefined') {
    return
  }

  window.localStorage.setItem(SAVED_CREDENTIALS_KEY, JSON.stringify(credentials))
}

function clearSavedCredentials() {
  if (typeof window === 'undefined') {
    return
  }

  window.localStorage.removeItem(SAVED_CREDENTIALS_KEY)
}

function readAlertsEnabled() {
  if (typeof window === 'undefined') {
    return true
  }

  const raw = window.localStorage.getItem(ALERTS_ENABLED_KEY)
  if (raw === null) {
    return true
  }

  return raw === 'true'
}

function writeAlertsEnabled(enabled: boolean) {
  if (typeof window === 'undefined') {
    return
  }

  window.localStorage.setItem(ALERTS_ENABLED_KEY, enabled ? 'true' : 'false')
}

function readMutedChats() {
  if (typeof window === 'undefined') {
    return {} as Record<string, number>
  }

  const raw = window.localStorage.getItem(CHAT_MUTE_UNTIL_KEY)
  if (!raw) {
    return {} as Record<string, number>
  }

  try {
    const parsed = JSON.parse(raw) as Record<string, number>
    if (!parsed || typeof parsed !== 'object') {
      return {} as Record<string, number>
    }

    return parsed
  } catch {
    return {} as Record<string, number>
  }
}

function writeMutedChats(value: Record<string, number>) {
  if (typeof window === 'undefined') {
    return
  }

  window.localStorage.setItem(CHAT_MUTE_UNTIL_KEY, JSON.stringify(value))
}

function formatDurationMinutes(totalMinutes: number) {
  if (totalMinutes < 60) {
    return `${totalMinutes}m`
  }

  const hours = Math.floor(totalMinutes / 60)
  const minutes = totalMinutes % 60
  if (minutes === 0) {
    return `${hours}h`
  }

  return `${hours}h ${minutes}m`
}

function pruneExpiredMutes(source: Record<string, number>, now = Date.now()) {
  const next: Record<string, number> = {}
  let changed = false

  for (const [key, value] of Object.entries(source)) {
    if (!Number.isFinite(value) || value <= now) {
      changed = true
      continue
    }
    next[key] = value
  }

  return {
    next,
    changed,
  }
}

function areChatListsEqual(previous: ChatSummary[], next: ChatSummary[]) {
  if (previous.length !== next.length) {
    return false
  }

  for (let index = 0; index < previous.length; index += 1) {
    if (previous[index].chat_id !== next[index].chat_id) {
      return false
    }
    if (previous[index].with_user !== next[index].with_user) {
      return false
    }
  }

  return true
}

function formatMessageTime(value: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return value
  }
  return date.toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  })
}

function toErrorText(error: unknown) {
  if (error instanceof Error) {
    return error.message
  }
  return String(error)
}

function formatFileSize(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return '0 B'
  }

  const units = ['B', 'KB', 'MB', 'GB']
  let value = bytes
  let unitIndex = 0

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024
    unitIndex += 1
  }

  const precision = value >= 10 || unitIndex === 0 ? 0 : 1
  return `${value.toFixed(precision)} ${units[unitIndex]}`
}

function formatCallDurationSeconds(totalSeconds: number | null | undefined) {
  if (!Number.isFinite(totalSeconds) || (totalSeconds ?? 0) <= 0) {
    return null
  }

  const seconds = Math.max(0, Math.floor(totalSeconds ?? 0))
  const minutes = Math.floor(seconds / 60)
  const remaining = seconds % 60

  if (minutes <= 0) {
    return `${remaining}s`
  }

  if (remaining === 0) {
    return `${minutes}m`
  }

  return `${minutes}m ${remaining}s`
}

function formatCallDurationClock(totalSeconds: number | null | undefined) {
  if (!Number.isFinite(totalSeconds) || (totalSeconds ?? 0) <= 0) {
    return null
  }

  const seconds = Math.max(0, Math.floor(totalSeconds ?? 0))
  const minutes = Math.floor(seconds / 60)
  const remaining = String(seconds % 60).padStart(2, '0')
  return `${minutes}:${remaining}`
}

function formatCallSystemBody(message: ChatMessage, peerUsername: string | null, currentUserId: number | null) {
  const initiatorLabel = currentUserId !== null && message.sender_id === currentUserId ? 'You' : (peerUsername ?? 'Contact')
  const duration = formatCallDurationClock(message.duration_seconds)

  switch (message.call_status) {
    case 'missed':
      return 'Missed call'
    case 'rejected':
      return 'Call declined'
    case 'ended':
      return duration ? `Call ended · ${duration}` : 'Call ended'
    case 'accepted':
      return duration ? `Call completed · ${duration}` : 'Call completed'
    case 'ringing':
      return `${initiatorLabel} started a call`
    default:
      return 'Call event'
  }
}

function attachmentFileName(attachment: MediaAttachment) {
  const fallback = attachment.upload_id || 'attachment'
  const mimeType = (attachment.mime_type || '').toLowerCase()

  if (!mimeType.includes('/')) {
    return fallback
  }

  const rawExtension = mimeType.split('/')[1]?.split(';')[0]?.trim()
  if (!rawExtension) {
    return fallback
  }

  let extension = rawExtension.toLowerCase()
  if (extension === 'svg+xml') {
    extension = 'svg'
  } else if (extension.includes('+')) {
    extension = extension.split('+')[0]
  }

  if (extension === 'jpeg') {
    extension = 'jpg'
  }

  if (!extension) {
    return fallback
  }

  return `${fallback}.${extension.replace(/[^a-z0-9.-]/gi, '')}`
}

function getInitials(username: string) {
  return username.slice(0, 2).toUpperCase()
}

const AVATAR_CLASSES = [
  'avatar-amber',
  'avatar-sky',
  'avatar-violet',
  'avatar-emerald',
  'avatar-rose',
  'avatar-teal',
  'avatar-orange',
]

function usernameColorClass(username: string): string {
  let hash = 0
  for (let i = 0; i < username.length; i++) {
    hash = ((hash << 5) - hash + username.charCodeAt(i)) | 0
  }
  return AVATAR_CLASSES[Math.abs(hash) % AVATAR_CLASSES.length]
}

function PendingAttachmentChip({
  file,
  onRemove,
}: {
  file: File
  onRemove: () => void
}) {
  const [previewUrl, setPreviewUrl] = useState<string | null>(null)
  const isImage = file.type.startsWith('image/')

  useEffect(() => {
    if (!isImage) {
      setPreviewUrl(null)
      return
    }

    const objectUrl = URL.createObjectURL(file)
    setPreviewUrl(objectUrl)

    return () => {
      URL.revokeObjectURL(objectUrl)
    }
  }, [file, isImage])

  return (
    <div className="flex items-center gap-2.5 rounded-2xl border border-border/40 bg-secondary/70 px-2.5 py-2 animate-fade-in">
      {previewUrl ? (
        <img src={previewUrl} alt={file.name} className="h-10 w-10 rounded-xl object-cover shrink-0" />
      ) : (
        <div className="h-10 w-10 rounded-xl bg-accent/60 flex items-center justify-center shrink-0">
          <Paperclip className="h-4 w-4 text-muted-foreground" />
        </div>
      )}
      <div className="min-w-0 flex-1">
        <p className="max-w-[150px] truncate text-sm font-medium text-foreground">{file.name}</p>
        <p className="font-mono text-[11px] text-muted-foreground mt-0.5">{formatFileSize(file.size)}</p>
      </div>
      <button
        type="button"
        className="h-7 w-7 flex items-center justify-center rounded-full text-muted-foreground hover:text-foreground hover:bg-accent/60 transition-all shrink-0"
        onClick={onRemove}
        aria-label="Remove"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  )
}

export function OmnisDashboard() {
  const [screen, setScreen] = useState<ScreenState>('boot')
  const [status, setStatus] = useState('Booting Omnis...')
  const [busy, setBusy] = useState(false)
  const [desktopRuntime, setDesktopRuntime] = useState(false)

  const [config, setConfig] = useState<BackendConfig | null>(null)
  const [runtime, setRuntime] = useState<AuthRuntime | null>(null)
  const [health, setHealth] = useState<HealthResponse | null>(null)
  const [currentUser, setCurrentUser] = useState<{ id: number; username: string } | null>(null)

  const [setupUrl, setSetupUrl] = useState('http://127.0.0.1:6767')

  const [authMode, setAuthMode] = useState<AuthMode>('login')
  const [loginUsername, setLoginUsername] = useState('')
  const [loginPassword, setLoginPassword] = useState('')
  const [signupUsername, setSignupUsername] = useState('')
  const [signupPassword, setSignupPassword] = useState('')
  const [signupConfirm, setSignupConfirm] = useState('')

  const [chats, setChats] = useState<ChatSummary[]>([])
  const [chatSearch, setChatSearch] = useState('')
  const [selectedChatId, setSelectedChatId] = useState<number | null>(null)
  const [messages, setMessages] = useState<ViewMessage[]>([])
  const [newChatUsername, setNewChatUsername] = useState('')
  const [draftMessage, setDraftMessage] = useState('')
  const [pendingAttachments, setPendingAttachments] = useState<File[]>([])
  const [replyTarget, setReplyTarget] = useState<ViewMessage | null>(null)
  const [expandedMessages, setExpandedMessages] = useState<Set<number>>(new Set())
  const [openMenuMessageId, setOpenMenuMessageId] = useState<number | null>(null)
  const [attachmentDownloads, setAttachmentDownloads] = useState<Record<string, boolean>>({})
  const [attachmentPreviewLoading, setAttachmentPreviewLoading] = useState<Record<string, boolean>>({})
  const [attachmentPreview, setAttachmentPreview] = useState<AttachmentPreviewState | null>(null)
  const [unreadCount, setUnreadCount] = useState(0)
  const [bannerUnreadCount, setBannerUnreadCount] = useState(0)
  const [showUnreadBanner, setShowUnreadBanner] = useState(false)
  const [behindMessages, setBehindMessages] = useState(0)
  const [notificationsAllowed, setNotificationsAllowed] = useState<boolean | null>(null)
  const [alertsEnabled, setAlertsEnabled] = useState(() => readAlertsEnabled())
  const [mutedChats, setMutedChats] = useState<Record<string, number>>(() => readMutedChats())
  const [chatMuteMenu, setChatMuteMenu] = useState<ChatMuteMenuState | null>(null)
  const [serverDiagnostics, setServerDiagnostics] = useState<ServerDiagnostics | null>(null)
  const [activeCall, setActiveCall] = useState<ActiveCallState | null>(null)

  const lastSeenMessageIdByChatRef = useRef(new Map<number, number>())
  const loadedChatMessagesRef = useRef(new Set<number>())
  const pollInFlightRef = useRef(false)
  const notificationBaselineReadyRef = useRef(false)
  const selectedChatIdRef = useRef<number | null>(null)
  const currentUserRef = useRef<{ id: number; username: string } | null>(null)
  const busyRef = useRef(false)
  const messageInputRef = useRef<HTMLTextAreaElement | null>(null)
  const attachmentInputRef = useRef<HTMLInputElement | null>(null)
  const messagesRef = useRef<ViewMessage[]>([])
  const messageListRef = useRef<HTMLDivElement | null>(null)
  const forceScrollToBottomRef = useRef(false)
  const hiddenUnreadRef = useRef(0)
  const wasHiddenRef = useRef(false)
  const unreadBannerTimerRef = useRef<number | null>(null)
  const alertsEnabledRef = useRef(alertsEnabled)
  const mutedChatsRef = useRef(mutedChats)
  const activeCallRef = useRef<ActiveCallState | null>(null)
  const callSocketRef = useRef<WebSocket | null>(null)
  const callOfferSentRef = useRef(false)
  const callPingTimerRef = useRef<number | null>(null)
  const callLastPingAtRef = useRef<number | null>(null)
  const callDiscoverySeenRef = useRef(new Set<string>())
  const callClosingRef = useRef(false)
  const callAcceptedRef = useRef(false)
  const callOfferReceivedRef = useRef(false)
  const callAnswerSentRef = useRef(false)
  const callKeyRef = useRef<CryptoKey | null>(null)
  const callAudioMimeTypeRef = useRef('audio/webm;codecs=opus')
  const callAudioSeqRef = useRef(0)
  const callFirstAudioChunkRef = useRef(true)
  const callAudioCaptureStartingRef = useRef(false)
  const callAudioStreamRef = useRef<MediaStream | null>(null)
  const callMediaRecorderRef = useRef<MediaRecorder | null>(null)
  const callPlaybackElementRef = useRef<HTMLAudioElement | null>(null)
  const callPlaybackObjectUrlRef = useRef<string | null>(null)
  const callMediaSourceRef = useRef<MediaSource | null>(null)
  const callSourceBufferRef = useRef<SourceBuffer | null>(null)
  const callSourceBufferReadyRef = useRef(false)
  const callPendingAudioBuffersRef = useRef<ArrayBuffer[]>([])
  const callQueuedFramesRef = useRef<string[]>([])
  const callCloseAfterFlushRef = useRef(false)

  const canLogin = useMemo(
    () => loginUsername.trim().length >= 5 && loginPassword.trim().length >= 6,
    [loginPassword, loginUsername],
  )

  const canSignup = useMemo(
    () =>
      signupUsername.trim().length >= 5 &&
      signupPassword.trim().length >= 6 &&
      signupPassword === signupConfirm,
    [signupConfirm, signupPassword, signupUsername],
  )

  const visibleChats = useMemo(() => {
    const query = chatSearch.trim().toLowerCase()
    if (!query) {
      return chats
    }
    return chats.filter((chat) => chat.with_user.toLowerCase().includes(query))
  }, [chatSearch, chats])

  const activeChat = useMemo(
    () => chats.find((chat) => chat.chat_id === selectedChatId) ?? null,
    [chats, selectedChatId],
  )

  const messageById = useMemo(() => {
    const lookup = new Map<number, ViewMessage>()
    for (const message of messages) {
      lookup.set(message.id, message)
    }
    return lookup
  }, [messages])

  const activeMutedChats = useMemo(() => {
    const now = Date.now()
    const chatLookup = new Map(chats.map((chat) => [String(chat.chat_id), chat.with_user]))

    return Object.entries(mutedChats)
      .filter(([, until]) => Number.isFinite(until) && until > now)
      .map(([chatId, until]) => {
        const remainingMinutes = Math.max(1, Math.ceil((until - now) / 60000))
        return {
          chatId: Number(chatId),
          username: chatLookup.get(chatId) || `Chat #${chatId}`,
          remainingMinutes,
        }
      })
      .sort((left, right) => left.remainingMinutes - right.remainingMinutes)
  }, [chats, mutedChats])

  const activePeerLabel = activeChat?.with_user ?? 'Contact'

  useEffect(() => {
    selectedChatIdRef.current = selectedChatId
    setReplyTarget(null)
    setPendingAttachments([])
    setAttachmentDownloads({})
    setAttachmentPreviewLoading({})
    setUnreadCount(0)
    setBannerUnreadCount(0)
    setShowUnreadBanner(false)
    setBehindMessages(0)
    hiddenUnreadRef.current = 0
    forceScrollToBottomRef.current = true
    if (attachmentInputRef.current) {
      attachmentInputRef.current.value = ''
    }
    if (attachmentPreview) {
      URL.revokeObjectURL(attachmentPreview.objectUrl)
      setAttachmentPreview(null)
    }
  }, [selectedChatId])

  useEffect(() => {
    currentUserRef.current = currentUser
  }, [currentUser])

  useEffect(() => {
    busyRef.current = busy
  }, [busy])

  useEffect(() => {
    messagesRef.current = messages
  }, [messages])

  useEffect(() => {
    alertsEnabledRef.current = alertsEnabled
    writeAlertsEnabled(alertsEnabled)
  }, [alertsEnabled])

  useEffect(() => {
    mutedChatsRef.current = mutedChats
    writeMutedChats(mutedChats)
  }, [mutedChats])

  useEffect(() => {
    activeCallRef.current = activeCall
  }, [activeCall])

  useEffect(() => {
    const intervalId = window.setInterval(() => {
      setMutedChats((previous) => {
        const { next, changed } = pruneExpiredMutes(previous)
        return changed ? next : previous
      })
    }, 30000)

    return () => {
      window.clearInterval(intervalId)
    }
  }, [])

  useEffect(() => {
    if (selectedChatId === null) {
      return
    }

    const frameId = window.requestAnimationFrame(() => {
      if (forceScrollToBottomRef.current) {
        forceScrollToBottomRef.current = false
        scrollMessagesToBottom(messages.length > 0 ? 'smooth' : 'auto')
        return
      }

      refreshBehindCount()
    })

    return () => {
      window.cancelAnimationFrame(frameId)
    }
  }, [messages, selectedChatId])

  useEffect(() => {
    if (screen !== 'chat' && chatMuteMenu) {
      setChatMuteMenu(null)
    }
  }, [chatMuteMenu, screen])

  useEffect(() => {
    if (typeof document === 'undefined') {
      return
    }

    wasHiddenRef.current = document.hidden

    function onVisibilityChange() {
      const hidden = document.hidden
      const wasHidden = wasHiddenRef.current
      wasHiddenRef.current = hidden

      if (!hidden && wasHidden && hiddenUnreadRef.current > 0) {
        const recoveredUnread = hiddenUnreadRef.current
        hiddenUnreadRef.current = 0
        setUnreadCount(0)
        setBannerUnreadCount(recoveredUnread)
        setShowUnreadBanner(true)
        forceScrollToBottomRef.current = true

        if (unreadBannerTimerRef.current !== null) {
          window.clearTimeout(unreadBannerTimerRef.current)
        }
        unreadBannerTimerRef.current = window.setTimeout(() => {
          setShowUnreadBanner(false)
          unreadBannerTimerRef.current = null
        }, 5000)
      }
    }

    document.addEventListener('visibilitychange', onVisibilityChange)

    return () => {
      document.removeEventListener('visibilitychange', onVisibilityChange)
      if (unreadBannerTimerRef.current !== null) {
        window.clearTimeout(unreadBannerTimerRef.current)
        unreadBannerTimerRef.current = null
      }
    }
  }, [])

  useEffect(() => {
    if (!attachmentPreview) {
      return
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== 'Escape') {
        return
      }

      setAttachmentPreview((current) => {
        if (!current) {
          return current
        }

        URL.revokeObjectURL(current.objectUrl)
        return null
      })
    }

    window.addEventListener('keydown', onKeyDown)
    return () => {
      window.removeEventListener('keydown', onKeyDown)
    }
  }, [attachmentPreview])

  useEffect(() => {
    return () => {
      closeCallSocket()
      updateCallState(null)
      if (callPlaybackElementRef.current) {
        callPlaybackElementRef.current.remove()
        callPlaybackElementRef.current = null
      }
    }
  }, [])

  function senderLabel(senderId: number) {
    if (currentUser !== null && senderId === currentUser.id) {
      return 'You'
    }
    return activePeerLabel
  }

  function previewText(text: string | null, limit = 120) {
    if (!text) {
      return '[Empty message]'
    }
    if (text.length <= limit) {
      return text
    }
    return `${text.slice(0, limit - 3)}...`
  }

  function attachmentDownloadKey(messageId: number, uploadId: string) {
    return `${messageId}:${uploadId}`
  }

  function closeAttachmentPreview() {
    if (!attachmentPreview) {
      return
    }

    URL.revokeObjectURL(attachmentPreview.objectUrl)
    setAttachmentPreview(null)
  }

  function muteRemainingMs(chatId: number) {
    const until = mutedChatsRef.current[String(chatId)]
    if (!until || !Number.isFinite(until)) {
      return 0
    }

    return Math.max(0, until - Date.now())
  }

  function isChatMuted(chatId: number) {
    return muteRemainingMs(chatId) > 0
  }

  function applyMute(chatId: number, minutes: number) {
    const safeMinutes = Math.max(1, Math.floor(minutes))
    const until = Date.now() + safeMinutes * 60 * 1000
    setMutedChats((previous) => ({
      ...previous,
      [String(chatId)]: until,
    }))
  }

  function clearMute(chatId: number) {
    setMutedChats((previous) => {
      const key = String(chatId)
      if (!(key in previous)) {
        return previous
      }

      const next = { ...previous }
      delete next[key]
      return next
    })
  }

  function muteLabel(chatId: number) {
    const remainingMs = muteRemainingMs(chatId)
    if (remainingMs <= 0) {
      return null
    }

    return formatDurationMinutes(Math.max(1, Math.ceil(remainingMs / 60000)))
  }

  function computeBehindCount() {
    const container = messageListRef.current
    if (!container) {
      return 0
    }

    const messageNodes = container.querySelectorAll<HTMLElement>('[data-message-index]')
    if (messageNodes.length === 0) {
      return 0
    }

    const containerRect = container.getBoundingClientRect()
    let lastVisibleIndex = -1

    messageNodes.forEach((node) => {
      const index = Number(node.dataset.messageIndex ?? '-1')
      if (index < 0) {
        return
      }

      const nodeRect = node.getBoundingClientRect()
      if (nodeRect.top < containerRect.bottom - 8) {
        lastVisibleIndex = Math.max(lastVisibleIndex, index)
      }
    })

    if (lastVisibleIndex < 0) {
      return messageNodes.length
    }

    return Math.max(0, messageNodes.length - 1 - lastVisibleIndex)
  }

  function scrollMessagesToBottom(behavior: ScrollBehavior = 'smooth') {
    const container = messageListRef.current
    if (!container) {
      return
    }

    container.scrollTo({ top: container.scrollHeight, behavior })
    setUnreadCount(0)
    setBehindMessages(0)
  }

  function scrollToFirstUnread() {
    const container = messageListRef.current
    if (!container) return
    if (unreadCount > 0) {
      const messageNodes = container.querySelectorAll('[data-message-index]')
      const targetIndex = messagesRef.current.length - unreadCount
      const targetNode = messageNodes[targetIndex] as HTMLElement | undefined
      if (targetNode) {
        targetNode.scrollIntoView({ behavior: 'smooth', block: 'start' })
        return
      }
    }
    scrollMessagesToBottom('smooth')
  }

  function refreshBehindCount() {
    const behind = computeBehindCount()
    setBehindMessages(behind)
    if (behind <= 0) {
      setUnreadCount(0)
    }
    return behind
  }

  function resizeComposerInput(target?: HTMLTextAreaElement | null) {
    const input = target ?? messageInputRef.current
    if (!input) {
      return
    }

    input.style.height = 'auto'
    const nextHeight = Math.max(COMPOSER_MIN_HEIGHT, Math.min(input.scrollHeight, COMPOSER_MAX_HEIGHT))
    input.style.height = `${nextHeight}px`
    input.style.overflowY = input.scrollHeight > COMPOSER_MAX_HEIGHT ? 'auto' : 'hidden'
  }

  function handleIncomingMessages(incomingCount: number) {
    if (incomingCount <= 0) {
      return
    }

    if (typeof document !== 'undefined' && document.hidden) {
      hiddenUnreadRef.current += incomingCount
      setUnreadCount((previous) => previous + incomingCount)
      return
    }

    const behind = refreshBehindCount()
    if (behind <= 10) {
      forceScrollToBottomRef.current = true
      return
    }

    setUnreadCount((previous) => previous + incomingCount)
  }

  function handleAttachmentSelection(event: React.ChangeEvent<HTMLInputElement>) {
    const selectedFiles = Array.from(event.target.files ?? [])
    if (selectedFiles.length === 0) {
      return
    }

    setPendingAttachments((previous) => [...previous, ...selectedFiles])
    setStatus(`${selectedFiles.length} file${selectedFiles.length === 1 ? '' : 's'} attached.`)
    event.target.value = ''
  }

  function removePendingAttachment(index: number) {
    setPendingAttachments((previous) => previous.filter((_, current) => current !== index))
  }

  async function handleDownloadAttachment(message: ViewMessage, attachment: MediaAttachment) {
    if (selectedChatId === null || activeChat === null) {
      return
    }
    if (typeof message.epoch_id !== 'number') {
      setStatus('Attachment cannot be decrypted because this message has no epoch key.')
      return
    }

    const key = attachmentDownloadKey(message.id, attachment.upload_id)
    setAttachmentDownloads((previous) => ({ ...previous, [key]: true }))

    try {
      const blob = await downloadChatAttachment(
        selectedChatId,
        activeChat.with_user,
        message.epoch_id,
        attachment,
      )
      const objectUrl = URL.createObjectURL(blob)
      const anchor = document.createElement('a')
      anchor.href = objectUrl
      anchor.download = attachmentFileName(attachment)
      document.body.appendChild(anchor)
      anchor.click()
      anchor.remove()
      window.setTimeout(() => URL.revokeObjectURL(objectUrl), 1000)
      setStatus(`Downloaded ${attachmentFileName(attachment)}.`)
    } catch (error) {
      setStatus(`Failed to download attachment: ${toErrorText(error)}`)
    } finally {
      setAttachmentDownloads((previous) => {
        const next = { ...previous }
        delete next[key]
        return next
      })
    }
  }

  async function handlePreviewAttachment(message: ViewMessage, attachment: MediaAttachment) {
    if (selectedChatId === null || activeChat === null) {
      return
    }
    if (typeof message.epoch_id !== 'number') {
      setStatus('Attachment cannot be previewed because this message has no epoch key.')
      return
    }

    const key = attachmentDownloadKey(message.id, attachment.upload_id)
    setAttachmentPreviewLoading((previous) => ({ ...previous, [key]: true }))

    try {
      const blob = await downloadChatAttachment(
        selectedChatId,
        activeChat.with_user,
        message.epoch_id,
        attachment,
      )
      const objectUrl = URL.createObjectURL(blob)
      const fileName = attachmentFileName(attachment)

      setAttachmentPreview((previous) => {
        if (previous) {
          URL.revokeObjectURL(previous.objectUrl)
        }

        return {
          messageId: message.id,
          uploadId: attachment.upload_id,
          fileName,
          mimeType: attachment.mime_type || blob.type || 'application/octet-stream',
          objectUrl,
        }
      })
      setStatus(`Previewing ${fileName}.`)
    } catch (error) {
      setStatus(`Failed to preview attachment: ${toErrorText(error)}`)
    } finally {
      setAttachmentPreviewLoading((previous) => {
        const next = { ...previous }
        delete next[key]
        return next
      })
    }
  }

  async function handleDeleteMessage(message: ViewMessage) {
    if (selectedChatId === null) return
    setOpenMenuMessageId(null)
    try {
      await chatDeleteMessage(selectedChatId, message.id)
      setMessages((prev) =>
        prev.map((m) => (m.id === message.id ? { ...m, deleted: true, body: null } : m)),
      )
      messagesRef.current = messagesRef.current.map((m) =>
        m.id === message.id ? { ...m, deleted: true, body: null } : m,
      )
    } catch (error) {
      setStatus(`Failed to delete message: ${toErrorText(error)}`)
    }
  }

  function stopCallPingLoop() {
    if (callPingTimerRef.current !== null) {
      window.clearInterval(callPingTimerRef.current)
      callPingTimerRef.current = null
    }
    callLastPingAtRef.current = null
  }

  function resetCallSessionRefs() {
    callOfferSentRef.current = false
    callOfferReceivedRef.current = false
    callAcceptedRef.current = false
    callAnswerSentRef.current = false
    callKeyRef.current = null
    callAudioSeqRef.current = 0
    callFirstAudioChunkRef.current = true
    callAudioCaptureStartingRef.current = false
    callQueuedFramesRef.current = []
    callCloseAfterFlushRef.current = false
  }

  function getSupportedCallAudioMimeType() {
    if (typeof MediaRecorder === 'undefined') {
      return ''
    }

    const candidates = [
      'audio/webm;codecs=opus',
      'audio/webm',
      'audio/ogg;codecs=opus',
      'audio/ogg',
    ]

    for (const candidate of candidates) {
      try {
        if (MediaRecorder.isTypeSupported(candidate)) {
          return candidate
        }
      } catch {
        // ignore and try the next mime type
      }
    }

    return ''
  }

  function stopCallAudioCapture() {
    const recorder = callMediaRecorderRef.current
    if (recorder && recorder.state !== 'inactive') {
      try {
        recorder.stop()
      } catch {
        // no-op
      }
    }
    callMediaRecorderRef.current = null

    const stream = callAudioStreamRef.current
    if (stream) {
      for (const track of stream.getTracks()) {
        track.stop()
      }
    }
    callAudioStreamRef.current = null
  }

  function stopCallAudioPlayback() {
    const mediaSource = callMediaSourceRef.current
    if (mediaSource && mediaSource.readyState === 'open') {
      try {
        mediaSource.endOfStream()
      } catch {
        // no-op
      }
    }
    callMediaSourceRef.current = null
    callSourceBufferRef.current = null
    callSourceBufferReadyRef.current = false
    callPendingAudioBuffersRef.current = []

    const playbackElement = callPlaybackElementRef.current
    if (playbackElement) {
      playbackElement.pause()
      playbackElement.removeAttribute('src')
      playbackElement.load()
    }

    if (callPlaybackObjectUrlRef.current) {
      URL.revokeObjectURL(callPlaybackObjectUrlRef.current)
      callPlaybackObjectUrlRef.current = null
    }
  }

  function stopCallAudio() {
    stopCallAudioCapture()
    stopCallAudioPlayback()
  }

  function flushCallPlaybackQueue() {
    const sourceBuffer = callSourceBufferRef.current
    if (!sourceBuffer || sourceBuffer.updating || !callSourceBufferReadyRef.current) {
      return
    }

    const nextBuffer = callPendingAudioBuffersRef.current.shift()
    if (!nextBuffer) {
      return
    }

    try {
      sourceBuffer.appendBuffer(nextBuffer)
      callSourceBufferReadyRef.current = false
    } catch {
      callPendingAudioBuffersRef.current.unshift(nextBuffer)
    }
  }

  function queueCallPlaybackBuffer(buffer: ArrayBuffer) {
    callPendingAudioBuffersRef.current.push(buffer)
    flushCallPlaybackQueue()
  }

  function initCallPlayback(mimeType: string) {
    if (typeof window === 'undefined' || typeof MediaSource === 'undefined') {
      return
    }

    let resolvedMimeType = mimeType
    if (!MediaSource.isTypeSupported(resolvedMimeType)) {
      if (resolvedMimeType.includes('webm') && MediaSource.isTypeSupported('audio/webm')) {
        resolvedMimeType = 'audio/webm'
      } else if (MediaSource.isTypeSupported('audio/ogg;codecs=opus')) {
        resolvedMimeType = 'audio/ogg;codecs=opus'
      } else {
        return
      }
    }

    stopCallAudioPlayback()

    if (!callPlaybackElementRef.current) {
      const audioElement = document.createElement('audio')
      audioElement.autoplay = true
      audioElement.setAttribute('playsinline', 'true')
      audioElement.preload = 'none'
      audioElement.style.display = 'none'
      document.body.appendChild(audioElement)
      callPlaybackElementRef.current = audioElement
    }

    const playbackElement = callPlaybackElementRef.current
    if (!playbackElement) {
      return
    }

    const mediaSource = new MediaSource()
    callMediaSourceRef.current = mediaSource

    const objectUrl = URL.createObjectURL(mediaSource)
    callPlaybackObjectUrlRef.current = objectUrl
    playbackElement.src = objectUrl

    void playbackElement.play().catch(() => {
      // autoplay may be blocked in some runtimes
    })

    mediaSource.addEventListener(
      'sourceopen',
      () => {
        try {
          const sourceBuffer = mediaSource.addSourceBuffer(resolvedMimeType)
          sourceBuffer.mode = 'sequence'
          callSourceBufferRef.current = sourceBuffer
          callSourceBufferReadyRef.current = true

          sourceBuffer.addEventListener('updateend', () => {
            callSourceBufferReadyRef.current = true
            flushCallPlaybackQueue()
          })

          flushCallPlaybackQueue()
        } catch {
          callSourceBufferRef.current = null
          callSourceBufferReadyRef.current = false
        }
      },
      { once: true },
    )
  }

  async function startCallAudioCapture() {
    if (callMediaRecorderRef.current && callMediaRecorderRef.current.state !== 'inactive') {
      return
    }
    if (callAudioCaptureStartingRef.current) {
      return
    }
    if (!callKeyRef.current) {
      return
    }
    if (typeof navigator === 'undefined' || !navigator.mediaDevices?.getUserMedia) {
      throw new Error('Audio capture is not available in this runtime')
    }

    callAudioCaptureStartingRef.current = true
    let stream: MediaStream
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true })
    } catch (error) {
      callAudioCaptureStartingRef.current = false
      throw error
    }
    callAudioStreamRef.current = stream

    const mimeType = getSupportedCallAudioMimeType()
    callAudioMimeTypeRef.current = mimeType || 'audio/webm'
    callAudioSeqRef.current = 0
    callFirstAudioChunkRef.current = true

    let recorder: MediaRecorder
    try {
      recorder = mimeType
        ? new MediaRecorder(stream, { mimeType, audioBitsPerSecond: 64000 })
        : new MediaRecorder(stream, { audioBitsPerSecond: 64000 })
    } catch {
      recorder = mimeType ? new MediaRecorder(stream, { mimeType }) : new MediaRecorder(stream)
    }

    callMediaRecorderRef.current = recorder

    recorder.ondataavailable = async (event) => {
      if (event.data.size <= 0) {
        return
      }

      const socket = callSocketRef.current
      if (!socket || socket.readyState !== WebSocket.OPEN || !callKeyRef.current) {
        return
      }

      try {
        const chunk = await event.data.arrayBuffer()
        const encrypted = await encryptCallAudioFrame(callKeyRef.current, chunk)

        const frame: Record<string, unknown> = {
          type: 'audio_frame',
          data: encrypted.data,
          nonce: encrypted.nonce,
          seq: callAudioSeqRef.current,
        }
        callAudioSeqRef.current += 1

        if (callFirstAudioChunkRef.current) {
          frame.is_init = true
          frame.mime = callAudioMimeTypeRef.current
        }

        const payload = JSON.stringify(frame)
        if (payload.length <= 4096) {
          socket.send(payload)
          if (callFirstAudioChunkRef.current) {
            callFirstAudioChunkRef.current = false
          }
        }
      } catch {
        // keep the call alive even if a single audio frame fails
      }
    }

    recorder.onstop = () => {
      if (callAudioStreamRef.current === stream) {
        for (const track of stream.getTracks()) {
          track.stop()
        }
        callAudioStreamRef.current = null
      }
      if (callMediaRecorderRef.current === recorder) {
        callMediaRecorderRef.current = null
      }
    }

    recorder.start(100)
    callAudioCaptureStartingRef.current = false
  }

  async function handleIncomingCallAudioFrame(frame: CallSocketFrame) {
    if (!callKeyRef.current || typeof frame.data !== 'string' || typeof frame.nonce !== 'string') {
      return
    }

    try {
      const decrypted = await decryptCallAudioFrame(callKeyRef.current, frame.data, frame.nonce)

      if (frame.is_init) {
        const mimeType = typeof frame.mime === 'string' ? frame.mime : callAudioMimeTypeRef.current
        callAudioMimeTypeRef.current = mimeType
        if (!callMediaSourceRef.current) {
          initCallPlayback(mimeType)
        }
      }

      queueCallPlaybackBuffer(decrypted)
    } catch {
      // ignore malformed audio frames
    }
  }

  function sendCallFrame(frame: Record<string, unknown>) {
    const payload = JSON.stringify(frame)
    const socket = callSocketRef.current
    if (socket && socket.readyState === WebSocket.OPEN) {
      socket.send(payload)
      return true
    }

    callQueuedFramesRef.current.push(payload)
    return false
  }

  function flushQueuedCallFrames() {
    const socket = callSocketRef.current
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      return
    }

    if (callQueuedFramesRef.current.length > 0) {
      const queued = [...callQueuedFramesRef.current]
      callQueuedFramesRef.current = []

      for (let index = 0; index < queued.length; index += 1) {
        const payload = queued[index]
        try {
          socket.send(payload)
        } catch {
          callQueuedFramesRef.current = queued.slice(index)
          return
        }
      }
    }

    if (callCloseAfterFlushRef.current) {
      callCloseAfterFlushRef.current = false
      closeCallSocket()
    }
  }

  function closeCallSocket() {
    stopCallPingLoop()

    stopCallAudio()

    if (!callSocketRef.current) {
      resetCallSessionRefs()
      return
    }

    const socket = callSocketRef.current
    callSocketRef.current = null
    callClosingRef.current = true
    socket.onopen = null
    socket.onmessage = null
    socket.onerror = null
    socket.onclose = null

    if (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING) {
      try {
        socket.close(1000, 'Call closed')
      } catch {
        // no-op
      }
    }

    callClosingRef.current = false
    resetCallSessionRefs()
  }

  function updateCallState(next: ActiveCallState | null) {
    setActiveCall(next)
    activeCallRef.current = next
  }

  function describeCallStatus(status: string) {
    switch (status) {
      case 'initiated':
        return 'Calling...'
      case 'ringing':
        return 'Ringing...'
      case 'accepted':
        return 'Call active'
      case 'rejected':
        return 'Call rejected'
      case 'missed':
        return 'Missed call'
      case 'ended':
        return 'Call ended'
      default:
        return `Call status: ${status}`
    }
  }

  async function ensureCallMicPermission() {
    if (typeof navigator === 'undefined' || !navigator.mediaDevices?.getUserMedia) {
      throw new Error('Audio capture is not available in this runtime')
    }

    const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
    for (const track of stream.getTracks()) {
      track.stop()
    }
  }

  async function maybeSendCallAnswer() {
    const socket = callSocketRef.current
    const current = activeCallRef.current
    if (!socket || socket.readyState !== WebSocket.OPEN || !current) {
      return
    }
    if (current.direction !== 'incoming') {
      return
    }
    if (!callAcceptedRef.current || !callOfferReceivedRef.current || !callKeyRef.current) {
      return
    }
    if (callAnswerSentRef.current) {
      return
    }

    socket.send(JSON.stringify({ type: 'answer', sdp: '' }))
    callAnswerSentRef.current = true
    updateCallState({
      ...current,
      state: 'active',
      statusText: 'Call active',
    })

    try {
      await startCallAudioCapture()
    } catch (error) {
      setStatus(`Microphone error: ${toErrorText(error)}`)
      sendCallFrame({ type: 'end' })
      updateCallState({ ...current, state: 'ended', statusText: 'Call setup failed' })
      closeCallSocket()
    }
  }

  async function openCallSocket(callId: string, chatId: number, direction: CallDirection, peerUsername: string) {
    closeCallSocket()
    resetCallSessionRefs()
    callAcceptedRef.current = direction === 'outgoing'

    const socket = await connectCallWebSocket(callId)
    callSocketRef.current = socket
    updateCallState({
      callId,
      chatId,
      direction,
      peerUsername,
      state: direction === 'incoming' ? 'incoming' : 'outgoing',
      statusText: direction === 'incoming' ? 'Incoming call' : 'Connecting call...',
      rttMs: null,
    })

    socket.onopen = () => {
      const current = activeCallRef.current
      if (!current || current.callId !== callId) {
        return
      }

      if (current.state !== 'ended') {
        updateCallState({
          ...current,
          statusText: current.direction === 'incoming' ? 'Incoming call' : 'Calling...',
        })
      }

      flushQueuedCallFrames()

      if (!callSocketRef.current || callSocketRef.current !== socket || socket.readyState !== WebSocket.OPEN) {
        return
      }

      void maybeSendCallAnswer()

      stopCallPingLoop()
      callPingTimerRef.current = window.setInterval(() => {
        if (!callSocketRef.current || callSocketRef.current.readyState !== WebSocket.OPEN) {
          return
        }
        callLastPingAtRef.current = Date.now()
        callSocketRef.current.send(JSON.stringify({ type: 'ping' }))
      }, 3000)
    }

    socket.onmessage = async (event) => {
      let frame: CallSocketFrame | null = null
      try {
        frame = JSON.parse(event.data) as CallSocketFrame
      } catch {
        return
      }

      const current = activeCallRef.current
      if (!current || current.callId !== callId) {
        return
      }

      if (frame.type === 'pong') {
        if (callLastPingAtRef.current !== null) {
          const measured = Math.max(0, Date.now() - callLastPingAtRef.current)
          updateCallState({ ...current, rttMs: measured })
        }
        return
      }

      if (frame.type === 'call_state' && typeof frame.status === 'string') {
        const status = frame.status
        if (status === 'ringing' && current.direction === 'outgoing' && !callOfferSentRef.current) {
          try {
            const keyMaterial = await createWrappedCallKeyForPeer(current.peerUsername)
            callKeyRef.current = keyMaterial.callKey
            callOfferSentRef.current = true
            sendCallFrame({ type: 'offer', sdp: '', wrapped_call_key: keyMaterial.wrappedCallKey })
          } catch (error) {
            setStatus(`Failed to establish call key: ${toErrorText(error)}`)
            sendCallFrame({ type: 'end' })
            updateCallState({ ...current, state: 'ended', statusText: 'Call setup failed' })
            closeCallSocket()
            return
          }
        }

        if (status === 'accepted') {
          updateCallState({ ...current, state: 'active', statusText: describeCallStatus(status) })
          if (current.direction === 'outgoing') {
            try {
              await startCallAudioCapture()
            } catch (error) {
              setStatus(`Microphone error: ${toErrorText(error)}`)
            }
          }
          return
        }

        if (status === 'rejected' || status === 'missed' || status === 'ended') {
          updateCallState({ ...current, state: 'ended', statusText: describeCallStatus(status) })
          closeCallSocket()
          return
        }

        updateCallState({
          ...current,
          state: status === 'ringing' ? (current.direction === 'incoming' ? 'incoming' : 'ringing') : current.state,
          statusText: describeCallStatus(status),
        })
        return
      }

      if (frame.type === 'offer' && current.direction === 'incoming') {
        if (typeof frame.wrapped_call_key !== 'string' || frame.wrapped_call_key.length === 0) {
          updateCallState({ ...current, state: 'ended', statusText: 'Invalid call offer' })
          closeCallSocket()
          return
        }

        try {
          callKeyRef.current = await unwrapCallKeyFromPeer(current.peerUsername, frame.wrapped_call_key)
        } catch (error) {
          setStatus(`Failed to accept call key: ${toErrorText(error)}`)
          sendCallFrame({ type: 'end' })
          updateCallState({ ...current, state: 'ended', statusText: 'Call setup failed' })
          closeCallSocket()
          return
        }

        callOfferReceivedRef.current = true
        void maybeSendCallAnswer()
        return
      }

      if (frame.type === 'answer' && current.direction === 'outgoing' && current.state !== 'ended') {
        updateCallState({ ...current, state: 'active', statusText: 'Call active' })
        try {
          await startCallAudioCapture()
        } catch (error) {
          setStatus(`Microphone error: ${toErrorText(error)}`)
        }
        return
      }

      if (frame.type === 'audio_frame') {
        await handleIncomingCallAudioFrame(frame)
        return
      }

      if (frame.type === 'rejected' || frame.type === 'ended') {
        updateCallState({
          ...current,
          state: 'ended',
          statusText: frame.type === 'rejected' ? 'Call rejected' : 'Call ended',
        })
        closeCallSocket()
      }
    }

    socket.onerror = () => {
      const current = activeCallRef.current
      if (!current || current.callId !== callId) {
        return
      }
      updateCallState({ ...current, state: 'ended', statusText: 'Call connection error' })
      closeCallSocket()
    }

    socket.onclose = () => {
      stopCallPingLoop()
      if (callSocketRef.current === socket) {
        callSocketRef.current = null
      }

      stopCallAudio()
      resetCallSessionRefs()

      const current = activeCallRef.current
      if (!current || current.callId !== callId) {
        return
      }

      if (callClosingRef.current) {
        return
      }

      if (current.state !== 'ended') {
        updateCallState({ ...current, state: 'ended', statusText: 'Call disconnected' })
      }
    }
  }

  async function onStartCall() {
    if (!activeChat) {
      setStatus('Select a chat first')
      return
    }
    if (activeCallRef.current && activeCallRef.current.state !== 'ended') {
      setStatus('A call is already active.')
      return
    }

    await runTask(async () => {
      await ensureCallMicPermission()
      const payload = await callInitiate(activeChat.chat_id)
      await openCallSocket(payload.call_id, activeChat.chat_id, 'outgoing', activeChat.with_user)
      setStatus(`Calling ${activeChat.with_user}...`)
    })

    // If no call was established (e.g. 409 conflict), poll immediately to pick up an incoming call
    if (!activeCallRef.current || activeCallRef.current.state === 'ended') {
      void pollWorkspace()
    }
  }

  async function onAcceptCall() {
    const current = activeCallRef.current
    if (!current || current.direction !== 'incoming' || current.state === 'ended') {
      return
    }

    await runTask(async () => {
      await ensureCallMicPermission()
      callAcceptedRef.current = true
      if (!callSocketRef.current || callSocketRef.current.readyState !== WebSocket.OPEN) {
        await openCallSocket(current.callId, current.chatId, 'incoming', current.peerUsername)
        await maybeSendCallAnswer()
        return
      }
      await maybeSendCallAnswer()
    })
  }

  async function onRejectCall() {
    const current = activeCallRef.current
    if (!current || current.direction !== 'incoming' || current.state === 'ended') {
      return
    }

    await runTask(async () => {
      if (
        !callSocketRef.current ||
        callSocketRef.current.readyState === WebSocket.CLOSED ||
        callSocketRef.current.readyState === WebSocket.CLOSING
      ) {
        await openCallSocket(current.callId, current.chatId, 'incoming', current.peerUsername)
      }

      const sentImmediately = sendCallFrame({ type: 'reject' })
      if (!sentImmediately) {
        callCloseAfterFlushRef.current = true
      }

      updateCallState({ ...current, state: 'ended', statusText: 'Call rejected' })
      if (sentImmediately) {
        closeCallSocket()
      }
    })
  }

  async function onEndCall() {
    const current = activeCallRef.current
    if (!current || current.state === 'ended') {
      return
    }

    await runTask(async () => {
      if (
        !callSocketRef.current ||
        callSocketRef.current.readyState === WebSocket.CLOSED ||
        callSocketRef.current.readyState === WebSocket.CLOSING
      ) {
        await openCallSocket(current.callId, current.chatId, current.direction, current.peerUsername)
      }

      const sentImmediately = sendCallFrame({ type: 'end' })
      if (!sentImmediately) {
        callCloseAfterFlushRef.current = true
      }

      updateCallState({ ...current, state: 'ended', statusText: 'Call ended' })

      if (sentImmediately) {
        closeCallSocket()
      }
    })
  }

  async function syncCallState(chatId: number, userId: number) {
    let latest: CallHistoryEntry | null = null
    try {
      const payload = await callHistory(chatId, undefined, 1)
      latest = payload.calls[0] ?? null
    } catch {
      return
    }

    if (!latest) {
      return
    }

    const isParticipant = latest.initiator_id === userId || latest.recipient_id === userId
    if (!isParticipant) {
      return
    }

    const isActive = latest.status === 'initiated' || latest.status === 'ringing' || latest.status === 'accepted'
    const direction: CallDirection = latest.initiator_id === userId ? 'outgoing' : 'incoming'
    const peerUsername = chats.find((chat) => chat.chat_id === chatId)?.with_user ?? activePeerLabel

    const current = activeCallRef.current
    if (!isActive) {
      if (current && current.callId === latest.call_id && current.state !== 'ended') {
        updateCallState({ ...current, state: 'ended', statusText: describeCallStatus(latest.status) })
        closeCallSocket()
      }
      return
    }

    if (current && current.callId === latest.call_id) {
      if (latest.status === 'accepted' && current.state !== 'active') {
        updateCallState({ ...current, state: 'active', statusText: describeCallStatus(latest.status) })
      }
      return
    }

    if (callDiscoverySeenRef.current.has(latest.call_id)) {
      return
    }

    callDiscoverySeenRef.current.add(latest.call_id)
    await openCallSocket(latest.call_id, chatId, direction, peerUsername)

    const next = activeCallRef.current
    if (!next) {
      return
    }

    if (latest.status === 'accepted') {
      updateCallState({ ...next, state: 'active', statusText: describeCallStatus(latest.status) })
    } else {
      const nextState = direction === 'incoming' ? 'incoming' : latest.status === 'ringing' ? 'ringing' : 'outgoing'
      updateCallState({ ...next, state: nextState, statusText: describeCallStatus(latest.status) })
      if (direction === 'incoming') {
        setStatus(`Incoming call from ${peerUsername}.`)
      }
    }
  }

  async function runTask(action: () => Promise<void>) {
    setBusy(true)
    try {
      await action()
    } catch (error) {
      setStatus(toErrorText(error))
    } finally {
      setBusy(false)
    }
  }

  async function requestNotificationPermission(interactive = false) {
    if (!alertsEnabledRef.current && !interactive) {
      setNotificationsAllowed(false)
      return false
    }

    try {
      const allowed = interactive
        ? await requestMessageNotificationPermission()
        : await isMessageNotificationPermissionGranted()
      setNotificationsAllowed(allowed)
      if (interactive) {
        setStatus(
          allowed
            ? 'Notifications enabled.'
            : 'Notifications are blocked. If you are testing a dev build, use the installed MSI app for Windows notifications.',
        )
      }
      return allowed
    } catch {
      setNotificationsAllowed(false)
      if (interactive) {
        setStatus('Could not enable notifications on this runtime.')
      }
      return false
    }
  }

  async function sendTestNotification() {
    if (!alertsEnabled) {
      setStatus('Alerts are disabled in settings.')
      return
    }

    const allowed = await requestNotificationPermission(false)
    if (!allowed) {
      setStatus('Notifications are not enabled yet. Click Enable Alerts first.')
      return
    }

    await notifyIncomingMessage('Omnis')
    setStatus('Test notification sent.')
  }

  function toggleAlerts(enabled: boolean) {
    setAlertsEnabled(enabled)
    if (!enabled) {
      setNotificationsAllowed(false)
      setStatus('Alerts disabled.')
      return
    }

    void requestNotificationPermission(true)
  }

  function openChatMuteMenu(event: React.MouseEvent<HTMLButtonElement>, chat: ChatSummary) {
    event.preventDefault()
    const maxX = Math.max(8, window.innerWidth - 280)
    const maxY = Math.max(8, window.innerHeight - 260)
    const remainingMinutes = Math.ceil(muteRemainingMs(chat.chat_id) / 60000)
    setChatMuteMenu({
      chatId: chat.chat_id,
      username: chat.with_user,
      x: Math.min(event.clientX, maxX),
      y: Math.min(event.clientY, maxY),
      customMinutes: remainingMinutes > 0 ? String(remainingMinutes) : '60',
    })
  }

  function applyChatMuteFromMenu(minutes: number) {
    if (!chatMuteMenu) {
      return
    }

    applyMute(chatMuteMenu.chatId, minutes)
    setStatus(
      `Muted ${chatMuteMenu.username} notifications for ${formatDurationMinutes(Math.max(1, Math.floor(minutes)))}.`,
    )
    setChatMuteMenu(null)
  }

  function removeChatMuteFromMenu() {
    if (!chatMuteMenu) {
      return
    }

    clearMute(chatMuteMenu.chatId)
    setStatus(`Unmuted ${chatMuteMenu.username} notifications.`)
    setChatMuteMenu(null)
  }

  async function runServerDiagnostics() {
    await runTask(async () => {
      const activeRuntime = await authRuntime()
      const baseUrl = activeRuntime.backendUrl.replace(/\/$/, '')

      const pingStart = performance.now()
      const pingResponse = await fetch(`${baseUrl}/`)
      if (!pingResponse.ok) {
        throw new Error(`Ping failed with HTTP ${pingResponse.status}`)
      }
      const pingPayload = (await pingResponse.json()) as { ping?: string; PING?: string }
      const pingLatencyMs = Math.round(performance.now() - pingStart)
      const pingValue = pingPayload.ping || pingPayload.PING || 'pong'

      const versionStart = performance.now()
      const versionResponse = await fetch(`${baseUrl}/version`)
      if (!versionResponse.ok) {
        throw new Error(`Version check failed with HTTP ${versionResponse.status}`)
      }
      const versionPayload = (await versionResponse.json()) as { version?: string }
      const versionLatencyMs = Math.round(performance.now() - versionStart)
      const version = versionPayload.version || 'unknown'

      setServerDiagnostics({
        pingValue,
        pingLatencyMs,
        version,
        versionLatencyMs,
        checkedAt: new Date().toLocaleTimeString(),
      })
      setHealth({ ping: pingValue, version })
      setStatus(`Server check complete • ping ${pingLatencyMs}ms • version ${versionLatencyMs}ms.`)
    })
  }

  async function refreshRuntime() {
    const [nextConfig, nextRuntime] = await Promise.all([getBackendConfig(), authRuntime()])
    setConfig(nextConfig)
    setRuntime(nextRuntime)
    setSetupUrl(nextConfig.backendUrl)
    return { nextConfig, nextRuntime }
  }

  async function loadChats(selectFirst = false) {
    const payload = await chatList()
    setChats(payload)

    if (payload.length === 0) {
      lastSeenMessageIdByChatRef.current.clear()
      setSelectedChatId(null)
      setMessages([])
      return
    }

    if (selectFirst) {
      setSelectedChatId(payload[0].chat_id)
      return
    }

    setSelectedChatId((previous) => {
      if (previous !== null && payload.some((chat) => chat.chat_id === previous)) {
        return previous
      }
      return payload[0].chat_id
    })
  }

  async function loadMessages(chatId: number, peerUsername: string | null) {
    const previousMessages = messagesRef.current
    const previouslyLoaded = loadedChatMessagesRef.current.has(chatId)
    const payload = await chatFetch(chatId, undefined, 80)
    const latest = payload.messages[payload.messages.length - 1]
    if (latest) {
      lastSeenMessageIdByChatRef.current.set(chatId, latest.id)
    }

    loadedChatMessagesRef.current.add(chatId)

    if (!peerUsername) {
      const fallbackMessages = payload.messages.map((message) => ({
        ...message,
        body: message.deleted
          ? null
          : message.message_type === 'call'
            ? formatCallSystemBody(message, null, currentUserRef.current?.id ?? null)
            : message.ciphertext,
        attachments: message.attachments ?? [],
      }))
      setMessages(fallbackMessages)

      if (previouslyLoaded && currentUserRef.current) {
        const previousIds = new Set(previousMessages.map((message) => message.id))
        const incomingCount = fallbackMessages.reduce((count, message) => {
          if (previousIds.has(message.id) || message.deleted) {
            return count
          }
          if (message.sender_id === currentUserRef.current!.id) {
            return count
          }
          return count + 1
        }, 0)
        handleIncomingMessages(incomingCount)
      }

      return
    }

    try {
      await primeEpochKeys(chatId, peerUsername, payload.messages)
    } catch {
      // best-effort key priming
    }

    const decryptedMessages = await Promise.all(
      payload.messages.map(async (message) => {
        if (message.deleted) {
          return {
            ...message,
            body: null,
            attachments: message.attachments ?? [],
          }
        }

        if (message.message_type === 'call') {
          return {
            ...message,
            body: formatCallSystemBody(message, peerUsername, currentUserRef.current?.id ?? null),
            attachments: message.attachments ?? [],
          }
        }

        try {
          const body = await decryptChatMessage(chatId, peerUsername, message)
          return {
            ...message,
            body,
            attachments: message.attachments ?? [],
          }
        } catch {
          return {
            ...message,
            body: '[Decryption failed]',
            attachments: message.attachments ?? [],
          }
        }
      }),
    )

    setMessages(decryptedMessages)

    if (previouslyLoaded && currentUserRef.current) {
      const previousIds = new Set(previousMessages.map((message) => message.id))
      const incomingCount = decryptedMessages.reduce((count, message) => {
        if (previousIds.has(message.id) || message.deleted) {
          return count
        }
        if (message.sender_id === currentUserRef.current!.id) {
          return count
        }
        return count + 1
      }, 0)
      handleIncomingMessages(incomingCount)
    }
  }

  async function enterChatSession(user: { id: number; username: string }, message: string) {
    notificationBaselineReadyRef.current = false
    lastSeenMessageIdByChatRef.current.clear()
    loadedChatMessagesRef.current.clear()
    setCurrentUser(user)
    setScreen('chat')
    setStatus(message)
    await loadChats(true)
  }

  async function tryResumeTokenSession(nextRuntime: AuthRuntime) {
    if (!nextRuntime.token || !hasUnlockedIdentityKeys()) {
      return false
    }

    try {
      const me = await authMe()
      await enterChatSession(me, `Welcome back, ${me.username}.`)
      return true
    } catch {
      return false
    }
  }

  async function tryAutoLoginFromSavedCredentials() {
    const saved = readSavedCredentials()
    if (!saved) {
      return false
    }

    setLoginUsername(saved.username)
    setLoginPassword(saved.password)

    try {
      const session = await authLogin(saved.username, saved.password)
      await enterChatSession(
        { id: session.userId, username: session.username },
        `Signed in automatically as ${session.username}.`,
      )
      return true
    } catch {
      clearSavedCredentials()
      setLoginPassword('')
      return false
    }
  }

  async function continueAfterSetup() {
    const nextRuntime = await authRuntime()
    setRuntime(nextRuntime)

    if (await tryResumeTokenSession(nextRuntime)) {
      return
    }

    if (await tryAutoLoginFromSavedCredentials()) {
      return
    }

    setScreen('auth')
    setStatus('Backend is ready. Please login or sign up.')
  }

  async function refreshWorkspace() {
    if (selectedChatId !== null) {
      const peerUsername = chats.find((chat) => chat.chat_id === selectedChatId)?.with_user ?? null
      await loadMessages(selectedChatId, peerUsername)
    }
    await loadChats()
    setStatus('Chat data refreshed.')
  }

  async function pollWorkspace() {
    if (screen !== 'chat' || pollInFlightRef.current || busyRef.current) {
      return
    }

    const signedInUser = currentUserRef.current
    if (!signedInUser) {
      return
    }

    pollInFlightRef.current = true

    try {
      const listedChats = await chatList()
      setChats((previous) => (areChatListsEqual(previous, listedChats) ? previous : listedChats))

      if (listedChats.length === 0) {
        lastSeenMessageIdByChatRef.current.clear()
        notificationBaselineReadyRef.current = false
        setSelectedChatId(null)
        setMessages([])
        if (activeCallRef.current) {
          updateCallState({ ...activeCallRef.current, state: 'ended', statusText: 'Call ended' })
          closeCallSocket()
        }
        return
      }

      const previousSelected = selectedChatIdRef.current
      const nextSelected =
        previousSelected !== null && listedChats.some((chat) => chat.chat_id === previousSelected)
          ? previousSelected
          : listedChats[0].chat_id

      if (nextSelected !== previousSelected) {
        setSelectedChatId(nextSelected)
      }

      const latestSnapshots = await Promise.allSettled(
        listedChats.map(async (chat) => {
          const payload = await chatFetch(chat.chat_id, undefined, 5)
          return {
            chat,
            messages: payload.messages,
            latest: payload.messages[payload.messages.length - 1] ?? null,
          }
        }),
      )

      let shouldReloadSelected = nextSelected !== previousSelected

      for (const snapshot of latestSnapshots) {
        if (snapshot.status !== 'fulfilled') {
          continue
        }

        const { chat, latest, messages: recentMessages } = snapshot.value
        if (!latest) {
          continue
        }

        const previousLatestId = lastSeenMessageIdByChatRef.current.get(chat.chat_id)
        if (previousLatestId === undefined) {
          lastSeenMessageIdByChatRef.current.set(chat.chat_id, latest.id)
          const shouldNotifyNewChat =
            notificationBaselineReadyRef.current &&
            Number(latest.sender_id) !== Number(signedInUser.id) &&
            alertsEnabledRef.current &&
            !isChatMuted(chat.chat_id) &&
            chat.chat_id !== selectedChatIdRef.current
          if (shouldNotifyNewChat) {
            void notifyIncomingMessage(chat.with_user)
          }
          continue
        }

        if (latest.id <= previousLatestId) {
          continue
        }

        lastSeenMessageIdByChatRef.current.set(chat.chat_id, latest.id)

        if (chat.chat_id === nextSelected) {
          shouldReloadSelected = true
        }

        const hasIncomingSinceLastSeen = recentMessages.some(
          (message) => message.id > previousLatestId && Number(message.sender_id) !== Number(signedInUser.id),
        )

        if (hasIncomingSinceLastSeen && alertsEnabledRef.current && !isChatMuted(chat.chat_id) && chat.chat_id !== selectedChatIdRef.current) {
          void notifyIncomingMessage(chat.with_user)
        }
      }

      notificationBaselineReadyRef.current = true

      if (nextSelected !== null && shouldReloadSelected) {
        const peerUsername = listedChats.find((chat) => chat.chat_id === nextSelected)?.with_user ?? null
        await loadMessages(nextSelected, peerUsername)
      }

      if (nextSelected !== null) {
        await syncCallState(nextSelected, signedInUser.id)
      }
    } catch (error) {
      const errorText = toErrorText(error)
      if (errorText.includes('Not authenticated') || errorText.includes('HTTP 401')) {
        setScreen('auth')
        setStatus('Session expired. Please login again.')
      }
    } finally {
      pollInFlightRef.current = false
    }
  }

  useEffect(() => {
    void runTask(async () => {
      setScreen('boot')
      setDesktopRuntime(isTauriRuntime())

      const { nextConfig, nextRuntime } = await refreshRuntime()
      const verifiedUrl = readVerifiedUrl()
      const savedCredentials = readSavedCredentials()

      if (savedCredentials) {
        setLoginUsername(savedCredentials.username)
      }

      if (verifiedUrl !== nextConfig.backendUrl) {
        setScreen('setup')
        setStatus('Set and test your backend URL before continuing.')
        return
      }

      try {
        const nextHealth = await backendHealth()
        setHealth(nextHealth)
      } catch {
        setScreen('setup')
        setStatus(`Backend at ${nextConfig.backendUrl} is unreachable. Complete Step 1 to continue.`)
        return
      }

      if (await tryResumeTokenSession(nextRuntime)) {
        return
      }

      if (await tryAutoLoginFromSavedCredentials()) {
        return
      }

      if (nextRuntime.token) {
        setStatus('Stored session could not be restored automatically. Please sign in again.')
      } else {
        setStatus('Login or sign up to continue.')
      }

      setScreen('auth')
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    if (screen !== 'chat' || selectedChatId === null) {
      return
    }

    let cancelled = false
    const peerUsername = activeChat?.with_user ?? null

    void loadMessages(selectedChatId, peerUsername).catch((error) => {
      if (!cancelled) {
        setStatus(toErrorText(error))
      }
    })

    return () => {
      cancelled = true
    }
  }, [activeChat?.with_user, screen, selectedChatId])

  useEffect(() => {
    resizeComposerInput()
  }, [draftMessage])

  useEffect(() => {
    if (screen !== 'chat') {
      return
    }

    if (alertsEnabled) {
      void requestNotificationPermission(false)
    } else {
      setNotificationsAllowed(false)
    }

    void pollWorkspace()

    const timerId = window.setInterval(() => {
      void pollWorkspace()
    }, AUTO_REFRESH_MS)

    return () => {
      window.clearInterval(timerId)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [alertsEnabled, screen])

  async function onTestSetup() {
    await runTask(async () => {
      const url = setupUrl.trim()
      if (!url) {
        throw new Error('Backend URL is required')
      }

      const next = await setBackendUrl(url)
      setSetupUrl(next.backendUrl)
      setConfig(next)

      const check = await backendHealth()
      setHealth(check)

      writeVerifiedUrl(next.backendUrl)
      setStatus(`Connected to ${next.backendUrl} (v${check.version}).`)
      await continueAfterSetup()
    })
  }

  async function onLogin() {
    await runTask(async () => {
      const username = loginUsername.trim()
      const password = loginPassword
      const session = await authLogin(username, password)
      writeSavedCredentials({ username, password })
      setCurrentUser({ id: session.userId, username: session.username })
      setLoginPassword('')
      setScreen('chat')
      setStatus(`Logged in as ${session.username}.`)
      await loadChats(true)
    })
  }

  async function onSignup() {
    await runTask(async () => {
      const username = signupUsername.trim()
      if (signupPassword !== signupConfirm) {
        throw new Error('Passwords do not match')
      }

      await authSignup(username, signupPassword)
      const session = await authLogin(username, signupPassword)
      writeSavedCredentials({ username, password: signupPassword })
      setCurrentUser({ id: session.userId, username: session.username })
      setSignupUsername('')
      setSignupPassword('')
      setSignupConfirm('')
      setScreen('chat')
      setStatus(`Account created. Welcome, ${session.username}.`)
      await loadChats(true)
    })
  }

  async function onLogout() {
    await runTask(async () => {
      await authLogout()
      clearSavedCredentials()
      await resetConfig()
      window.localStorage.removeItem(VERIFIED_URL_KEY)
      lastSeenMessageIdByChatRef.current.clear()
      notificationBaselineReadyRef.current = false
      setNotificationsAllowed(null)
      setCurrentUser(null)
      setChats([])
      setMessages([])
      setSelectedChatId(null)
      setReplyTarget(null)
      setDraftMessage('')
      setPendingAttachments([])
      setAttachmentDownloads({})
      setAttachmentPreviewLoading({})
      callDiscoverySeenRef.current.clear()
      closeCallSocket()
      updateCallState(null)
      loadedChatMessagesRef.current.clear()
      if (attachmentInputRef.current) {
        attachmentInputRef.current.value = ''
      }
      closeAttachmentPreview()
      setLoginPassword('')
      setScreen('auth')
      await refreshRuntime()
      setStatus('Logged out.')
    })
  }

  async function onCreateChat() {
    await runTask(async () => {
      const username = newChatUsername.trim()
      if (!username) {
        throw new Error('Username is required')
      }

      const payload = await chatCreate(username)
      setSelectedChatId(payload.chat_id)
      setNewChatUsername('')
      await loadChats()
      await loadMessages(payload.chat_id, username)
      setStatus(`Chat ready with ${username}.`)
    })
  }

  async function onSendMessage() {
    await runTask(async () => {
      if (selectedChatId === null) {
        throw new Error('Select a chat first')
      }

      const chatId = selectedChatId
      const content = draftMessage.trim()
      const filesToUpload = [...pendingAttachments]
      if (!content && filesToUpload.length === 0) {
        return
      }

      const peerUsername = chats.find((chat) => chat.chat_id === chatId)?.with_user
      if (!peerUsername) {
        throw new Error('Could not resolve peer public key')
      }

      const replyId = replyTarget?.id ?? null
      const messageBody = content || (filesToUpload.length > 0 ? '📎' : '')

      setDraftMessage('')
      setPendingAttachments([])
      if (attachmentInputRef.current) {
        attachmentInputRef.current.value = ''
      }

      const sendWithEpoch = async (forceRefreshEpoch: boolean) => {
        const epoch = await resolveChatEpoch(chatId, peerUsername, forceRefreshEpoch)
        const mediaIds: number[] = []

        for (let index = 0; index < filesToUpload.length; index += 1) {
          const file = filesToUpload[index]
          const mediaId = await uploadChatMedia(chatId, file, epoch, (progress) => {
            const percent = Math.round(progress * 100)
            setStatus(`Uploading ${file.name} (${index + 1}/${filesToUpload.length}) • ${percent}%`)
          })
          mediaIds.push(mediaId)
        }

        const encrypted = await encryptMessageForEpoch(messageBody, epoch)
        await chatSendMessage(chatId, {
          ...encrypted,
          reply_id: replyId,
          ...(mediaIds.length > 0 ? { media_ids: mediaIds } : {}),
        })
      }

      try {
        await sendWithEpoch(false)
      } catch (error) {
        const message = toErrorText(error)
        const shouldRetry =
          message.includes('Stale epoch') ||
          message.includes('Unknown epoch') ||
          message.includes('Epoch not initialized')

        if (!shouldRetry) {
          setDraftMessage(content)
          setPendingAttachments(filesToUpload)
          throw error
        }

        invalidateChatEpochCache(chatId)

        try {
          await sendWithEpoch(true)
        } catch (retryError) {
          setDraftMessage(content)
          setPendingAttachments(filesToUpload)
          throw retryError
        }
      }

      setReplyTarget(null)
      forceScrollToBottomRef.current = true
      await loadMessages(chatId, peerUsername)
      await loadChats()
      setStatus(filesToUpload.length > 0 ? 'Message and media sent.' : 'Message sent.')
    })
  }

  if (screen === 'boot') {
    return (
      <div className="relative flex h-full items-center justify-center bg-background overflow-hidden">
        {/* Ambient background glow */}
        <div className="glow-orb w-80 h-80 top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2" />
        <div className="glow-orb w-48 h-48 top-1/4 left-1/3 opacity-60" style={{ animationDelay: '1.5s' }} />

        <div className="relative z-10 flex flex-col items-center gap-10 animate-fade-up">
          {/* Logo mark */}
          <div className="relative">
            <div className="absolute inset-0 rounded-2xl bg-primary/25 blur-2xl animate-float" />
            <div className="relative h-20 w-20 rounded-2xl bg-primary flex items-center justify-center animate-glow-ring">
              <MessageCircle className="h-9 w-9 text-primary-foreground" strokeWidth={1.75} />
            </div>
          </div>

          {/* Brand */}
          <div className="text-center space-y-2">
            <h1 className="font-syne text-5xl font-black tracking-[0.24em] uppercase">OMNIS</h1>
            <p className="font-mono text-[11px] text-muted-foreground/60 tracking-[0.35em] uppercase">Encrypted · Private</p>
          </div>

          {/* Loading indicator */}
          <div className="flex items-center gap-2.5 stagger-3 animate-fade-up">
            <span className="h-1.5 w-1.5 rounded-full bg-primary animate-pulse-dot" />
            <span className="font-mono text-xs text-muted-foreground/60 tracking-wide">Initializing secure runtime</span>
          </div>
        </div>
      </div>
    )
  }

  if (screen === 'setup') {
    return (
      <div className="relative flex h-full items-center justify-center bg-background p-6 overflow-hidden">
        <div className="glow-orb w-64 h-64 top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 opacity-60" />

        <div className="relative z-10 w-full max-w-sm space-y-6 animate-fade-up">
          {/* Header */}
          <div className="space-y-1.5">
            <div className="flex items-center gap-2 mb-3">
              <div className="h-8 w-8 rounded-xl bg-primary/15 border border-primary/25 flex items-center justify-center">
                <PlugZap className="h-4 w-4 text-primary" strokeWidth={1.75} />
              </div>
              <span className="font-mono text-xs text-primary/70 tracking-[0.18em] uppercase font-medium">Step 1 of 2</span>
            </div>
            <h1 className="font-syne text-2xl font-bold">Connect to server</h1>
            <p className="text-sm text-muted-foreground leading-relaxed">
              Enter your backend URL. Verified connections are remembered.
            </p>
          </div>

          {/* Connection card */}
          <div className="rounded-2xl border border-border/60 bg-card p-5 space-y-3">
            <Input
              value={setupUrl}
              onChange={(event) => setSetupUrl(event.target.value)}
              placeholder="http://127.0.0.1:6767"
              className="h-12 font-mono text-sm rounded-xl border-border/50 bg-input/60 focus-visible:ring-1 focus-visible:ring-primary/50 focus-visible:ring-offset-0"
            />
            <div className="flex flex-wrap items-center gap-2">
              <button
                disabled={busy}
                onClick={() => void onTestSetup()}
                className="h-11 px-5 rounded-xl bg-primary text-primary-foreground text-sm font-semibold hover:opacity-85 active:scale-[0.98] transition-all disabled:opacity-40 flex items-center gap-2"
              >
                <Server className="h-4 w-4" strokeWidth={1.75} />
                Test connection
              </button>
              {health ? (
                <span className="inline-flex items-center gap-1.5 rounded-lg bg-emerald-500/10 border border-emerald-500/20 px-2.5 py-1 text-xs text-emerald-400 font-medium">
                  <span className="h-1.5 w-1.5 rounded-full bg-emerald-400" />
                  v{health.version}
                </span>
              ) : null}
              <span className={cn('inline-flex items-center rounded-lg px-2.5 py-1 text-xs font-mono border', desktopRuntime ? 'bg-primary/10 border-primary/20 text-primary/80' : 'bg-secondary border-border/40 text-muted-foreground')}>
                {desktopRuntime ? 'Tauri' : 'Browser'}
              </span>
            </div>
          </div>

          {/* Status */}
          <div className="rounded-xl border border-border/40 bg-muted/40 px-4 py-3 font-mono text-xs text-muted-foreground/80">
            {status}
          </div>
        </div>
      </div>
    )
  }

  if (screen === 'auth') {
    return (
      <div className="relative flex h-full items-center justify-center bg-background p-6 overflow-hidden">
        <div className="glow-orb w-72 h-72 top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2" />

        <div className="relative z-10 w-full max-w-xs space-y-7 animate-fade-up">
          {/* Logo & brand */}
          <div className="flex flex-col items-center gap-4">
            <div className="relative">
              <div className="absolute inset-0 rounded-2xl bg-primary/20 blur-xl animate-float" />
              <div className="relative h-16 w-16 rounded-2xl bg-primary flex items-center justify-center animate-glow-ring">
                <MessageCircle className="h-7 w-7 text-primary-foreground" strokeWidth={1.75} />
              </div>
            </div>
            <div className="text-center">
              <h1 className="font-syne text-3xl font-black tracking-[0.2em] uppercase">OMNIS</h1>
              <p className="text-xs text-muted-foreground/60 mt-1 font-mono tracking-wider">End-to-end encrypted</p>
            </div>
          </div>

          {/* Auth card */}
          <div className="rounded-2xl border border-border/60 bg-card p-5 space-y-4">
            {/* Tab switcher */}
            <div className="grid grid-cols-2 gap-1 rounded-xl bg-muted/60 p-1">
              <button
                className={cn(
                  'h-9 rounded-lg text-sm font-semibold transition-all',
                  authMode === 'login'
                    ? 'bg-card text-foreground shadow-sm'
                    : 'text-muted-foreground hover:text-foreground',
                )}
                disabled={busy}
                onClick={() => setAuthMode('login')}
              >
                Login
              </button>
              <button
                className={cn(
                  'h-9 rounded-lg text-sm font-semibold transition-all',
                  authMode === 'signup'
                    ? 'bg-card text-foreground shadow-sm'
                    : 'text-muted-foreground hover:text-foreground',
                )}
                disabled={busy}
                onClick={() => setAuthMode('signup')}
              >
                Sign up
              </button>
            </div>

            {authMode === 'login' ? (
              <div className="space-y-3">
                <Input
                  value={loginUsername}
                  onChange={(event) => setLoginUsername(event.target.value)}
                  placeholder="Username"
                  className="h-12 rounded-xl border-border/50 bg-input/60 focus-visible:ring-1 focus-visible:ring-primary/50 focus-visible:ring-offset-0"
                  autoComplete="username"
                />
                <Input
                  type="password"
                  value={loginPassword}
                  onChange={(event) => setLoginPassword(event.target.value)}
                  placeholder="Password"
                  className="h-12 rounded-xl border-border/50 bg-input/60 focus-visible:ring-1 focus-visible:ring-primary/50 focus-visible:ring-offset-0"
                  autoComplete="current-password"
                  onKeyDown={(e) => { if (e.key === 'Enter' && canLogin && !busy) void onLogin() }}
                />
                <button
                  disabled={busy || !canLogin}
                  onClick={() => void onLogin()}
                  className="w-full h-12 rounded-xl bg-primary text-primary-foreground text-sm font-semibold hover:opacity-90 active:scale-[0.98] transition-all disabled:opacity-35"
                >
                  Sign in
                </button>
              </div>
            ) : (
              <div className="space-y-3">
                <Input
                  value={signupUsername}
                  onChange={(event) => setSignupUsername(event.target.value)}
                  placeholder="Username (min 5 chars)"
                  className="h-12 rounded-xl border-border/50 bg-input/60 focus-visible:ring-1 focus-visible:ring-primary/50 focus-visible:ring-offset-0"
                  autoComplete="username"
                />
                <Input
                  type="password"
                  value={signupPassword}
                  onChange={(event) => setSignupPassword(event.target.value)}
                  placeholder="Password (min 6 chars)"
                  className="h-12 rounded-xl border-border/50 bg-input/60 focus-visible:ring-1 focus-visible:ring-primary/50 focus-visible:ring-offset-0"
                  autoComplete="new-password"
                />
                <Input
                  type="password"
                  value={signupConfirm}
                  onChange={(event) => setSignupConfirm(event.target.value)}
                  placeholder="Confirm password"
                  className="h-12 rounded-xl border-border/50 bg-input/60 focus-visible:ring-1 focus-visible:ring-primary/50 focus-visible:ring-offset-0"
                  autoComplete="new-password"
                />
                <button
                  disabled={busy || !canSignup}
                  onClick={() => void onSignup()}
                  className="w-full h-12 rounded-xl bg-primary text-primary-foreground text-sm font-semibold hover:opacity-90 active:scale-[0.98] transition-all disabled:opacity-35"
                >
                  Create account
                </button>
              </div>
            )}
          </div>

          {/* Status */}
          <p className="text-center font-mono text-xs text-muted-foreground/55">{status}</p>
        </div>
      </div>
    )
  }

  if (screen === 'settings') {
    return (
      <div className="flex h-full min-h-0 flex-col bg-background">
        {/* Header */}
        <header className="glass shrink-0 flex items-center justify-between px-4 py-3 pt-safe">
          <div className="flex items-center gap-3">
            <button
              className="h-9 w-9 flex items-center justify-center rounded-xl text-muted-foreground hover:text-foreground hover:bg-accent transition-all"
              disabled={busy}
              onClick={() => setScreen('chat')}
              aria-label="Back"
            >
              <ArrowLeft className="h-5 w-5" strokeWidth={1.75} />
            </button>
            <h1 className="font-syne text-lg font-bold tracking-wide">Settings</h1>
          </div>
          <button
            className="flex h-9 items-center gap-1.5 rounded-xl px-3 text-sm text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-all disabled:opacity-40"
            disabled={busy}
            onClick={onLogout}
          >
            <LogOut className="h-4 w-4" strokeWidth={1.75} />
            <span className="hidden sm:inline">Sign out</span>
          </button>
        </header>

        <ScrollArea className="min-h-0 flex-1" viewportClassName="p-4 md:p-5">
          <div className="max-w-2xl mx-auto space-y-3 md:grid md:grid-cols-2 md:gap-3 md:space-y-0">

            {/* Server diagnostics */}
            <div className="rounded-2xl border border-border/50 bg-card p-5 space-y-3">
              <div>
                <h2 className="font-semibold text-foreground">Server diagnostics</h2>
                <p className="text-xs text-muted-foreground mt-0.5">Round-trip latency to your backend.</p>
              </div>
              <div className="rounded-xl bg-input/50 border border-border/40 px-3 py-2.5 font-mono text-xs text-muted-foreground/70 truncate">
                {config?.backendUrl ?? runtime?.backendUrl ?? setupUrl}
              </div>
              <button
                disabled={busy}
                onClick={() => void runServerDiagnostics()}
                className="w-full h-10 rounded-xl border border-border/40 bg-secondary/60 text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-all flex items-center justify-center gap-2 disabled:opacity-40"
              >
                <Activity className="h-4 w-4" strokeWidth={1.75} />
                Run diagnostics
              </button>
              {serverDiagnostics ? (
                <div className="rounded-xl bg-input/40 border border-border/30 px-3 py-3 font-mono text-xs space-y-2">
                  <div className="flex justify-between"><span className="text-muted-foreground/60">Ping</span><span>{serverDiagnostics.pingValue}</span></div>
                  <div className="flex justify-between"><span className="text-muted-foreground/60">Latency</span><span>{serverDiagnostics.pingLatencyMs}ms</span></div>
                  <div className="flex justify-between"><span className="text-muted-foreground/60">Version</span><span>{serverDiagnostics.version}</span></div>
                  <div className="flex justify-between"><span className="text-muted-foreground/60">Ver. latency</span><span>{serverDiagnostics.versionLatencyMs}ms</span></div>
                  <p className="text-muted-foreground/40 pt-1.5 border-t border-border/30">{serverDiagnostics.checkedAt}</p>
                </div>
              ) : null}
            </div>

            {/* Notifications */}
            <div className="rounded-2xl border border-border/50 bg-card p-5 space-y-3">
              <div>
                <h2 className="font-semibold text-foreground">Notifications</h2>
                <p className="text-xs text-muted-foreground/70 mt-0.5">Desktop alerts and per-chat muting.</p>
              </div>
              <div className="grid grid-cols-2 gap-2">
                <button
                  className={cn('h-10 rounded-xl text-sm font-medium transition-all flex items-center justify-center gap-1.5',
                    alertsEnabled
                      ? 'bg-primary text-primary-foreground'
                      : 'border border-border/40 bg-secondary/60 text-muted-foreground hover:text-foreground hover:bg-accent'
                  )}
                  disabled={busy || alertsEnabled}
                  onClick={() => toggleAlerts(true)}
                >
                  <Bell className="h-4 w-4" strokeWidth={1.75} />
                  Enable
                </button>
                <button
                  className={cn('h-10 rounded-xl text-sm font-medium transition-all flex items-center justify-center gap-1.5',
                    !alertsEnabled
                      ? 'bg-primary text-primary-foreground'
                      : 'border border-border/40 bg-secondary/60 text-muted-foreground hover:text-foreground hover:bg-accent'
                  )}
                  disabled={busy || !alertsEnabled}
                  onClick={() => toggleAlerts(false)}
                >
                  <BellOff className="h-4 w-4" strokeWidth={1.75} />
                  Disable
                </button>
              </div>
              <button
                className="w-full h-10 rounded-xl border border-border/40 bg-secondary/60 text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-all flex items-center justify-center gap-2 disabled:opacity-40"
                disabled={busy || !alertsEnabled || notificationsAllowed !== true}
                onClick={() => void sendTestNotification()}
              >
                <Bell className="h-3.5 w-3.5" strokeWidth={1.75} />
                Send test alert
              </button>
              <div className="rounded-xl bg-input/40 border border-border/30 px-3 py-2.5 font-mono text-xs text-muted-foreground/60">
                {alertsEnabled ? (notificationsAllowed ? '● Alerts active' : '○ Permission needed') : '○ Alerts off'}
              </div>
              <p className="text-xs text-muted-foreground/40">Right-click any chat to mute it temporarily.</p>
            </div>

            {/* Muted chats */}
            <div className="rounded-2xl border border-border/50 bg-card p-5 space-y-3 md:col-span-2">
              <div>
                <h2 className="font-semibold text-foreground">Muted chats</h2>
                <p className="text-xs text-muted-foreground/70 mt-0.5">Active per-chat notification mutes.</p>
              </div>
              {activeMutedChats.length === 0 ? (
                <div className="rounded-2xl border border-dashed border-border/40 px-4 py-8 text-center text-sm text-muted-foreground/55">
                  No muted chats.
                </div>
              ) : (
                <div className="space-y-2 md:grid md:grid-cols-2 md:gap-2 md:space-y-0">
                  {activeMutedChats.map((entry) => (
                    <div key={entry.chatId} className="flex items-center justify-between gap-3 rounded-xl border border-border/40 bg-secondary/50 px-3 py-3">
                      <div className="min-w-0">
                        <p className="truncate text-sm font-medium text-foreground">{entry.username}</p>
                        <p className="font-mono text-xs text-muted-foreground/55 mt-0.5">{formatDurationMinutes(entry.remainingMinutes)} remaining</p>
                      </div>
                      <button
                        className="h-8 px-3 rounded-lg text-xs border border-border/40 text-muted-foreground hover:text-foreground hover:bg-accent transition-all"
                        onClick={() => clearMute(entry.chatId)}
                      >
                        Unmute
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>

          </div>
        </ScrollArea>

        {/* Status bar */}
        <div className="shrink-0 border-t border-border/30 bg-card/60 px-4 py-2.5 font-mono text-xs text-muted-foreground/55 truncate pb-safe">
          {status}
        </div>
      </div>
    )
  }


  return (
    <div className="flex h-full min-h-0 overflow-hidden bg-background">

      {/* ── Sidebar ── */}
      <aside className={cn(
        'flex flex-col w-full md:w-[300px] lg:w-[320px] shrink-0 bg-card/40 border-r border-border/40',
        selectedChatId !== null && 'hidden md:flex'
      )}>

        {/* ── Brand bar ── */}
        <div className="shrink-0 flex items-center justify-between px-4" style={{ paddingTop: 'max(16px, env(safe-area-inset-top))' }}>
          <div className="flex items-center gap-2.5 py-3">
            <div className="h-8 w-8 rounded-xl bg-primary flex items-center justify-center shrink-0 animate-glow-ring">
              <MessageCircle className="h-3.5 w-3.5 text-primary-foreground" strokeWidth={2} />
            </div>
            <span className="font-syne text-base font-bold tracking-[0.18em] uppercase">OMNIS</span>
            <span className={cn('font-mono text-[9px] px-1.5 py-0.5 rounded-md border', desktopRuntime ? 'bg-primary/10 border-primary/20 text-primary/70' : 'bg-secondary border-border/40 text-muted-foreground/55')}>
              {desktopRuntime ? 'Tauri' : 'Web'}
            </span>
          </div>
          <div className="flex items-center gap-0.5">
            {alertsEnabled && notificationsAllowed === false ? (
              <button className="h-8 w-8 flex items-center justify-center rounded-xl text-amber-400 hover:bg-accent transition-all" disabled={busy} onClick={() => void requestNotificationPermission(true)} aria-label="Enable alerts">
                <Bell className="h-4 w-4" strokeWidth={1.75} />
              </button>
            ) : alertsEnabled && notificationsAllowed === true ? (
              <button className="h-8 w-8 flex items-center justify-center rounded-xl text-muted-foreground/60 hover:text-foreground hover:bg-accent transition-all" disabled={busy} onClick={() => void sendTestNotification()} aria-label="Test notification">
                <Bell className="h-4 w-4" strokeWidth={1.75} />
              </button>
            ) : null}
            <button className="h-8 w-8 flex items-center justify-center rounded-xl text-muted-foreground/60 hover:text-foreground hover:bg-accent transition-all" disabled={busy} onClick={() => setScreen('settings')} aria-label="Settings">
              <Settings2 className="h-4 w-4" strokeWidth={1.75} />
            </button>
          </div>
        </div>

        {/* ── User profile card ── */}
        {currentUser ? (
          <div className="mx-3 mb-1 rounded-2xl border border-border/40 bg-secondary/40 px-3.5 py-3 flex items-center gap-3">
            <div className={cn('h-10 w-10 rounded-full border-2 flex items-center justify-center text-sm font-bold shrink-0', usernameColorClass(currentUser.username))}>
              {getInitials(currentUser.username)}
            </div>
            <div className="min-w-0 flex-1">
              <p className="text-sm font-semibold truncate">@{currentUser.username}</p>
              <p className="font-mono text-[10px] text-muted-foreground/50 mt-0.5">
                {health ? `v${health.version} · E2EE` : '○ connecting'}
              </p>
            </div>
            <div className="flex items-center gap-0.5 shrink-0">
              <button className="h-7 w-7 flex items-center justify-center rounded-lg text-muted-foreground/50 hover:text-foreground hover:bg-accent transition-all" disabled={busy} onClick={() => void refreshWorkspace()} aria-label="Refresh">
                <Activity className="h-3.5 w-3.5" strokeWidth={1.75} />
              </button>
              <button className="h-7 w-7 flex items-center justify-center rounded-lg text-muted-foreground/50 hover:text-destructive hover:bg-destructive/10 transition-all" disabled={busy} onClick={onLogout} aria-label="Sign out">
                <LogOut className="h-3.5 w-3.5" strokeWidth={1.75} />
              </button>
            </div>
          </div>
        ) : null}

        {/* ── Search + new conversation ── */}
        <div className="px-3 pt-2.5 pb-2 space-y-2 shrink-0">
          <div className="relative">
            <Search className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground/40" strokeWidth={2} />
            <Input
              className="pl-9 h-9 rounded-full border-border/30 bg-secondary/40 text-sm focus-visible:ring-1 focus-visible:ring-primary/40 focus-visible:ring-offset-0 placeholder:text-muted-foreground/40"
              value={chatSearch}
              onChange={(event) => setChatSearch(event.target.value)}
              placeholder="Search..."
            />
          </div>
          <div className="flex gap-2">
            <Input
              className="h-9 rounded-full flex-1 border-border/30 bg-secondary/40 text-sm focus-visible:ring-1 focus-visible:ring-primary/40 focus-visible:ring-offset-0 placeholder:text-muted-foreground/40"
              value={newChatUsername}
              onChange={(event) => setNewChatUsername(event.target.value)}
              placeholder="New conversation..."
              onKeyDown={(e) => { if (e.key === 'Enter' && newChatUsername.trim() && !busy) void onCreateChat() }}
            />
            <button
              className="h-9 w-9 shrink-0 flex items-center justify-center rounded-full bg-primary text-primary-foreground hover:opacity-85 active:scale-95 transition-all disabled:opacity-30 touch-active"
              disabled={busy || newChatUsername.trim().length === 0}
              onClick={onCreateChat}
              aria-label="Start chat"
            >
              <MessageSquarePlus className="h-3.5 w-3.5" strokeWidth={2} />
            </button>
          </div>
        </div>

        {/* ── Chat list ── */}
        <ScrollArea className="min-h-0 flex-1" viewportClassName="px-3 pb-4">
          {visibleChats.length === 0 ? (
            <div className="mt-2 rounded-2xl border border-dashed border-border/30 px-4 py-10 text-center text-sm text-muted-foreground/50">
              {chats.length === 0 ? 'No conversations yet.' : 'No results.'}
            </div>
          ) : (
            <div className="space-y-px">
              {visibleChats.map((chat) => {
                const isSelected = chat.chat_id === selectedChatId
                const mutedFor = muteLabel(chat.chat_id)
                const colorClass = usernameColorClass(chat.with_user)
                return (
                  <button
                    key={chat.chat_id}
                    className={cn(
                      'w-full flex items-center gap-3 rounded-2xl px-3 py-3 text-left transition-all touch-active',
                      isSelected
                        ? 'bg-primary/10 border border-primary/20'
                        : 'hover:bg-accent/60 border border-transparent'
                    )}
                    onClick={() => setSelectedChatId(chat.chat_id)}
                    onContextMenu={(event) => openChatMuteMenu(event, chat)}
                  >
                    <div className={cn('h-11 w-11 shrink-0 rounded-full border-2 flex items-center justify-center text-sm font-bold', colorClass)}>
                      {getInitials(chat.with_user)}
                    </div>
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-semibold leading-tight">{chat.with_user}</p>
                      <p className="font-mono text-[10px] text-muted-foreground/50 truncate mt-0.5">
                        {mutedFor ? `muted · ${mutedFor}` : `chat #${chat.chat_id}`}
                      </p>
                    </div>
                    {isSelected ? <div className="h-1.5 w-1.5 shrink-0 rounded-full bg-primary animate-pulse-dot" /> : null}
                  </button>
                )
              })}
            </div>
          )}
        </ScrollArea>
      </aside>

      {/* ── Chat section ── */}
      <section className={cn('relative min-h-0 flex-1 flex-col bg-background', selectedChatId === null ? 'hidden md:flex' : 'flex')}>

        {/* ── Chat header ── */}
        <div className="glass shrink-0 flex items-center gap-3 px-4" style={{ paddingTop: 'max(12px, env(safe-area-inset-top))', paddingBottom: '12px' }}>
          <button
            className="h-9 w-9 flex md:hidden items-center justify-center rounded-xl text-muted-foreground hover:text-foreground hover:bg-accent transition-all shrink-0"
            onClick={() => setSelectedChatId(null)}
            aria-label="Back"
          >
            <ArrowLeft className="h-5 w-5" strokeWidth={1.75} />
          </button>
          {activeChat ? (
            <>
              <div className={cn('h-10 w-10 shrink-0 rounded-full border-2 flex items-center justify-center text-sm font-bold', usernameColorClass(activeChat.with_user))}>
                {getInitials(activeChat.with_user)}
              </div>
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-semibold leading-tight">{activeChat.with_user}</p>
                <div className="flex items-center gap-1.5 mt-0.5">
                  <span className="h-1.5 w-1.5 rounded-full bg-primary shrink-0" />
                  <p className="font-mono text-[10px] text-muted-foreground/55 truncate">E2EE · chat #{activeChat.chat_id}</p>
                </div>
              </div>
            </>
          ) : (
            <div className="flex-1">
              <p className="text-sm text-muted-foreground/50">Select a conversation</p>
            </div>
          )}
          <button
            className={cn('h-9 w-9 shrink-0 flex items-center justify-center rounded-xl transition-all',
              busy || !activeChat || (activeCall !== null && activeCall.state !== 'ended')
                ? 'opacity-25 cursor-not-allowed text-muted-foreground'
                : 'text-muted-foreground hover:text-foreground hover:bg-accent'
            )}
            disabled={busy || !activeChat || (activeCall !== null && activeCall.state !== 'ended')}
            onClick={onStartCall}
            aria-label="Start call"
          >
            <Phone className="h-[18px] w-[18px]" strokeWidth={1.75} />
          </button>
        </div>

        {/* Call banner */}
        {activeCall && activeChat && activeCall.chatId === activeChat.chat_id ? (
          <div className="shrink-0 px-3 pt-2 md:px-4">
            <div className={cn('flex items-center justify-between gap-3 rounded-2xl border px-4 py-2.5',
              activeCall.state === 'incoming' ? 'border-primary/30 bg-primary/10'
                : activeCall.state === 'active' ? 'border-emerald-500/30 bg-emerald-500/10'
                : 'border-border/40 bg-secondary/50'
            )}>
              <div className="flex min-w-0 items-center gap-2">
                {activeCall.direction === 'incoming'
                  ? <PhoneIncoming className="h-4 w-4 shrink-0 text-emerald-400" strokeWidth={1.75} />
                  : <Phone className="h-4 w-4 shrink-0 text-emerald-400" strokeWidth={1.75} />}
                <span className="text-sm truncate">{activeCall.statusText}</span>
                {activeCall.rttMs !== null ? <span className="font-mono text-[11px] text-muted-foreground/60 shrink-0">· {activeCall.rttMs}ms</span> : null}
              </div>
              <div className="flex items-center gap-1.5 shrink-0">
                {activeCall.direction === 'incoming' && activeCall.state === 'incoming' ? (
                  <>
                    <button type="button" aria-label="Accept" className="h-9 w-9 flex items-center justify-center rounded-full bg-emerald-600 text-white hover:bg-emerald-500 active:scale-95 transition-all touch-active" disabled={busy} onClick={() => void onAcceptCall()}>
                      <Phone className="h-4 w-4" strokeWidth={1.75} />
                    </button>
                    <button type="button" aria-label="Reject" className="h-9 w-9 flex items-center justify-center rounded-full bg-destructive text-destructive-foreground hover:opacity-85 active:scale-95 transition-all touch-active" disabled={busy} onClick={() => void onRejectCall()}>
                      <PhoneOff className="h-4 w-4" strokeWidth={1.75} />
                    </button>
                  </>
                ) : null}
                {activeCall.state !== 'ended' && !(activeCall.direction === 'incoming' && activeCall.state === 'incoming') ? (
                  <button type="button" aria-label="End call" className="h-9 w-9 flex items-center justify-center rounded-full bg-destructive text-destructive-foreground hover:opacity-85 active:scale-95 transition-all touch-active" disabled={busy} onClick={() => void onEndCall()}>
                    <PhoneOff className="h-4 w-4" strokeWidth={1.75} />
                  </button>
                ) : (
                  <button type="button" className="h-9 px-3 rounded-xl text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-all" disabled={busy} onClick={() => updateCallState(null)}>Dismiss</button>
                )}
              </div>
            </div>
          </div>
        ) : null}

        {/* Messages */}
        <ScrollArea className="min-h-0 flex-1" viewportRef={messageListRef} viewportClassName="px-4 py-5 md:px-5" onViewportScroll={refreshBehindCount}>
          <div className="flex w-full flex-col gap-1.5">
            {showUnreadBanner ? (
              <div className="mx-auto rounded-full border border-primary/25 bg-primary/10 px-4 py-1.5 text-xs text-foreground/75 text-center animate-fade-in">
                {bannerUnreadCount} new message{bannerUnreadCount === 1 ? '' : 's'} while away
              </div>
            ) : null}
            {unreadCount > 0 && behindMessages > 10 ? (
              <div className="mx-auto rounded-full border border-primary/25 bg-primary/10 px-4 py-1.5 text-xs text-foreground/75 text-center">
                {unreadCount} unread ↓
              </div>
            ) : null}
            {activeChat === null ? (
              <div className="flex flex-col items-center justify-center gap-5 py-24 text-center">
                <div className="h-16 w-16 rounded-2xl border border-border/40 bg-card flex items-center justify-center">
                  <MessageCircle className="h-7 w-7 text-muted-foreground/30" strokeWidth={1.5} />
                </div>
                <div className="space-y-1">
                  <p className="text-sm font-semibold text-foreground/70">No conversation selected</p>
                  <p className="text-xs text-muted-foreground/50">Choose a chat from the sidebar</p>
                </div>
              </div>
            ) : messages.length === 0 ? (
              <div className="flex flex-col items-center justify-center gap-5 py-24 text-center">
                <div className={cn('h-14 w-14 rounded-full border-2 flex items-center justify-center text-sm font-bold', usernameColorClass(activeChat.with_user))}>
                  {getInitials(activeChat.with_user)}
                </div>
                <div className="space-y-1">
                  <p className="text-sm font-semibold">{activeChat.with_user}</p>
                  <p className="text-xs text-muted-foreground/55">No messages yet. Say hello!</p>
                </div>
              </div>
            ) : (
                messages.map((message, index) => {
                  const isCallMessage = message.message_type === 'call'
                  const mine = currentUser !== null && message.sender_id === currentUser.id
                  const attachments = message.attachments ?? []
                  const hasAttachments = !message.deleted && attachments.length > 0
                  const hideBodyText = hasAttachments && (message.body ?? '').trim() === '📎'
                  const repliedMessage = message.reply_id !== null && message.reply_id !== undefined ? messageById.get(message.reply_id) ?? null : null

                  if (!message.deleted && isCallMessage) {
                    const callDuration = formatCallDurationClock(message.duration_seconds)
                    const callLabel = message.call_status === 'missed' ? 'Missed call' : message.call_status === 'rejected' ? 'Call declined' : message.call_status === 'ended' ? (callDuration ? `Call ended · ${callDuration}` : 'Call ended') : message.call_status === 'accepted' ? (callDuration ? `Call completed · ${callDuration}` : 'Call completed') : 'Call'
                    const Icon = message.call_status === 'missed' ? PhoneMissed : message.call_status === 'rejected' ? PhoneOff : Phone
                    const iconTone = message.call_status === 'missed' ? 'text-amber-400' : message.call_status === 'rejected' ? 'text-destructive' : 'text-emerald-400'
                    return (
                      <div key={message.id} data-message-index={index} className="mx-auto flex w-fit max-w-[80%] items-center gap-2 rounded-full border border-border/40 bg-secondary/60 px-4 py-2 text-xs text-muted-foreground">
                        <Icon className={cn('h-3.5 w-3.5 shrink-0', iconTone)} />
                        <span>{callLabel}</span>
                        <span className="font-mono text-[10px] text-muted-foreground/50">{formatMessageTime(message.created_at)}</span>
                      </div>
                    )
                  }

                  return (
                    <div
                      key={message.id}
                      data-message-index={index}
                      className={cn('group relative max-w-[82%] sm:max-w-[70%] rounded-3xl px-3.5 py-2.5 text-sm', mine ? 'ml-auto bg-primary text-primary-foreground rounded-tr-sm' : 'mr-auto bg-card border border-border/50 text-foreground rounded-tl-sm')}
                      onContextMenu={(e) => { e.preventDefault(); setOpenMenuMessageId(message.id) }}
                    >
                      {!message.deleted && message.reply_id ? (
                        <div className={cn('mb-2 rounded-xl px-2.5 py-2 text-xs border-l-2', mine ? 'bg-primary-foreground/10 border-primary-foreground/40' : 'bg-background/40 border-primary/50')}>
                          <p className={cn('font-semibold text-[11px]', mine ? 'text-primary-foreground/70' : 'text-muted-foreground')}>
                            {repliedMessage ? senderLabel(repliedMessage.sender_id) : 'Original message'}
                          </p>
                          <p className={cn('mt-0.5 truncate', mine ? 'text-primary-foreground/70' : 'text-muted-foreground')}>
                            {repliedMessage ? repliedMessage.attachments.length > 0 && (repliedMessage.body ?? '').trim() === '📎' ? `[Attachment] ${repliedMessage.attachments.length} file${repliedMessage.attachments.length === 1 ? '' : 's'}` : previewText(repliedMessage.deleted ? 'Message deleted' : repliedMessage.body) : '[Original unavailable]'}
                          </p>
                        </div>
                      ) : null}
                      {message.deleted ? (
                        <p className={cn('italic text-xs', mine ? 'text-primary-foreground/60' : 'text-muted-foreground')}>Message deleted</p>
                      ) : !hideBodyText ? (
                        <>
                          <p style={{ wordBreak: 'break-word', overflowWrap: 'anywhere' }} className="whitespace-pre-wrap leading-relaxed">
                            {(message.body ?? '[Decryption failed]').length > 300 && !expandedMessages.has(message.id)
                              ? (message.body ?? '[Decryption failed]').slice(0, 300) + '…'
                              : (message.body ?? '[Decryption failed]')}
                          </p>
                          {(message.body ?? '[Decryption failed]').length > 300 ? (
                            <button type="button" className={cn('mt-1 text-[11px] hover:underline', mine ? 'text-primary-foreground/60' : 'text-primary')} onClick={() => setExpandedMessages((prev) => { const next = new Set(prev); if (next.has(message.id)) next.delete(message.id); else next.add(message.id); return next })}>
                              {expandedMessages.has(message.id) ? 'Show less' : 'Show more'}
                            </button>
                          ) : null}
                        </>
                      ) : null}
                      {hasAttachments ? (
                        <div className="mt-2 space-y-1.5">
                          {attachments.map((attachment) => {
                            const key = attachmentDownloadKey(message.id, attachment.upload_id)
                            const downloading = Boolean(attachmentDownloads[key])
                            const previewing = Boolean(attachmentPreviewLoading[key])
                            const isImage = (attachment.mime_type || '').toLowerCase().startsWith('image/')
                            return (
                              <div key={key} className={cn('flex items-center justify-between gap-2 rounded-xl px-3 py-2', mine ? 'bg-primary-foreground/10 border border-primary-foreground/20' : 'bg-background/50 border border-border/40')}>
                                <div className="min-w-0">
                                  <p className={cn('truncate font-mono text-[11px] font-medium', mine ? 'text-primary-foreground/80' : 'text-foreground')}>{attachment.mime_type || 'application/octet-stream'}</p>
                                  <p className={cn('truncate font-mono text-[11px] mt-0.5', mine ? 'text-primary-foreground/55' : 'text-muted-foreground')}>{attachmentFileName(attachment)} · {formatFileSize(attachment.total_size)}</p>
                                </div>
                                <div className="flex items-center gap-1 shrink-0">
                                  <button type="button" className={cn('h-8 px-2 rounded-lg text-[11px] transition-colors flex items-center gap-1', mine ? 'bg-primary-foreground/15 text-primary-foreground hover:bg-primary-foreground/25' : 'bg-accent text-muted-foreground hover:text-foreground')} disabled={busy || downloading} onClick={() => void handleDownloadAttachment(message, attachment)}>
                                    <Download className="h-3.5 w-3.5" />
                                    {downloading ? '…' : 'Save'}
                                  </button>
                                  {isImage ? (
                                    <button type="button" className={cn('h-8 px-2 rounded-lg text-[11px] transition-colors flex items-center gap-1', mine ? 'bg-primary-foreground/15 text-primary-foreground hover:bg-primary-foreground/25' : 'bg-accent text-muted-foreground hover:text-foreground')} disabled={busy || previewing} onClick={() => void handlePreviewAttachment(message, attachment)}>
                                      <Eye className="h-3.5 w-3.5" />
                                      {previewing ? '…' : 'View'}
                                    </button>
                                  ) : null}
                                </div>
                              </div>
                            )
                          })}
                        </div>
                      ) : null}
                      <p className={cn('mt-1.5 font-mono text-[10px] text-right', mine ? 'text-primary-foreground/50' : 'text-muted-foreground/55')}>
                        {formatMessageTime(message.created_at)}
                      </p>
                      {openMenuMessageId === message.id ? (
                        <>
                          <div className="fixed inset-0 z-40" onClick={() => setOpenMenuMessageId(null)} />
                          <div className={cn('absolute top-0 z-50 w-40 rounded-2xl border border-border/50 bg-card/95 p-1.5 shadow-xl backdrop-blur-xl', mine ? 'right-0' : 'left-0')}>
                            {!message.deleted && message.message_type !== 'call' ? (
                              <button type="button" className="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-sm hover:bg-accent transition-colors" onClick={() => { setReplyTarget(message); messageInputRef.current?.focus(); setOpenMenuMessageId(null) }}>
                                <CornerUpLeft className="h-3.5 w-3.5 text-muted-foreground" />Reply
                              </button>
                            ) : null}
                            {!message.deleted && mine ? (
                              <button type="button" className="flex w-full items-center gap-2.5 rounded-xl px-3 py-2 text-sm text-destructive hover:bg-accent transition-colors" onClick={() => void handleDeleteMessage(message)}>Delete</button>
                            ) : null}
                          </div>
                        </>
                      ) : null}
                    </div>
                  )
                })
              )}
            </div>
          </ScrollArea>

          {/* Scroll-to-bottom FAB */}
          {behindMessages > 0 ? (
            <div className="pointer-events-none absolute bottom-24 right-4 z-10 md:bottom-28 md:right-5">
              <button type="button" className="pointer-events-auto relative h-11 w-11 flex items-center justify-center rounded-full bg-primary text-primary-foreground shadow-lg hover:bg-primary/85 transition-colors touch-active animate-fade-in" onClick={scrollToFirstUnread} aria-label="Scroll to unread">
                <ChevronDown className="h-5 w-5" />
                {unreadCount > 0 ? (
                  <span className="absolute -top-1 -right-1 h-5 min-w-5 px-1 rounded-full bg-destructive text-destructive-foreground text-[10px] font-bold flex items-center justify-center">
                    {unreadCount}
                  </span>
                ) : null}
              </button>
            </div>
          ) : null}

          {/* Composer */}
          <div className="glass shrink-0 border-t border-border/30 px-3 py-3 md:px-4 pb-safe">
            <div className="flex w-full flex-col gap-2">
              <input ref={attachmentInputRef} type="file" multiple className="hidden" onChange={handleAttachmentSelection} />
              {replyTarget ? (
                <div className="flex items-start justify-between gap-2 rounded-2xl border border-border/50 bg-secondary/60 px-3.5 py-2.5 animate-fade-in">
                  <div className="min-w-0">
                    <p className="text-[11px] font-semibold text-primary">Replying to {senderLabel(replyTarget.sender_id)}</p>
                    <p className="truncate text-xs text-muted-foreground mt-0.5">
                      {replyTarget.attachments.length > 0 && (replyTarget.body ?? '').trim() === '📎' ? `[Attachment] ${replyTarget.attachments.length} file${replyTarget.attachments.length === 1 ? '' : 's'}` : previewText(replyTarget.deleted ? 'Message deleted' : replyTarget.body, 180)}
                    </p>
                  </div>
                  <button type="button" className="h-7 w-7 flex items-center justify-center rounded-lg text-muted-foreground hover:text-foreground hover:bg-accent transition-colors shrink-0" onClick={() => setReplyTarget(null)} aria-label="Cancel reply">
                    <X className="h-3.5 w-3.5" />
                  </button>
                </div>
              ) : null}
              {pendingAttachments.length > 0 ? (
                <div className="flex flex-wrap gap-2">
                  {pendingAttachments.map((file, index) => (
                    <PendingAttachmentChip key={`${file.name}-${file.size}-${index}`} file={file} onRemove={() => removePendingAttachment(index)} />
                  ))}
                </div>
              ) : null}
              <div className="flex items-end gap-2">
                <button type="button" className="h-11 w-11 shrink-0 flex items-center justify-center rounded-full bg-secondary/60 border border-border/40 text-muted-foreground hover:text-primary hover:bg-accent disabled:opacity-40 transition-all touch-active" disabled={busy || selectedChatId === null} onClick={() => attachmentInputRef.current?.click()} aria-label="Attach file">
                  <Paperclip className="h-[18px] w-[18px]" />
                </button>
                <Textarea
                  ref={messageInputRef}
                  rows={1}
                  className="min-h-[44px] max-h-44 flex-1 resize-none rounded-3xl border border-border/40 bg-secondary/50 focus-visible:ring-1 focus-visible:ring-primary/40 focus-visible:ring-offset-0 leading-5 px-4 py-3 text-sm"
                  value={draftMessage}
                  disabled={busy || selectedChatId === null}
                  onChange={(event) => { setDraftMessage(event.target.value); resizeComposerInput(event.target) }}
                  onKeyDown={(event) => { if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); void onSendMessage() } }}
                  placeholder={selectedChatId === null ? 'Select a chat first' : 'Message...'}
                />
                <button
                  type="button"
                  className={cn('h-11 w-11 shrink-0 flex items-center justify-center rounded-full transition-all touch-active', busy || selectedChatId === null || (draftMessage.trim().length === 0 && pendingAttachments.length === 0) ? 'bg-secondary/50 border border-border/40 text-muted-foreground/30 cursor-not-allowed' : 'bg-primary text-primary-foreground hover:opacity-85 active:scale-95')}
                  disabled={busy || selectedChatId === null || (draftMessage.trim().length === 0 && pendingAttachments.length === 0)}
                  onClick={onSendMessage}
                  aria-label="Send"
                >
                  <Send className="h-[18px] w-[18px]" />
                </button>
              </div>
              <p className="font-mono text-[11px] text-muted-foreground/50 truncate px-1">{status}</p>
            </div>
          </div>
        </section>

      {/* ── Mute menu ── */}
      {chatMuteMenu ? (
        <div className="fixed inset-0 z-40" onClick={() => setChatMuteMenu(null)} onContextMenu={(event) => event.preventDefault()}>
          <div className="fixed z-50 w-[272px] rounded-2xl border border-border/60 bg-card/95 p-4 shadow-2xl backdrop-blur-xl animate-fade-in" style={{ left: chatMuteMenu.x, top: chatMuteMenu.y }} onClick={(event) => event.stopPropagation()}>
            <p className="truncate text-sm font-semibold text-foreground">Mute {chatMuteMenu.username}</p>
            <p className="mt-0.5 text-xs text-muted-foreground">Silence notifications for a duration.</p>
            <div className="mt-4 grid grid-cols-4 gap-2">
              <button className="h-9 rounded-xl bg-secondary border border-border/40 text-xs font-medium text-foreground hover:bg-accent transition-colors touch-active" onClick={() => applyChatMuteFromMenu(15)}>15m</button>
              <button className="h-9 rounded-xl bg-secondary border border-border/40 text-xs font-medium text-foreground hover:bg-accent transition-colors touch-active" onClick={() => applyChatMuteFromMenu(60)}>1h</button>
              <button className="h-9 rounded-xl bg-secondary border border-border/40 text-xs font-medium text-foreground hover:bg-accent transition-colors touch-active" onClick={() => applyChatMuteFromMenu(480)}>8h</button>
              <button className="h-9 rounded-xl bg-secondary border border-border/40 text-xs font-medium text-foreground hover:bg-accent transition-colors touch-active" onClick={() => applyChatMuteFromMenu(1440)}>24h</button>
            </div>
            <div className="mt-3 flex items-center gap-2">
              <Input value={chatMuteMenu.customMinutes} onChange={(event) => setChatMuteMenu((previous) => previous ? { ...previous, customMinutes: event.target.value.replace(/[^0-9]/g, '') } : previous)} placeholder="Minutes" className="h-10 rounded-xl text-sm" />
              <button className="h-10 w-10 shrink-0 flex items-center justify-center rounded-xl bg-primary text-primary-foreground hover:bg-primary/85 transition-colors touch-active" onClick={() => { const m = Number.parseInt(chatMuteMenu.customMinutes || '0', 10); if (!Number.isFinite(m) || m <= 0) { setStatus('Duration must be at least 1 minute.'); return }; applyChatMuteFromMenu(m) }}>
                <Clock3 className="h-4 w-4" />
              </button>
            </div>
            <div className="mt-3 flex items-center justify-between gap-2 pt-3 border-t border-border/40">
              <button className="h-9 px-3 rounded-xl text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-colors" onClick={removeChatMuteFromMenu}>Unmute</button>
              <button className="h-9 px-3 rounded-xl text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-colors" onClick={() => setChatMuteMenu(null)}>Close</button>
            </div>
          </div>
        </div>
      ) : null}

      {/* ── Image preview ── */}
      {attachmentPreview ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4 animate-fade-in" onClick={closeAttachmentPreview}>
          <div className="flex max-h-[94vh] w-full max-w-4xl flex-col gap-3 rounded-3xl border border-border/50 bg-card/95 p-4 shadow-2xl" onClick={(event) => event.stopPropagation()}>
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <p className="truncate text-sm font-semibold text-foreground">{attachmentPreview.fileName}</p>
                <p className="font-mono text-xs text-muted-foreground">{attachmentPreview.mimeType}</p>
              </div>
              <div className="flex items-center gap-2 shrink-0">
                <button type="button" className="h-9 px-4 flex items-center gap-2 rounded-xl bg-secondary border border-border/50 text-sm hover:bg-accent transition-colors" onClick={() => { const a = document.createElement('a'); a.href = attachmentPreview.objectUrl; a.download = attachmentPreview.fileName; document.body.appendChild(a); a.click(); a.remove() }}>
                  <Download className="h-4 w-4" />Download
                </button>
                <button type="button" className="h-9 w-9 flex items-center justify-center rounded-xl text-muted-foreground hover:text-foreground hover:bg-accent transition-colors" onClick={closeAttachmentPreview} aria-label="Close">
                  <X className="h-5 w-5" />
                </button>
              </div>
            </div>
            <div className="flex min-h-[200px] flex-1 items-center justify-center overflow-auto rounded-2xl border border-border/40 bg-background/60 p-3">
              <img src={attachmentPreview.objectUrl} alt={attachmentPreview.fileName} className="max-h-[78vh] w-auto max-w-full object-contain rounded-xl" />
            </div>
          </div>
        </div>
      ) : null}
    </div>
  )
}

