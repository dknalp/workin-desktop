# workin-desktop — Implementation Plan

## What this is and why it exists

The web app (`workin-desktop`'s companion at `workin.kiwimi.co`) already works well for most things. But the browser is a hard constraint for files:

- Every uploaded byte travels **browser → backend server → Cloudflare R2**. The backend is a bottleneck you pay for in RAM, bandwidth, and timeout budget.
- Nginx proxy timeouts kill uploads that take longer than ~60–90 seconds.
- Browsers buffer large files in RAM — a 2 GB file can crash the tab.
- No resume. Drop the connection, start over.
- No background. Close the tab, lose the upload.

The desktop app solves all of this with one architectural change: **bytes never touch the backend**. The app talks to your backend only to authenticate and get presigned URLs, then uploads directly to Cloudflare R2 from the user's machine. The backend records the metadata in Firestore after the upload completes. The file appears on the web app instantly.

This is not a separate product. It is a power client for the same system — same files, same database, same R2 bucket, same users.

---

## Goals

1. Upload files and folders of any size (tested to 50 GB+) directly to R2, bypassing the backend server entirely.
2. Files uploaded from the desktop appear on the web app immediately.
3. Files added/deleted/renamed on the web appear in the desktop app's file browser.
4. Background sync of a watched local folder (optional per folder, user-controlled).
5. Resumable transfers — a dropped connection retries only the failed chunk, not the whole file.
6. Windows `.exe` installer, ~8 MB, no Chromium bundled, runs on Windows 10/11.

---

## Why Tauri (not Electron)

| | Electron | Tauri |
|---|---|---|
| Installer size | 150–200 MB (ships Chromium) | 4–8 MB (uses OS WebView2, pre-installed on Win 10+) |
| RAM usage | 200–400 MB idle | 30–80 MB idle |
| UI language | React/TS (same as your web) | React/TS (same as your web) |
| Native backend | Node.js | Rust |
| File system access | Node fs | Rust `std::fs` + `notify` crate |
| OS keychain | `keytar` (flaky) | Tauri `keyring` plugin (native) |
| Auto-update | `electron-updater` | Tauri built-in updater |
| Multipart upload | JS (fine) | Rust (parallel, non-blocking, resumable) |

Tauri wins on install size, RAM, and the native file watcher. The UI is still React/TypeScript so you can copy components directly from the web app.

---

## Tech Stack

| Layer | Technology | Reason |
|---|---|---|
| UI | React 18 + TypeScript | Same as web app — reuse components, API client, design tokens |
| Styling | Tailwind CSS v4 + shadcn/ui | Identical to web — copy components directly |
| Desktop shell | Tauri 2.x | Small binary, native OS APIs, Rust backend |
| Native backend | Rust | File watcher, multipart upload engine, OS keychain |
| File watcher | `notify` crate (Rust) | Cross-platform inotify/FSEvents/ReadDirectoryChangesW |
| HTTP client | `reqwest` crate (Rust) | Async, streaming, used for chunk uploads to R2 |
| Auth storage | Tauri `keyring` plugin | Stores JWT in Windows Credential Manager — never in a plain file |
| State sync | Tauri events (Rust → React) | Rust emits progress events, React renders them |
| Build / package | Tauri CLI + NSIS | Produces signed `.exe` installer |

---

## Architecture

```
workin-desktop/
├── src-tauri/                    # Rust — native backend
│   ├── Cargo.toml
│   ├── tauri.conf.json           # app config, permissions, updater URL
│   └── src/
│       ├── main.rs               # Tauri app entry, command registration
│       ├── auth.rs               # Login, token refresh, OS keychain storage
│       ├── sync.rs               # Delta sync engine (polls /files/sync)
│       ├── watcher.rs            # Local folder watcher (notify crate)
│       ├── upload.rs             # Multipart upload engine, presigned URLs
│       ├── download.rs           # Presigned GET, streaming to disk
│       └── state.rs              # Shared app state (AppState struct)
│
└── src/                          # React/TypeScript — UI
    ├── main.tsx                  # Tauri app bootstrap
    ├── App.tsx                   # Router (Setup → Main)
    ├── pages/
    │   ├── Setup.tsx             # First-run: server URL + login
    │   ├── Files.tsx             # File browser (mirrors web app /files)
    │   ├── Transfers.tsx         # Active uploads / downloads queue
    │   └── Settings.tsx          # Sync folders, account, preferences
    ├── components/
    │   │                         # Copied/adapted from web app:
    │   ├── ui/                   # shadcn/ui components (Button, Input, etc.)
    │   ├── FileTree.tsx          # Folder tree browser
    │   ├── TransferItem.tsx      # Single upload/download row with progress bar
    │   └── SyncBadge.tsx        # Status indicator (synced / syncing / error)
    ├── lib/
    │   ├── api.ts                # API client (same shape as web, hits production URL)
    │   ├── tauriCommands.ts      # Typed wrappers for Rust #[tauri::command] calls
    │   └── store.ts              # Zustand store — transfers queue, sync state
    └── hooks/
        ├── useTransfers.ts       # Subscribe to Tauri upload progress events
        └── useSyncStatus.ts      # Subscribe to Tauri sync status events
```

---

## How the data flows

### Upload (any file size)

```
User drops file into app
        │
        ▼
React → Tauri command: upload_file(local_path, remote_path)
        │
        ▼ (Rust: upload.rs)
GET /api/v1/files/presign/multipart/start
  → backend returns: upload_id + N presigned chunk URLs
        │
        ▼
Rust uploads 4 chunks in parallel directly to R2 (reqwest streaming)
No bytes touch the backend server
        │
        ▼ (all chunks done)
POST /api/v1/files/presign/multipart/complete
  → backend calls R2 CompleteMultipartUpload
  → backend writes Firestore file_record
        │
        ▼
Tauri event → React: transfer complete, 100%
File appears on web app at workin.kiwimi.co
```

Files under 10 MB skip multipart and use a single presigned PUT URL.

### Download

```
User clicks Download in desktop file browser
        │
        ▼
React → Tauri command: download_file(remote_path, local_path)
        │
        ▼ (Rust: download.rs)
GET /api/v1/files/presign/download?path=...
  → backend returns presigned GET URL (30-min expiry)
        │
        ▼
Rust streams bytes directly from R2 to local disk
Progress events → React every 256 KB
```

### Sync (web changes appear on desktop)

```
Rust sync.rs polls every 30 seconds:
GET /api/v1/files/sync?since=<last_check_timestamp>
  → returns list of {path, action: created|updated|deleted, updated_at}
        │
        ▼
For each change:
  created / updated → download_file() if path is inside a watched folder
  deleted → delete local file if present
        │
        ▼
Tauri event → React: file tree refreshed
```

The `/files/sync` endpoint is **the only net-new backend work**. It is a single Firestore query on `file_records` filtered by `updated_at > since`. Everything else (presigned URLs, auth, file metadata) already exists in the backend.

### Authentication

```
First run:
  User enters production URL + email + password
  App calls POST /auth/login (existing endpoint)
  JWT stored in Windows Credential Manager via Tauri keyring plugin
  Never written to disk as plaintext

Ongoing:
  Rust reads token from keychain on startup
  Uses same refresh flow (POST /auth/refresh) as web app
  Token rotation is transparent to the user
  User can revoke the desktop session from web app Settings → Devices
```

---

## Backend additions required

Only two new things needed. Everything else already exists.

### 1. `GET /api/v1/files/sync`

```python
# Query params: since (ISO 8601 timestamp)
# Returns: list of file_records changed after `since`
# Each record includes: path, action (created/updated/deleted), updated_at, size, content_type
```

The `file_records` collection in Firestore already has `updated_at` and `deleted_at` fields. This is a 30-line router.

### 2. Desktop session token (optional for v1)

For v1, the desktop can reuse the standard JWT + refresh flow. A dedicated long-lived device token (revocable per-device from the web) is a v2 improvement.

---

## Build and release

```bash
# Development
cd workin-desktop
pnpm install
pnpm tauri dev          # hot reload — Rust recompiles on change, React uses Vite HMR

# Production build
pnpm tauri build        # produces:
                        #   src-tauri/target/release/bundle/nsis/workin-desktop_x.x.x_x64-setup.exe
                        #   src-tauri/target/release/bundle/msi/workin-desktop_x.x.x_x64_en-US.msi
```

The NSIS installer is signed with a code-signing certificate. Without it, Windows SmartScreen shows a warning on first run (acceptable for internal/beta use).

Auto-update: Tauri's built-in updater checks a JSON endpoint (hosted on GitHub Releases or your own server) on every launch. Releases update silently in the background.

---

## Implementation phases

### Phase 1 — Foundation (1–2 days)
- Initialize Tauri 2 project with React/TS/Tailwind
- Setup page (URL + login)
- JWT storage in OS keychain
- API client pointed at production
- File browser (read-only, mirrors the web)

### Phase 2 — Upload engine (2–3 days)
- Rust multipart upload with 4-parallel chunks
- Progress events → React transfer queue UI
- Single presigned PUT for files < 10 MB
- Retry logic per chunk (3 attempts, exponential backoff)

### Phase 3 — Download + sync delta (1–2 days)
- Presigned GET download with streaming progress
- `/files/sync` backend endpoint (backend work)
- Rust sync loop polling every 30s

### Phase 4 — Local folder watcher (2 days)
- User selects a local folder to watch
- `notify` crate detects new/changed/deleted files
- Auto-uploads new files, tombstones deletes on remote

### Phase 5 — Packaging (1 day)
- NSIS installer
- Auto-updater endpoint
- Code signing (or unsigned for beta)

---

## What this is NOT

- Not a replacement for the web app — it's a companion for power users who work with large files
- Not a separate data store — same Firestore, same R2, same users
- Not a full Dropbox clone in v1 — folder watch is Phase 4, not day one
- Not cross-platform in v1 — Windows first, macOS/Linux later if needed (Tauri supports all three with minimal changes)
