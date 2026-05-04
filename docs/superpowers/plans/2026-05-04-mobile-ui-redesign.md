# Mobile UI Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign the Tauri/Next.js Android frontend with slide-out drawer navigation, a full 5-section settings page (including sessions viewer and mic test), FCM push notification registration, 429 error handling on sign-in, and a back button from the auth screen to the URL setup screen.

**Architecture:** All UI lives in `omnis-dashboard.tsx` (a single monolithic React component with screen-state routing). New API functions are added to `omnis.ts`. FCM on Android requires a Kotlin `FirebaseMessagingService`, Firebase SDK in Gradle, and a Tauri Rust command `get_fcm_token` that reads the token from a file written by the Kotlin service.

**Tech Stack:** Next.js 15, React, Tauri v2, TypeScript, Tailwind CSS, lucide-react; Kotlin for Android FCM service, Firebase Cloud Messaging SDK.

---

## File Map

| File | Change |
|------|--------|
| `omnis-tui/web/src/lib/omnis.ts` | Add `UserSession` type + `userSessionsList`, `userSessionsRevoke`, `userSessionsRevokeOther`, `fcmRegisterToken`, `fcmUnregisterToken` |
| `omnis-tui/web/src/components/omnis-dashboard.tsx` | Drawer nav, auth back button, 429 handling, settings overhaul, FCM wiring |
| `omnis-tui/src-tauri/src/lib.rs` | Add `get_fcm_token` command + register in `invoke_handler` |
| `omnis-tui/src-tauri/gen/android/build.gradle.kts` | Add `com.google.gms:google-services` classpath |
| `omnis-tui/src-tauri/gen/android/app/build.gradle.kts` | Apply `google-services` plugin + add Firebase BOM + FCM dependency |
| `omnis-tui/src-tauri/gen/android/app/src/main/AndroidManifest.xml` | Add FCM service registration + VIBRATE + POST_NOTIFICATIONS permissions |
| `omnis-tui/src-tauri/gen/android/app/src/main/java/com/omnis/desktop/OmnisFcmService.kt` | **New** — Firebase Messaging Service |
| `omnis-tui/src-tauri/gen/android/app/src/main/java/com/omnis/desktop/MainActivity.kt` | Proactively fetch + write FCM token on app start |
| `omnis-tui/.gitignore` | Exclude `google-services.json` |

---

## Task 1: API layer — sessions + FCM functions

**Files:**
- Modify: `omnis-tui/web/src/lib/omnis.ts` (end of file, after line 1458)

- [ ] **Step 1: Add `UserSession` type and sessions functions**

Append to the end of `omnis-tui/web/src/lib/omnis.ts`:

```typescript
export type UserSession = {
  id: number
  device_id: string
  user_agent: string | null
  last_accessed: string
  created_at: string
  expires_at: string
  current: boolean
}

export async function userSessionsList(): Promise<UserSession[]> {
  return requestJson<UserSession[]>('/users/sessions', { method: 'GET' }, true)
}

export async function userSessionsRevoke(sessionId: number): Promise<void> {
  return requestJson<void>(`/users/sessions/revoke/${sessionId}`, { method: 'DELETE' }, true)
}

export async function userSessionsRevokeOther(): Promise<void> {
  return requestJson<void>('/users/sessions/revoke_other', { method: 'DELETE' }, true)
}

export async function fcmRegisterToken(fcmToken: string): Promise<void> {
  return requestJson<void>(
    '/device/fcm/register',
    {
      method: 'POST',
      body: JSON.stringify({ fcm_token: fcmToken, platform: 'android' }),
    },
    true,
  )
}

export async function fcmUnregisterToken(): Promise<void> {
  return requestJson<void>('/device/fcm/current', { method: 'DELETE' }, true)
}
```

- [ ] **Step 2: Commit**

```bash
git add omnis-tui/web/src/lib/omnis.ts
git commit -m "feat: add sessions and FCM API functions to omnis.ts"
```

---

## Task 2: Auth screen — back button + 429 error handling

**Files:**
- Modify: `omnis-tui/web/src/components/omnis-dashboard.tsx`

**Context:** The auth screen renders at line ~2702. `onLogin()` at line ~2447 calls `runTask` which on error calls `setStatus(toErrorText(error))`. The error text from a 429 response will be "HTTP 429: Too many failed requests" or similar.

- [ ] **Step 1: Add 429-friendly error helper near the top of `onLogin`**

In `omnis-dashboard.tsx`, find the `onLogin` function body (around line 2447). Change:

```typescript
  async function onLogin() {
    await runTask(async () => {
      const username = loginUsername.trim()
      const password = loginPassword
      const session = await authLogin(username, password)
```

to:

```typescript
  async function onLogin() {
    await runTask(async () => {
      const username = loginUsername.trim()
      const password = loginPassword
      let session
      try {
        session = await authLogin(username, password)
      } catch (error) {
        const msg = toErrorText(error)
        if (msg.includes('429') || msg.toLowerCase().includes('too many')) {
          throw new Error('Too many failed attempts. Please wait a few minutes and try again.')
        }
        throw error
      }
```

- [ ] **Step 2: Add back button to the auth screen**

The auth screen JSX starts at line ~2703. Find the outer wrapper div and add a back button before the logo/brand section.

Find the block:
```tsx
        {/* Logo & brand */}
        <div className="flex flex-col items-center gap-4">
```

Replace with:
```tsx
          {/* Back to setup */}
          <button
            className="self-start flex items-center gap-1.5 text-xs text-muted-foreground/60 hover:text-foreground transition-colors mb-2"
            onClick={() => setScreen('setup')}
            disabled={busy}
          >
            <ArrowLeft className="h-3.5 w-3.5" strokeWidth={2} />
            Change server
          </button>

        {/* Logo & brand */}
        <div className="flex flex-col items-center gap-4">
```

- [ ] **Step 3: Commit**

```bash
git add omnis-tui/web/src/components/omnis-dashboard.tsx
git commit -m "fix: add back button on auth screen and friendly 429 error message"
```

---

## Task 3: Chat screen — slide-out drawer navigation

**Files:**
- Modify: `omnis-tui/web/src/components/omnis-dashboard.tsx`

**Context:** The chat layout renders at line ~2966 as a flex row `aside + section`. On mobile, the sidebar (`aside`) hides when `selectedChatId !== null` (line ~2972), and the section hides when `selectedChatId === null` (line ~3098). We're replacing this with a persistent full-screen chat + slide-out drawer overlay.

- [ ] **Step 1: Add `drawerOpen` state near the top of the state declarations**

Near the existing state declarations (around line 491), add after `const [desktopRuntime, setDesktopRuntime] = useState(false)`:

```typescript
  const [drawerOpen, setDrawerOpen] = useState(false)
```

- [ ] **Step 2: Close drawer when a chat is selected**

Inside the `useEffect` that watches `selectedChatId` (around line 630), add `setDrawerOpen(false)`:

```typescript
  useEffect(() => {
    selectedChatIdRef.current = selectedChatId
    setDrawerOpen(false)          // ← add this line
    setReplyTarget(null)
```

- [ ] **Step 3: Change the outer wrapper div to allow z-indexed overlay children**

Find:
```tsx
  return (
    <div className="flex h-full min-h-0 overflow-hidden bg-background">
```

Replace with:
```tsx
  return (
    <div className="relative flex h-full min-h-0 overflow-hidden bg-background">
```

- [ ] **Step 4: Add backdrop div as the first child of that outer div**

Immediately after the `<div className="relative flex h-full ...">` opening tag and before `{/* ── Sidebar ── */}`, insert:

```tsx
      {/* ── Drawer backdrop ── */}
      {drawerOpen && (
        <div
          className="absolute inset-0 z-20 bg-black/50 backdrop-blur-sm md:hidden"
          onClick={() => setDrawerOpen(false)}
          aria-hidden="true"
        />
      )}
```

- [ ] **Step 5: Change the aside element className (drawer positioning)**

Find:
```tsx
      <aside className={cn(
        'flex flex-col w-full md:w-[300px] lg:w-[320px] shrink-0 bg-card/40 border-r border-border/40',
        selectedChatId !== null && 'hidden md:flex'
      )}>
```

Replace with:
```tsx
      <aside className={cn(
        'absolute inset-y-0 left-0 z-30 flex flex-col w-[300px] bg-card border-r border-border/40 transition-transform duration-200',
        'md:relative md:translate-x-0 md:flex md:w-[300px] lg:w-[320px] md:shrink-0',
        drawerOpen ? 'translate-x-0 shadow-2xl' : '-translate-x-full'
      )}>
```

(Steps 3-5 are the replacements in the original Step 3 block; the aside content between the opening tag and `</aside>` is unchanged.)

- [ ] **Step 6: Make the section always full-width and update its className**

Find:
```tsx
      {/* ── Chat section ── */}
      <section className={cn('relative min-h-0 flex-1 flex-col bg-background', selectedChatId === null ? 'hidden md:flex' : 'flex')}>
```

Replace with:
```tsx
      {/* ── Chat section ── */}
      <section className="relative min-h-0 flex flex-col flex-1 bg-background">
```

- [ ] **Step 7: Add hamburger button to chat header (replaces the mobile back-to-list button)**

Find the chat header "Back" button (around line 3102):
```tsx
          <button
            className="h-9 w-9 flex md:hidden items-center justify-center rounded-xl text-muted-foreground hover:text-foreground hover:bg-accent transition-all shrink-0"
            onClick={() => setSelectedChatId(null)}
            aria-label="Back"
          >
            <ArrowLeft className="h-5 w-5" strokeWidth={1.75} />
          </button>
```

Replace with:
```tsx
          <button
            className="h-9 w-9 flex md:hidden items-center justify-center rounded-xl text-muted-foreground hover:text-foreground hover:bg-accent transition-all shrink-0"
            onClick={() => setDrawerOpen(true)}
            aria-label="Open chat list"
          >
            <Menu className="h-5 w-5" strokeWidth={1.75} />
          </button>
```

- [ ] **Step 8: Import `Menu` from lucide-react**

At the top of `omnis-dashboard.tsx`, add `Menu` to the lucide-react import list:

```typescript
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
  Menu,
  MessageCircle,
```

- [ ] **Step 9: Add "close drawer" button inside the drawer header (sidebar brand bar)**

Inside the sidebar, find the brand bar (around line 2976):
```tsx
        {/* ── Brand bar ── */}
        <div className="shrink-0 flex items-center justify-between px-4" style={{ paddingTop: 'max(16px, env(safe-area-inset-top))' }}>
```

Inside that div, after the settings button, add a close button visible only on mobile:
```tsx
            <button
              className="h-8 w-8 flex md:hidden items-center justify-center rounded-xl text-muted-foreground/60 hover:text-foreground hover:bg-accent transition-all"
              onClick={() => setDrawerOpen(false)}
              aria-label="Close menu"
            >
              <X className="h-4 w-4" strokeWidth={1.75} />
            </button>
```

- [ ] **Step 10: When a chat is selected from the drawer on mobile, close the drawer**

The chat selection button's onClick is already `() => setSelectedChatId(chat.chat_id)`. Since `useEffect` on `selectedChatId` now calls `setDrawerOpen(false)`, this is covered automatically by Step 2.

- [ ] **Step 11: Show "empty state" instead of hiding section when no chat selected**

Now that the section is always visible, we need to handle the no-chat state within the section's messages area. Find:

```tsx
            {activeChat === null ? (
              <div className="flex flex-col items-center justify-center gap-5 py-24 text-center">
```

Replace with (just adds a ☰ hint on mobile):
```tsx
            {activeChat === null ? (
              <div className="flex flex-col items-center justify-center gap-5 py-24 text-center">
                <button
                  className="md:hidden mb-2 flex items-center gap-2 rounded-xl border border-border/40 px-4 py-2.5 text-sm text-muted-foreground hover:bg-accent transition-all"
                  onClick={() => setDrawerOpen(true)}
                >
                  <Menu className="h-4 w-4" strokeWidth={1.75} />
                  Open chats
                </button>
```

Close the new button just before the existing children of the empty state div.

- [ ] **Step 12: Commit**

```bash
git add omnis-tui/web/src/components/omnis-dashboard.tsx
git commit -m "feat: replace sidebar with slide-out drawer navigation for Android"
```

---

## Task 4: Settings page — full 5-section redesign

**Files:**
- Modify: `omnis-tui/web/src/components/omnis-dashboard.tsx`

**Context:** The existing settings screen renders at line ~2821. It has server diagnostics, notifications, and muted chats. We're replacing it with 5 named sections. The existing logic functions (`runServerDiagnostics`, `toggleAlerts`, etc.) stay; we add sessions state + `loadSessions()` + `runMicTest()`.

- [ ] **Step 1: Add new state declarations for sessions and mic test**

After the existing `const [serverDiagnostics, setServerDiagnostics]` state (around line 531), add:

```typescript
  const [sessions, setSessions] = useState<UserSession[]>([])
  const [sessionsLoading, setSessionsLoading] = useState(false)
  const [micTestActive, setMicTestActive] = useState(false)
  const [micLevel, setMicLevel] = useState(0)
  const micStreamRef = useRef<MediaStream | null>(null)
  const micAnimFrameRef = useRef<number | null>(null)
```

- [ ] **Step 2: Import `UserSession` from omnis.ts**

Add `userSessionsList`, `userSessionsRevoke`, `userSessionsRevokeOther`, and `UserSession` to the import from `@/lib/omnis`:

```typescript
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
  userSessionsList,
  userSessionsRevoke,
  userSessionsRevokeOther,
  type AuthRuntime,
  type BackendConfig,
  type CallHistoryEntry,
  type ChatMessage,
  type ChatSummary,
  type HealthResponse,
  type MediaAttachment,
  type UserSession,
} from '@/lib/omnis'
```

- [ ] **Step 3: Add `loadSessions` function**

After the `runServerDiagnostics` function (around line 1981), add:

```typescript
  async function loadSessions() {
    setSessionsLoading(true)
    try {
      const list = await userSessionsList()
      setSessions(list)
    } catch (error) {
      setStatus(toErrorText(error))
    } finally {
      setSessionsLoading(false)
    }
  }

  async function onRevokeSession(sessionId: number) {
    await runTask(async () => {
      await userSessionsRevoke(sessionId)
      setStatus('Session revoked.')
      await loadSessions()
    })
  }

  async function onRevokeOtherSessions() {
    await runTask(async () => {
      await userSessionsRevokeOther()
      setStatus('All other sessions revoked.')
      await loadSessions()
    })
  }

  function clearLocalData() {
    if (typeof window === 'undefined') return
    window.localStorage.clear()
    setStatus('Local data cleared. Please restart the app.')
    setScreen('setup')
  }
```

- [ ] **Step 4: Add `runMicTest` and `stopMicTest` functions**

After `clearLocalData`, add:

```typescript
  async function runMicTest() {
    if (micTestActive) {
      stopMicTest()
      return
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
      micStreamRef.current = stream
      setMicTestActive(true)

      const ctx = new AudioContext()
      const source = ctx.createMediaStreamSource(stream)
      const analyser = ctx.createAnalyser()
      analyser.fftSize = 256
      source.connect(analyser)
      const data = new Uint8Array(analyser.frequencyBinCount)

      const tick = () => {
        analyser.getByteFrequencyData(data)
        const avg = data.reduce((s, v) => s + v, 0) / data.length
        setMicLevel(Math.min(100, Math.round((avg / 128) * 100)))
        micAnimFrameRef.current = requestAnimationFrame(tick)
      }
      tick()
    } catch {
      setStatus('Microphone access denied or unavailable.')
    }
  }

  function stopMicTest() {
    if (micAnimFrameRef.current !== null) {
      cancelAnimationFrame(micAnimFrameRef.current)
      micAnimFrameRef.current = null
    }
    if (micStreamRef.current) {
      for (const track of micStreamRef.current.getTracks()) {
        track.stop()
      }
      micStreamRef.current = null
    }
    setMicTestActive(false)
    setMicLevel(0)
  }
```

- [ ] **Step 5: Stop mic test when leaving settings screen**

In the useEffect or handler that navigates away from settings (in the header back button), call `stopMicTest()`:

Find the settings back button:
```tsx
            onClick={() => setScreen('chat')}
```

Replace with:
```tsx
            onClick={() => { stopMicTest(); setScreen('chat') }}
```

Also add a `useEffect` to stop the mic test when the screen changes:
```typescript
  useEffect(() => {
    if (screen !== 'settings') {
      stopMicTest()
    }
  }, [screen])
```

- [ ] **Step 6: Replace the settings screen JSX**

Replace the entire `if (screen === 'settings') { return (...) }` block (from `if (screen === 'settings')` at line ~2821 to its closing `}`) with the following:

```tsx
  if (screen === 'settings') {
    return (
      <div className="flex h-full min-h-0 flex-col bg-background">
        {/* Header */}
        <header className="glass shrink-0 flex items-center justify-between px-4 py-3 pt-safe">
          <div className="flex items-center gap-3">
            <button
              className="h-9 w-9 flex items-center justify-center rounded-xl text-muted-foreground hover:text-foreground hover:bg-accent transition-all"
              disabled={busy}
              onClick={() => { stopMicTest(); setScreen('chat') }}
              aria-label="Back"
            >
              <ArrowLeft className="h-5 w-5" strokeWidth={1.75} />
            </button>
            <h1 className="font-syne text-lg font-bold tracking-wide">Settings</h1>
          </div>
        </header>

        <ScrollArea className="min-h-0 flex-1" viewportClassName="p-4 space-y-3">
          <div className="max-w-2xl mx-auto space-y-3">

            {/* ── ACCOUNT ── */}
            <div className="rounded-2xl border border-border/50 bg-card p-5 space-y-3">
              <h2 className="text-xs font-semibold tracking-[0.14em] uppercase text-muted-foreground/60">Account</h2>
              {currentUser ? (
                <div className="flex items-center gap-3 rounded-xl bg-secondary/40 px-3.5 py-3">
                  <div className={cn('h-10 w-10 rounded-full border-2 flex items-center justify-center text-sm font-bold shrink-0', usernameColorClass(currentUser.username))}>
                    {getInitials(currentUser.username)}
                  </div>
                  <div className="min-w-0 flex-1">
                    <p className="text-sm font-semibold truncate">@{currentUser.username}</p>
                    <p className="font-mono text-[10px] text-muted-foreground/50 mt-0.5">Signed in</p>
                  </div>
                </div>
              ) : null}
              <button
                className="w-full h-10 rounded-xl border border-destructive/30 bg-destructive/5 text-sm text-destructive hover:bg-destructive/10 transition-all flex items-center justify-center gap-2 disabled:opacity-40"
                disabled={busy}
                onClick={onLogout}
              >
                <LogOut className="h-4 w-4" strokeWidth={1.75} />
                Sign out
              </button>
            </div>

            {/* ── SERVER ── */}
            <div className="rounded-2xl border border-border/50 bg-card p-5 space-y-3">
              <h2 className="text-xs font-semibold tracking-[0.14em] uppercase text-muted-foreground/60">Server</h2>
              <div className="rounded-xl bg-input/50 border border-border/40 px-3 py-2.5 font-mono text-xs text-muted-foreground/70 truncate">
                {config?.backendUrl ?? runtime?.backendUrl ?? setupUrl}
              </div>
              <div className="grid grid-cols-2 gap-2">
                <button
                  disabled={busy}
                  onClick={() => { stopMicTest(); setScreen('setup') }}
                  className="h-10 rounded-xl border border-border/40 bg-secondary/60 text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-all flex items-center justify-center gap-2 disabled:opacity-40"
                >
                  <Server className="h-4 w-4" strokeWidth={1.75} />
                  Edit URL
                </button>
                <button
                  disabled={busy}
                  onClick={() => void runServerDiagnostics()}
                  className="h-10 rounded-xl border border-border/40 bg-secondary/60 text-sm text-muted-foreground hover:text-foreground hover:bg-accent transition-all flex items-center justify-center gap-2 disabled:opacity-40"
                >
                  <Activity className="h-4 w-4" strokeWidth={1.75} />
                  Diagnostics
                </button>
              </div>
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

            {/* ── NOTIFICATIONS ── */}
            <div className="rounded-2xl border border-border/50 bg-card p-5 space-y-3">
              <h2 className="text-xs font-semibold tracking-[0.14em] uppercase text-muted-foreground/60">Notifications</h2>
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
              {activeMutedChats.length > 0 ? (
                <div className="space-y-2">
                  <p className="text-xs text-muted-foreground/50 font-medium">Muted chats</p>
                  {activeMutedChats.map((entry) => (
                    <div key={entry.chatId} className="flex items-center justify-between gap-3 rounded-xl border border-border/40 bg-secondary/50 px-3 py-2.5">
                      <div className="min-w-0">
                        <p className="truncate text-sm font-medium text-foreground">{entry.username}</p>
                        <p className="font-mono text-xs text-muted-foreground/55 mt-0.5">{formatDurationMinutes(entry.remainingMinutes)} remaining</p>
                      </div>
                      <button
                        className="h-7 px-3 rounded-lg text-xs border border-border/40 text-muted-foreground hover:text-foreground hover:bg-accent transition-all"
                        onClick={() => clearMute(entry.chatId)}
                      >
                        Unmute
                      </button>
                    </div>
                  ))}
                </div>
              ) : null}
            </div>

            {/* ── PRIVACY & SECURITY ── */}
            <div className="rounded-2xl border border-border/50 bg-card p-5 space-y-3">
              <h2 className="text-xs font-semibold tracking-[0.14em] uppercase text-muted-foreground/60">Privacy &amp; Security</h2>
              <div className="rounded-xl bg-input/40 border border-border/30 px-3 py-3 font-mono text-xs space-y-1.5">
                <div className="flex justify-between"><span className="text-muted-foreground/60">Encryption</span><span>E2EE</span></div>
                <div className="flex justify-between"><span className="text-muted-foreground/60">Key exchange</span><span>P-384 ECDH</span></div>
                <div className="flex justify-between"><span className="text-muted-foreground/60">Cipher</span><span>AES-256-GCM</span></div>
              </div>

              {/* Sessions */}
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <p className="text-xs text-muted-foreground/50 font-medium">Active sessions</p>
                  <button
                    className="text-xs text-primary/70 hover:text-primary transition-colors"
                    disabled={sessionsLoading}
                    onClick={() => void loadSessions()}
                  >
                    {sessionsLoading ? 'Loading…' : 'Refresh'}
                  </button>
                </div>
                {sessions.length === 0 && !sessionsLoading ? (
                  <div className="rounded-xl border border-dashed border-border/40 px-4 py-6 text-center text-xs text-muted-foreground/50">
                    Tap Refresh to load sessions.
                  </div>
                ) : (
                  <div className="space-y-2">
                    {sessions.map((session) => (
                      <div key={session.id} className="flex items-start justify-between gap-3 rounded-xl border border-border/40 bg-secondary/50 px-3 py-2.5">
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-2">
                            <p className="font-mono text-xs text-foreground truncate">{session.device_id.slice(0, 12)}…</p>
                            {session.current ? (
                              <span className="rounded-md border border-primary/25 bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary/80">current</span>
                            ) : null}
                          </div>
                          <p className="font-mono text-[10px] text-muted-foreground/50 mt-0.5">
                            Last seen {new Date(session.last_accessed).toLocaleDateString()}
                          </p>
                          {session.user_agent ? (
                            <p className="text-[10px] text-muted-foreground/40 mt-0.5 truncate">{session.user_agent}</p>
                          ) : null}
                        </div>
                        {!session.current ? (
                          <button
                            className="h-7 shrink-0 px-2.5 rounded-lg text-xs border border-destructive/25 text-destructive/70 hover:bg-destructive/10 transition-all"
                            disabled={busy}
                            onClick={() => void onRevokeSession(session.id)}
                          >
                            Revoke
                          </button>
                        ) : null}
                      </div>
                    ))}
                    {sessions.some((s) => !s.current) ? (
                      <button
                        className="w-full h-9 rounded-xl border border-destructive/25 bg-destructive/5 text-xs text-destructive/80 hover:bg-destructive/10 transition-all"
                        disabled={busy}
                        onClick={() => void onRevokeOtherSessions()}
                      >
                        Revoke all other sessions
                      </button>
                    ) : null}
                  </div>
                )}
              </div>

              <button
                className="w-full h-10 rounded-xl border border-destructive/30 bg-destructive/5 text-sm text-destructive/80 hover:bg-destructive/10 transition-all flex items-center justify-center gap-2"
                onClick={clearLocalData}
              >
                <X className="h-4 w-4" strokeWidth={1.75} />
                Clear local data
              </button>
            </div>

            {/* ── APP ── */}
            <div className="rounded-2xl border border-border/50 bg-card p-5 space-y-3">
              <h2 className="text-xs font-semibold tracking-[0.14em] uppercase text-muted-foreground/60">App</h2>
              <div className="rounded-xl bg-input/40 border border-border/30 px-3 py-2.5 font-mono text-xs flex justify-between">
                <span className="text-muted-foreground/60">Version</span>
                <span>{health?.version ?? '—'}</span>
              </div>
              <div className="rounded-xl bg-input/40 border border-border/30 px-3 py-2.5 font-mono text-xs flex justify-between">
                <span className="text-muted-foreground/60">Runtime</span>
                <span>{desktopRuntime ? 'Tauri' : 'Browser'}</span>
              </div>

              {/* Mic test */}
              <div className="space-y-2">
                <p className="text-xs text-muted-foreground/50 font-medium">Microphone test</p>
                <button
                  className={cn('w-full h-10 rounded-xl text-sm font-medium transition-all flex items-center justify-center gap-2',
                    micTestActive
                      ? 'bg-destructive/10 border border-destructive/30 text-destructive'
                      : 'border border-border/40 bg-secondary/60 text-muted-foreground hover:text-foreground hover:bg-accent'
                  )}
                  onClick={() => void runMicTest()}
                >
                  {micTestActive ? 'Stop mic test' : 'Start mic test'}
                </button>
                {micTestActive ? (
                  <div className="rounded-xl border border-border/40 bg-input/40 px-3 py-2.5 space-y-1.5">
                    <div className="flex justify-between font-mono text-xs">
                      <span className="text-muted-foreground/60">Level</span>
                      <span>{micLevel}%</span>
                    </div>
                    <div className="h-2 rounded-full bg-secondary overflow-hidden">
                      <div
                        className="h-full rounded-full bg-primary transition-all duration-75"
                        style={{ width: `${micLevel}%` }}
                      />
                    </div>
                  </div>
                ) : null}
              </div>
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
```

- [ ] **Step 7: Commit**

```bash
git add omnis-tui/web/src/components/omnis-dashboard.tsx
git commit -m "feat: overhaul settings page with 5 sections, sessions viewer, and mic test"
```

---

## Task 5: FCM Android native setup

**Files:**
- Modify: `omnis-tui/src-tauri/gen/android/build.gradle.kts`
- Modify: `omnis-tui/src-tauri/gen/android/app/build.gradle.kts`
- Modify: `omnis-tui/src-tauri/gen/android/app/src/main/AndroidManifest.xml`
- Create: `omnis-tui/src-tauri/gen/android/app/src/main/java/com/omnis/desktop/OmnisFcmService.kt`
- Modify: `omnis-tui/src-tauri/gen/android/app/src/main/java/com/omnis/desktop/MainActivity.kt`
- Modify: `omnis-tui/.gitignore`

**Prerequisite:** The user must place `google-services.json` (downloaded from Firebase Console) in `omnis-tui/src-tauri/gen/android/app/` before building. This file is gitignored.

- [ ] **Step 1: Add google-services.json to .gitignore**

Open `omnis-tui/.gitignore` (or the repo root `.gitignore`) and add:

```
# Firebase config — must be placed manually before building
**/google-services.json
```

- [ ] **Step 2: Add google-services classpath to project-level build.gradle.kts**

In `omnis-tui/src-tauri/gen/android/build.gradle.kts`, change:

```kotlin
buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:8.11.0")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:1.9.25")
    }
}
```

to:

```kotlin
buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:8.11.0")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:1.9.25")
        classpath("com.google.gms:google-services:4.4.2")
    }
}
```

- [ ] **Step 3: Apply google-services plugin and add FCM dependency to app/build.gradle.kts**

In `omnis-tui/src-tauri/gen/android/app/build.gradle.kts`, change the `plugins` block:

```kotlin
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}
```

to:

```kotlin
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
    id("com.google.gms.google-services")
}
```

And change the `dependencies` block:

```kotlin
dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}
```

to:

```kotlin
dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation(platform("com.google.firebase:firebase-bom:33.7.0"))
    implementation("com.google.firebase:firebase-messaging-ktx")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}
```

- [ ] **Step 4: Create `OmnisFcmService.kt`**

Create file `omnis-tui/src-tauri/gen/android/app/src/main/java/com/omnis/desktop/OmnisFcmService.kt`:

```kotlin
package com.omnis.desktop

import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import org.json.JSONObject
import java.io.File

class OmnisFcmService : FirebaseMessagingService() {

    override fun onNewToken(token: String) {
        super.onNewToken(token)
        saveToken(token)
    }

    override fun onMessageReceived(remoteMessage: RemoteMessage) {
        // Server sends wake-only pushes (no payload content).
        // The app polls for new messages on resume; nothing to do here.
    }

    private fun saveToken(token: String) {
        try {
            val file = File(filesDir, "fcm_token.json")
            file.writeText(JSONObject().put("token", token).toString())
        } catch (_: Exception) {
            // best effort
        }
    }
}
```

- [ ] **Step 5: Register FCM service in AndroidManifest.xml**

In `AndroidManifest.xml`, add the POST_NOTIFICATIONS permission and the FCM service inside `<application>`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
    <uses-permission android:name="android.permission.INTERNET" />
    <uses-permission android:name="android.permission.VIBRATE" />
    <uses-permission android:name="android.permission.POST_NOTIFICATIONS" />

    <!-- AndroidTV support -->
    <uses-feature android:name="android.software.leanback" android:required="false" />

    <application
        android:icon="@mipmap/ic_launcher"
        android:label="@string/app_name"
        android:theme="@style/Theme.omnis_desktop"
        android:usesCleartextTraffic="${usesCleartextTraffic}">
        <activity
            android:configChanges="orientation|keyboardHidden|keyboard|screenSize|locale|smallestScreenSize|screenLayout|uiMode"
            android:launchMode="singleTask"
            android:label="@string/main_activity_title"
            android:name=".MainActivity"
            android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
                <!-- AndroidTV support -->
                <category android:name="android.intent.category.LEANBACK_LAUNCHER" />
            </intent-filter>
        </activity>

        <service
            android:name=".OmnisFcmService"
            android:exported="false">
            <intent-filter>
                <action android:name="com.google.firebase.MESSAGING_EVENT" />
            </intent-filter>
        </service>

        <provider
          android:name="androidx.core.content.FileProvider"
          android:authorities="${applicationId}.fileprovider"
          android:exported="false"
          android:grantUriPermissions="true">
          <meta-data
            android:name="android.support.FILE_PROVIDER_PATHS"
            android:resource="@xml/file_paths" />
        </provider>
    </application>
</manifest>
```

- [ ] **Step 6: Update MainActivity.kt to proactively fetch and save the FCM token**

Replace the entire content of `MainActivity.kt`:

```kotlin
package com.omnis.desktop

import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import com.google.firebase.messaging.FirebaseMessaging
import org.json.JSONObject
import java.io.File

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    fetchAndSaveFcmToken()
  }

  private fun fetchAndSaveFcmToken() {
    FirebaseMessaging.getInstance().token.addOnCompleteListener { task ->
      if (!task.isSuccessful) return@addOnCompleteListener
      val token = task.result ?: return@addOnCompleteListener
      try {
        val file = File(filesDir, "fcm_token.json")
        file.writeText(JSONObject().put("token", token).toString())
      } catch (_: Exception) {
        // best effort
      }
    }
  }
}
```

- [ ] **Step 7: Commit**

```bash
git add omnis-tui/src-tauri/gen/android/build.gradle.kts
git add omnis-tui/src-tauri/gen/android/app/build.gradle.kts
git add omnis-tui/src-tauri/gen/android/app/src/main/AndroidManifest.xml
git add "omnis-tui/src-tauri/gen/android/app/src/main/java/com/omnis/desktop/OmnisFcmService.kt"
git add "omnis-tui/src-tauri/gen/android/app/src/main/java/com/omnis/desktop/MainActivity.kt"
git add omnis-tui/.gitignore
git commit -m "feat: add Firebase FCM service and Android native token setup"
```

---

## Task 6: FCM — Rust Tauri command + frontend wiring

**Files:**
- Modify: `omnis-tui/src-tauri/src/lib.rs`
- Modify: `omnis-tui/web/src/components/omnis-dashboard.tsx`

### Subtask 6a: Rust `get_fcm_token` command

- [ ] **Step 1: Add `get_fcm_token` command in lib.rs**

After the `sessions_revoke_other` command (around line 911), add:

```rust
#[tauri::command]
fn get_fcm_token(app: tauri::AppHandle) -> Result<Option<String>, String> {
  let data_dir = app
    .path()
    .app_data_dir()
    .map_err(|e| format!("could not resolve app data dir: {e}"))?;

  let token_path = data_dir.join("fcm_token.json");
  if !token_path.exists() {
    return Ok(None);
  }

  let raw = std::fs::read_to_string(&token_path)
    .map_err(|e| format!("failed to read fcm token file: {e}"))?;

  #[derive(serde::Deserialize)]
  struct TokenFile {
    token: String,
  }

  let parsed: TokenFile = serde_json::from_str(&raw)
    .map_err(|e| format!("invalid fcm token file: {e}"))?;

  Ok(Some(parsed.token))
}
```

- [ ] **Step 2: Register `get_fcm_token` in the invoke_handler**

In the `tauri::generate_handler!` macro (around line 1042), add `get_fcm_token`:

```rust
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
      get_fcm_token,
      media_upload_chunk,
      media_get_meta,
      media_download
    ])
```

- [ ] **Step 3: Commit**

```bash
git add omnis-tui/src-tauri/src/lib.rs
git commit -m "feat: add get_fcm_token Tauri command to expose Android FCM token"
```

### Subtask 6b: Frontend — register FCM after login, unregister on logout

- [ ] **Step 4: Add `registerFcmIfAndroid` helper to omnis.ts**

Append to `omnis-tui/web/src/lib/omnis.ts`:

```typescript
export async function registerFcmIfAndroid(): Promise<void> {
  if (!isTauriRuntime()) {
    return
  }

  try {
    const token = await invokeTauri<string | null>('get_fcm_token')
    if (!token) {
      return
    }
    await fcmRegisterToken(token)
  } catch {
    // best effort — FCM registration failure should not block login
  }
}
```

- [ ] **Step 5: Import `registerFcmIfAndroid` in omnis-dashboard.tsx**

Add `registerFcmIfAndroid` to the import from `@/lib/omnis` (add it alongside the other function imports).

- [ ] **Step 6: Call `registerFcmIfAndroid` after successful login**

In `onLogin()` function, after `await loadChats(true)`, add:

```typescript
      void registerFcmIfAndroid()
```

So `onLogin` ends like:
```typescript
  async function onLogin() {
    await runTask(async () => {
      // ... existing code ...
      setScreen('chat')
      setStatus(`Logged in as ${session.username}.`)
      await loadChats(true)
      void registerFcmIfAndroid()
    })
  }
```

Do the same in `onSignup()` after `await loadChats(true)`.

And in `tryAutoLoginFromSavedCredentials()` after `await enterChatSession(...)`:
```typescript
      void registerFcmIfAndroid()
      return true
```

- [ ] **Step 7: Unregister FCM on logout**

In `onLogout()`, after `await authLogout()` and before `clearSavedCredentials()`, add:

```typescript
      if (isTauriRuntime()) {
        try {
          await fcmUnregisterToken()
        } catch {
          // best effort
        }
      }
```

Import `fcmUnregisterToken` alongside `registerFcmIfAndroid` from `@/lib/omnis`.

- [ ] **Step 8: Commit**

```bash
git add omnis-tui/web/src/lib/omnis.ts
git add omnis-tui/web/src/components/omnis-dashboard.tsx
git commit -m "feat: register/unregister FCM token on login/logout"
```

---

## Task 7: Build verification

- [ ] **Step 1: Run TypeScript type check**

```bash
cd omnis-tui/web && npx tsc --noEmit
```

Expected: no errors.

- [ ] **Step 2: Verify dev server starts**

```bash
cd omnis-tui/web && npm run dev
```

Expected: server starts at `http://localhost:3000` with no build errors.

- [ ] **Step 3: Manual check — auth screen**

Open the app or dev server. Navigate to auth screen. Verify:
- "← Change server" button appears at the top
- Clicking it navigates back to the URL setup screen

- [ ] **Step 4: Manual check — drawer navigation**

Log in, navigate to chat screen. Verify:
- Hamburger ☰ button appears in the top-left of the chat header
- Tapping it slides in the chat list from the left
- Tapping outside the drawer (backdrop) closes it
- Selecting a chat closes the drawer and shows the chat

- [ ] **Step 5: Manual check — settings page**

Navigate to Settings. Verify all 5 sections render: Account, Server, Notifications, Privacy & Security, App. Verify:
- Mic test works (level bar animates when speaking)
- Sessions "Refresh" button loads sessions
- "Sign out" from Account section logs out

- [ ] **Step 6: Build Android APK using Docker**

```bash
cd omnis-tui
docker compose build android && docker compose run android
```

Expected: `./out/android/` contains an APK.

**Note:** `google-services.json` must be placed at `omnis-tui/src-tauri/gen/android/app/google-services.json` before building. Copy it from your Firebase Console project settings.

---

## Notes

**FCM `google-services.json` placement:**
```
omnis-tui/src-tauri/gen/android/app/google-services.json
```
Download from Firebase Console → Project settings → Your apps → Android app → Download `google-services.json`. This file is gitignored and must be placed manually before each Docker build.

**Mute right-click menu:** The existing `onContextMenu` handler on chat list items still works on desktop. On Android, long-press triggers `contextmenu` events in the WebView, so muting still works the same way.

**Session revoke note:** The `current` session cannot be revoked from the UI (no Revoke button shown for it). Revoke all others is available when there are non-current sessions.
