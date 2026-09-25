# workin-desktop — Implementation Plan

## What this is and why it exists

The web app at `workin.kiwimi.co` already works well for most things. But the browser is a fundamental constraint for file storage:

- Every uploaded byte travels **browser → backend server → Cloudflare R2**. The backend is a bottleneck you pay for in RAM, bandwidth, and timeout budget.
- Nginx/proxy timeouts kill uploads that take longer than ~60–90 seconds.
- Browsers buffer large files in RAM — a 2 GB file can crash the tab.
- No resume. Drop the connection, start over from zero.
- No background. Close the tab, the upload dies.

The desktop app solves all of this with one architectural change: **file bytes never touch the backend server**. The app talks to the backend only to authenticate and get presigned URLs, then uploads directly from the user's machine to Cloudflare R2 at full internet speed. The backend records only metadata in Firestore after completion. The file appears on the web app instantly.

This is not a separate product. It is a power client for the same system — same files, same Firestore database, same R2 bucket, same users.

---

## Goals

1. Upload files and folders of any size (tested to 50 GB+) directly to R2, bypassing the backend server entirely.
2. Files uploaded from the desktop appear on the web app immediately.
3. Files added/deleted/renamed on the web appear in the desktop file browser.
4. Background sync of a user-selected local folder (opt-in per folder).
5. Resumable transfers — a dropped connection retries only the failed chunk, not the whole file.
6. Windows `.exe` installer, ~8 MB, no Chromium bundled, runs on Windows 10/11.
7. System tray presence — sync runs in the background with status notifications.

---

## Why Tauri (not Electron)

| | Electron | Tauri |
|---|---|---|
| Installer size | 150–200 MB (ships Chromium) | 4–8 MB (uses OS WebView2, already on Win 10+) |
| RAM usage at idle | 200–400 MB | 30–80 MB |
| UI language | React/TS | React/TS (same as your web app) |
| Native backend | Node.js | Rust |
| File system access | Node `fs` | Rust `std::fs` + `notify` crate |
| OS keychain | `keytar` (frequently breaks) | Tauri `keyring` plugin (native Win Credential Manager) |
| Auto-update | `electron-updater` | Tauri built-in updater |
| Multipart upload | JS (fine) | Rust (parallel, streaming, non-blocking) |
| System tray | Electron tray API | Tauri tray plugin (native) |

Tauri wins on install size, RAM, and the native file watcher. The UI is React/TypeScript so you can copy components directly from the web app.

---

## Tech Stack

| Layer | Technology | Reason |
|---|---|---|
| UI | React 18 + TypeScript | Same as web app — reuse components, API client, design tokens |
| Styling | Tailwind CSS v4 + shadcn/ui | Identical to web — copy components directly |
| State management | Zustand | Lightweight, works well with Tauri events |
| Desktop shell | Tauri 2.x | Small binary, native OS APIs, Rust backend |
| Native backend | Rust | File watcher, multipart upload engine, OS keychain, tray |
| File watcher | `notify` crate (Rust) | Cross-platform — ReadDirectoryChangesW on Windows |
| HTTP client | `reqwest` crate (Rust) | Async, streaming, used for chunk uploads to R2 |
| Auth storage | Tauri `keyring` plugin | Stores JWT in Windows Credential Manager — never plaintext on disk |
| State bridge | Tauri commands + events | Rust emits typed events; React listens via `listen()` |
| Build / package | Tauri CLI + NSIS | Produces signed `.exe` and `.msi` installers |

---

## Project Structure

```
workin-desktop/
├── src-tauri/                      # Rust — native backend
│   ├── Cargo.toml
│   ├── tauri.conf.json             # app config, permissions, updater URL
│   ├── icons/                      # app icon (tray + taskbar)
│   └── src/
│       ├── main.rs                 # Tauri entry, command + event registration
│       ├── state.rs                # AppState (Arc<Mutex<State>>) shared across commands
│       ├── auth.rs                 # Login, token refresh, keychain read/write
│       ├── upload.rs               # Multipart upload engine (chunk split, parallel PUT, retry)
│       ├── download.rs             # Presigned GET, streaming write to disk
│       ├── sync.rs                 # Delta sync loop — polls /files/sync, resolves changes
│       ├── watcher.rs              # Local folder watcher (notify crate → upload queue)
│       ├── conflict.rs             # Conflict detection and resolution logic
│       ├── tray.rs                 # System tray icon, menu, notifications
│       └── error.rs                # Unified error type, serialised to React
│
└── src/                            # React/TypeScript — UI
    ├── main.tsx                    # Tauri bootstrap
    ├── App.tsx                     # Router: Setup | Main
    ├── pages/
    │   ├── Setup.tsx               # First-run: server URL + login form
    │   ├── Files.tsx               # File browser (mirrors /files on web)
    │   ├── Transfers.tsx           # Active + completed transfer queue
    │   └── Settings.tsx            # Watched folders, account, preferences
    ├── components/
    │   ├── ui/                     # shadcn/ui (copied from web app)
    │   ├── FileTree.tsx            # Folder tree with sync status overlays
    │   ├── TransferItem.tsx        # Upload/download row — progress bar, speed, ETA
    │   ├── SyncBadge.tsx           # Synced / syncing / conflict / error badge
    │   └── ConflictDialog.tsx      # Modal shown when a conflict needs user decision
    ├── lib/
    │   ├── api.ts                  # HTTP API client (same shape as web app)
    │   ├── commands.ts             # Typed wrappers for all Rust #[tauri::command] calls
    │   └── store.ts                # Zustand — transfers queue, sync state, watched folders
    └── hooks/
        ├── useTransfers.ts         # Subscribe to upload/download progress Tauri events
        ├── useSyncStatus.ts        # Subscribe to sync state change events
        └── useConflicts.ts         # Subscribe to conflict events, trigger resolution UI
```

---

## How the data flows

### Upload (any file size)

```
User drops file/folder into app (or watcher detects new file)
        │
        ▼
React → Rust command: upload_file(local_path, remote_path)
        │
        ▼  upload.rs
If file < 10 MB:
  GET /api/v1/files/presign/put?path=...
  → single presigned PUT URL
  Rust PUT bytes directly to R2
  → done, go to metadata step

If file ≥ 10 MB (multipart):
  POST /api/v1/files/presign/multipart/start  { path, size, content_type }
  → { upload_id, chunk_urls: [ url_1..url_N ] }   (5 MB chunks)
        │
        ▼
  Rust spawns 4 async tasks, each uploading one chunk
  Each chunk: PUT directly to R2 presigned URL
  On chunk failure: retry up to 3× with exponential backoff (1s, 2s, 4s)
  On session expiry (upload_id invalid): abort + restart from chunk 0
  Progress events → React every chunk completion: { path, bytes_done, total }
        │
        ▼  (all chunks complete)
  POST /api/v1/files/presign/multipart/complete { upload_id, path, etags }
  → backend calls R2 CompleteMultipartUpload
  → backend writes Firestore file_record { path, size, updated_at, owner_uid }
        │
        ▼
Tauri event → React: transfer_complete { path }
File appears instantly on web app
```

### Error taxonomy for uploads

| Error | Retryable | Action |
|---|---|---|
| Chunk PUT → 5xx (R2 error) | Yes | Retry chunk up to 3× |
| Chunk PUT → 400 EntityTooSmall | No | Abort multipart, re-split with larger chunk size |
| Chunk PUT → 403 (presign expired) | Yes | Request new presigned URLs for remaining chunks |
| Network timeout / connection reset | Yes | Retry chunk from byte 0 of that chunk |
| `complete` → 400 InvalidPart | No | Abort and restart upload entirely |
| Firestore write fails (backend 5xx) | Yes | Retry `complete` call up to 3× |
| Disk full on download | No | Notify user, cancel transfer |

### Download

```
User clicks Download in file browser
        │
        ▼
React → Rust command: download_file(remote_path, local_path)
        │
        ▼  download.rs
GET /api/v1/files/presign/download?path=...
  → presigned GET URL (30-min expiry)
        │
        ▼
Rust streams bytes from R2 → local disk (4 MB buffer)
Progress event → React every 256 KB: { path, bytes_done, total }
On network drop: retry from byte 0 of current 4 MB buffer segment
        │
        ▼
Tauri event → React: download_complete { path, local_path }
```

### Sync (web changes appear on desktop)

```
Rust sync.rs — runs as a background task, every 30 seconds:

GET /api/v1/files/sync?since=<last_cursor>&limit=200
  → { changes: [{path, action, updated_at, size}], next_cursor, has_more }
        │
        ▼
If has_more: immediately fetch next page (cursor-paginated, no gap)
        │
        ▼
For each change:
  action=created | updated:
    → if path is inside a watched folder: queue download_file()
    → always: refresh file tree in React
  action=deleted:
    → if local file exists: move to local trash (not hard delete)
    → refresh file tree
        │
        ▼
Save next_cursor to disk (persists across app restarts)
Tauri event → React: sync_complete { changes_count }
```

Sync cursor is stored in `%APPDATA%\workin-desktop\sync_state.json` — if deleted, the app performs a full re-sync on next launch.

### Conflict detection and resolution

A conflict occurs when the same file is modified both locally and remotely between sync cycles.

**Detection:** when the sync endpoint returns `action=updated` for a path, Rust checks:
- Does the local file exist?
- Is the local `modified_at` (from the OS) newer than the remote `updated_at` in the sync response?

If yes → conflict.

**Resolution options (user controls in Settings → Sync):**

| Strategy | Behaviour | Default |
|---|---|---|
| Ask me | ConflictDialog shown, user picks which version to keep | ✓ |
| Keep remote | Remote always wins, local version saved as `filename.conflict.ext` | |
| Keep local | Local always wins, re-upload overwrites remote | |

**ConflictDialog** shows: file name, local modified time, remote modified time, local size vs remote size. User picks "Keep mine" or "Keep cloud version". The losing version is saved as `filename (conflict copy YYYY-MM-DD).ext` before being overwritten — never silently deleted.

### Authentication

```
First run:
  User enters server URL (default: https://workin.kiwimi.co)
  + email + password
  POST /auth/login → { access_token, refresh_token, expires_in }
  Tokens stored in Windows Credential Manager (Tauri keyring)
  Never written to disk as plaintext

Ongoing:
  Rust reads tokens from keychain on startup
  Before every API call: check if access_token expires in < 60s
    → if yes: POST /auth/refresh, store new tokens silently
  Token rotation is fully transparent to the user

Session management:
  "Sign out" button in Settings → deletes keychain entries, clears sync cursor
  User can see and revoke active desktop sessions from web app Settings → Devices
    (requires desktop session token feature — v2)
```

---

## Offline behaviour

When the network is unavailable:

- The file browser shows the last known file tree (cached in `%APPDATA%\workin-desktop\file_cache.json`)
- Uploads are queued in memory (or optionally on disk if the user enabled "offline queue" in Settings)
- The sync loop backs off: 30s → 60s → 120s → 300s, then stays at 300s until reconnection
- System tray icon shows a grey "offline" indicator
- On reconnection: queued uploads drain first, then sync runs immediately

---

## R2 CORS configuration (required before Phase 2)

For the desktop to PUT directly to R2, the R2 bucket needs a CORS policy. Add this in the Cloudflare dashboard → R2 → your bucket → CORS:

```json
[
  {
    "AllowedOrigins": ["*"],
    "AllowedMethods": ["GET", "PUT", "HEAD"],
    "AllowedHeaders": ["*"],
    "ExposeHeaders": ["ETag"],
    "MaxAgeSeconds": 3600
  }
]
```

`AllowedOrigins: ["*"]` is safe here because every PUT URL is presigned with a short expiry (15 min) and is tied to a specific object path. An attacker with a URL can only overwrite that one specific file, and only within the expiry window. The real security gate is the backend generating the presigned URL — which requires a valid JWT.

---

## System tray behaviour

The tray icon has 4 states:

| State | Icon | Tooltip |
|---|---|---|
| Idle / up to date | Green checkmark | "workin — All files synced" |
| Syncing / uploading | Animated spinner | "workin — Syncing (3 files remaining)" |
| Conflict needs action | Orange exclamation | "workin — 2 conflicts need your attention" |
| Offline / error | Grey | "workin — No connection" |

Tray right-click menu:
- Open workin
- Pause sync / Resume sync
- View transfers
- Settings
- Quit

Desktop notifications (Windows toast) for:
- Upload complete (large files > 100 MB only — not every small file)
- Conflict detected
- Sync error that requires user action

---

## Backend additions required

Two additions. Everything else already exists.

### 1. `GET /api/v1/files/sync` (new endpoint)

```python
# Query params:
#   since  — ISO 8601 timestamp (optional; omit for full sync)
#   cursor — opaque pagination cursor (from previous response)
#   limit  — max records per page (default: 200, max: 500)
#
# Returns:
#   {
#     changes: [{ path, action, updated_at, size, content_type }],
#     next_cursor: "...",   # null if no more pages
#     has_more: bool
#   }
#
# Implementation:
#   Query Firestore `file_records` where updated_at > since, ordered by updated_at asc
#   Include records with deleted_at set (action="deleted")
#   Cursor = base64(last_updated_at + last_doc_id) for stable pagination
#   Requires admin or authenticated user — returns only files owned by the requesting user
```

Pagination is required because a user could have thousands of files change between sync cycles (e.g. bulk import). Without it, a single response could be megabytes.

### 2. `DELETE /api/v1/files/sync/cursor` (optional, nice to have)

Lets the desktop reset its sync cursor to trigger a full re-sync without deleting local state files.

### 3. R2 bucket CORS policy (config, not code)

See R2 CORS section above. One-time setup in the Cloudflare dashboard.

---

## Build and release

```bash
# Prerequisites: Rust toolchain, Node 20, pnpm, Tauri CLI
cargo install tauri-cli

# Development (hot reload)
pnpm install
pnpm tauri dev

# Production build (Windows x64)
pnpm tauri build
# Output:
#   src-tauri/target/release/bundle/nsis/workin-desktop_x.x.x_x64-setup.exe
#   src-tauri/target/release/bundle/msi/workin-desktop_x.x.x_x64_en-US.msi
```

**Code signing:** Without a certificate, Windows SmartScreen shows "Unknown publisher" on first run. Acceptable for beta. For production: purchase an EV certificate (~$300/yr) or use a standard OV certificate. Tauri supports signing via `signtool.exe` in CI.

**Auto-update:** Tauri's built-in updater fetches a `latest.json` from a URL set in `tauri.conf.json`. Host this on GitHub Releases. The app checks on every launch, downloads in the background, and prompts the user to restart.

```json
{
  "url": "https://github.com/parsherr/workin-desktop/releases/latest/download/latest.json",
  "pubkey": "your-tauri-update-public-key"
}
```

---

## Implementation phases

### Phase 1 — Foundation (1–2 days)
- Initialise Tauri 2 project with React 18, TypeScript, Tailwind CSS v4, shadcn/ui
- Setup page: server URL field + email/password login
- JWT stored in Windows Credential Manager via keyring plugin
- API client (`lib/api.ts`) pointed at production URL
- Read-only file browser calling existing `/api/v1/files` endpoints
- System tray: basic icon + "Open / Quit" menu

**Deliverable:** App opens, you can log in, browse files. No upload/download yet.

### Phase 2 — Upload engine (2–3 days)
- Rust `upload.rs`: single presigned PUT for files < 10 MB
- Rust `upload.rs`: multipart for files ≥ 10 MB (5 MB chunks, 4 parallel)
- Per-chunk retry with exponential backoff
- Progress events → React `Transfers.tsx` with speed (MB/s) and ETA
- R2 CORS policy configured (prerequisite — do this before writing any upload code)
- Drag-and-drop into file browser triggers upload

**Deliverable:** Users can upload files of any size. Large file progress is visible. Retries work on flaky connections.

### Phase 3 — Download + sync delta (2 days)
- Rust `download.rs`: presigned GET, streaming to disk with progress
- Backend: `GET /api/v1/files/sync` endpoint with cursor pagination
- Rust `sync.rs`: background sync loop, 30s interval, exponential backoff on error
- Cursor persisted to `%APPDATA%\workin-desktop\sync_state.json`
- File tree refreshes when sync detects remote changes
- Offline state: sync loop backs off, tray goes grey

**Deliverable:** Files added on the web appear in the desktop. App survives network drops.

### Phase 4 — Conflict resolution + folder watcher (2–3 days)
- Rust `conflict.rs`: detect conflicts by comparing local `modified_at` vs remote `updated_at`
- `ConflictDialog.tsx`: user picks which version wins; loser saved as conflict copy
- Settings → Sync: strategy selector (Ask / Keep remote / Keep local)
- Rust `watcher.rs`: `notify` crate watches user-selected folders
- New/changed local files auto-queued for upload
- Deleted local files tombstoned on remote (soft delete)

**Deliverable:** Watched folders sync automatically. Conflicts surface cleanly instead of silently overwriting.

### Phase 5 — Polish + packaging (1–2 days)
- Full tray states (synced / syncing / conflict / offline)
- Windows toast notifications (upload complete for large files, conflicts, errors)
- Auto-updater wired to GitHub Releases
- NSIS installer with app icon, start menu entry, optional startup with Windows
- Error handling audit: every retryable error retries, every fatal error surfaces a clear message

**Deliverable:** Signed/unsigned `.exe` installer ready to distribute. App updates itself.

---

## What this is NOT

- Not a replacement for the web app — it is a companion for users who work with large files or want background sync
- Not a separate data store — same Firestore, same R2 bucket, same user accounts
- Not a full Dropbox clone in v1 — folder watch is Phase 4, not day one
- Not cross-platform in v1 — Windows first; macOS and Linux require minimal Tauri changes and can follow
- Not an offline-first app — it requires internet to function; offline mode is graceful degradation, not a primary use case

---

## Security model

The desktop app has direct write access to Cloudflare R2 via presigned URLs. This section defines exactly what is and isn't protected, and where the security boundaries are.

### What presigned URLs protect

Every presigned PUT URL generated by the backend is:
- **Scoped to a single object path** — a URL for `projects/designs/logo.png` cannot be used to write to any other path
- **Time-limited** — upload presigned URLs expire in 15 minutes; download URLs expire in 30 minutes
- **Single-use by convention** — R2 does not enforce single-use, but expiry limits the window of abuse to 15 minutes
- **Gated by a valid JWT** — the backend only generates presigned URLs for authenticated users; a user without a valid session cannot get a presigned URL

**Consequence:** if a presigned URL were intercepted, an attacker could overwrite that one specific file within the 15-minute window. They cannot enumerate other files, cannot read files they didn't request a download URL for, and cannot write to paths they didn't request an upload URL for.

### Token scoping

In v1, the desktop uses the same JWT as the web app (same `/auth/login`, same `/auth/refresh`). This means a desktop session has full API access — it can call any endpoint the web app can call.

**v2 improvement:** issue a separate device token with a restricted scope: `files:read files:write` only, no `admin:*`. This limits blast radius if a token is extracted from the keychain. Track device tokens in a Firestore `device_sessions` collection so users can revoke individual desktop sessions from the web app under Settings → Devices.

### Keychain security

Tokens are stored in the **Windows Credential Manager** via Tauri's `keyring` plugin. This means:
- Tokens are encrypted at rest using the user's Windows login credentials (DPAPI)
- No other app on the same machine can read the credentials without the user's Windows password
- The credential is stored under the key `workin-desktop:<user-email>`
- On sign-out, `keyring::delete_credential()` is called — no trace left on disk

### Account suspension / deletion mid-upload

If a user's account is deleted or suspended while an upload is in progress:
- The next API call (either a new presigned URL request or the `multipart/complete` call) returns `401` or `403`
- Rust upload engine treats `401` as a token expiry → attempts one token refresh
- If refresh also returns `401`/`403`: upload is aborted, all in-progress chunks cancelled, Tauri event `session_invalidated` emitted
- React shows a full-screen "Your session has ended — please sign in again" prompt
- Local files are never deleted — the upload simply did not complete

### What is NOT protected

- **Local files on disk** — the app reads and writes files in user-selected folders. If a user grants the app access to a sensitive folder, those files are readable by the app. Tauri's file system permissions are scoped in `tauri.conf.json` to prevent access outside user-selected paths.
- **Presigned URL interception** — if a user's network is compromised (e.g. malicious proxy), presigned URLs could be intercepted. Mitigation: all API calls use HTTPS; presigned URLs are also HTTPS (R2 always serves TLS). There is no mitigation for a compromised machine.
- **Firestore metadata** — file metadata (names, sizes, paths) is stored in Firestore and accessible to anyone with a valid JWT for that account. File bytes require a separate presigned URL.

---

## State file versioning and migration

Local state files in `%APPDATA%\workin-desktop\` must be versioned so app updates don't corrupt them.

### State files

| File | Purpose | Format |
|---|---|---|
| `sync_state.json` | Last sync cursor, per-folder sync config | JSON |
| `file_cache.json` | Cached file tree for offline display | JSON |
| `preferences.json` | User preferences (tray behaviour, notification settings) | JSON |

### Schema versioning

Every state file contains a top-level `"schema_version": N` field. On startup, Rust reads the version and runs migrations sequentially:

```rust
match state.schema_version {
    1 => migrate_v1_to_v2(&mut state),
    2 => migrate_v2_to_v3(&mut state),
    _ => {} // current version, no migration needed
}
```

Migration functions are append-only — never deleted, only added. If a migration fails (corrupted file), the file is renamed to `sync_state.corrupt.json` and a fresh default state is written. The user loses their sync cursor (triggering a full re-sync) but the app does not crash.

### Breaking changes policy

- Adding a new field: non-breaking. New field gets a default value in old versions.
- Removing a field: increment `schema_version`, write a migration that drops the field.
- Changing a field type: increment `schema_version`, write a migration that converts the type.
- Never increment `schema_version` without writing the corresponding migration function.

---

## Testing strategy

Sync engines fail silently. A bug that overwrites the wrong file or misses a remote change may not be noticed for days. The test suite must catch these.

### Rust unit tests (`src-tauri/src/`)

Every module has a `#[cfg(test)]` block. Key test cases:

**`upload.rs`**
- Single PUT: mock R2 returns 200, verify Firestore `file_record` written with correct metadata
- Multipart: mock returns 200 for each chunk + valid ETag, verify `CompleteMultipartUpload` called with correct parts list
- Chunk retry: mock returns 503 on first attempt, 200 on second — verify upload succeeds
- Presign expiry mid-upload: mock returns 403 on chunk 3, verify new presigned URLs requested and upload continues
- `EntityTooSmall`: mock returns 400 on `complete`, verify abort called and upload restarted with larger chunk size
- Progress events: verify event emitted after each chunk with correct `bytes_done` value

**`sync.rs`**
- Empty response: verify cursor not advanced
- Pagination: mock returns `has_more: true` twice, verify three requests made and all changes processed
- `created` action inside watched folder: verify `download_file()` queued
- `deleted` action for local file: verify file moved to local trash, not hard-deleted
- Cursor persisted to disk after successful sync cycle
- Network failure: verify backoff doubles on each consecutive failure, caps at 300s

**`conflict.rs`**
- Local newer than remote: verify conflict detected
- Remote newer than local: verify no conflict, download queued
- Equal timestamps: verify no conflict (idempotent)
- "Keep remote" strategy: verify local file saved as `.conflict.ext` before overwrite
- "Keep local" strategy: verify upload queued, remote file not downloaded

**`watcher.rs`**
- New file in watched folder: verify upload queued within 500ms of filesystem event
- File modified: verify upload queued (not a new record — update existing)
- File deleted: verify soft-delete API call made
- File moved within watched folder: verify rename API call, not delete+create

**`auth.rs`**
- Token refresh: mock returns new token pair, verify keychain updated
- Refresh fails with 401: verify `session_invalidated` event emitted
- Keychain read failure: verify graceful error (prompt login), not panic

### Integration tests (against real R2 test bucket)

Run with `cargo test --features integration` (skipped in CI by default, run manually before release):
- Full multipart upload of a 100 MB file: verify object appears in R2 test bucket
- Download of that object: verify bytes match SHA-256 of the original file
- Upload abort: kill the test process mid-upload, restart, verify multipart upload was aborted in R2 (not left as incomplete — R2 charges for incomplete multipart uploads)

### React / UI tests (Vitest)

- `TransferItem.tsx`: renders correct progress bar percentage and speed label for given transfer state
- `ConflictDialog.tsx`: "Keep mine" button calls correct Tauri command; "Keep cloud" button calls correct command; losing file path displayed correctly
- `useTransfers.ts`: Tauri `listen()` mocked — verify store updates when progress event fires
- `useSyncStatus.ts`: verify tray badge component re-renders on sync state change

### End-to-end test (manual, pre-release checklist)

Before each release, run this checklist on a real Windows machine:

- [ ] Fresh install: Setup page shown, login with real account works
- [ ] Upload a 1 GB file: progress bar moves, file appears on web app after completion
- [ ] Upload a 10 MB file (single PUT path): appears on web app
- [ ] Kill the app mid-upload of a large file, restart: upload resumes or cleanly restarts
- [ ] Add a file on the web app: appears in desktop file browser within 35 seconds
- [ ] Delete a file on the web: disappears from desktop within 35 seconds
- [ ] Enable folder watch, add a file to the folder: file uploads automatically
- [ ] Create a conflict manually: ConflictDialog appears, both resolution options work
- [ ] Disable network mid-sync: tray goes grey, no crash, reconnect resumes sync
- [ ] Auto-update: previous version installed, new version pushed, app updates itself on launch
- [ ] Sign out: keychain entries deleted, sync stops, Setup page shown on next launch

---

## Known limitations and v2 roadmap

| Limitation | Reason | v2 plan |
|---|---|---|
| Windows only | Tauri supports all platforms but testing bandwidth is limited | macOS + Linux after Windows is stable |
| No per-file encryption | Files in R2 are encrypted at rest by Cloudflare but not end-to-end encrypted | Client-side AES-256 encryption option in Settings |
| Full JWT scope for desktop | Same token as web session | Scoped device tokens with `files:read files:write` only |
| Sync polling (30s latency) | Firestore doesn't expose a webhook to R2 events | Firestore real-time listener via WebSocket (eliminates polling) |
| No bandwidth throttle | Upload can saturate the user's connection | Upload speed limit setting (MB/s cap) |
| Local trash not size-limited | Conflict copies accumulate on disk | Auto-purge local trash after 30 days |
| No selective sync | All files in a watched folder sync | Per-subfolder include/exclude rules |
