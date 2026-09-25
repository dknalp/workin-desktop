# workin-desktop — TODO & Version Plan

## MVP definition

The MVP is the smallest thing that is genuinely useful and shippable.

**MVP = a user can log in, browse their files, and upload a file of any size directly to R2.**

That's it. No sync, no folder watcher, no conflict resolution. Just: open the app, sign in with your existing account, drag a 10 GB file in, watch it upload at full speed, see it appear on the web app. That alone solves the core problem (browser upload limits) and is worth shipping.

**MVP is NOT:**
- Background sync
- Folder watcher
- Conflict resolution
- Auto-update
- System tray
- Download (nice to have but not blocking)

---

## MVP checklist

### M1 — Project setup
- [ ] `pnpm create tauri-app workin-desktop` with React + TypeScript template
- [ ] Add Tailwind CSS v4 and shadcn/ui
- [ ] Copy design tokens from `workin.kiwimi.co` (oklch colors, font variables)
- [ ] Copy shared components from web app: `Button`, `Input`, `Badge`, `ScrollArea`, `Separator`
- [ ] Configure `tauri.conf.json`: app name, identifier (`co.kiwimi.workin-desktop`), window size (1100×720 min), no devtools in production
- [ ] Add Tauri `keyring` plugin to `Cargo.toml`
- [ ] Set up Vite config with `@` path alias pointing to `src/`
- [ ] Verify `pnpm tauri dev` opens a window on Windows

### M2 — Authentication
- [ ] `src/pages/Setup.tsx`: server URL field (default: `https://workin.kiwimi.co`) + email + password fields + Sign In button
- [ ] `src/lib/api.ts`: base API client — reads server URL from Zustand store, attaches `Authorization: Bearer <token>` header, handles 401 → refresh → retry
- [ ] `src/lib/commands.ts`: typed Tauri command wrappers (`invoke<T>()` with proper types)
- [ ] `src-tauri/src/auth.rs`:
  - [ ] `login(email, password, server_url)` → calls `POST /auth/login`, stores tokens in keychain
  - [ ] `logout()` → deletes keychain entries
  - [ ] `get_token()` → reads from keychain, refreshes if expiry < 60s
  - [ ] `refresh_token()` → calls `POST /auth/refresh`, stores new tokens
- [ ] `src-tauri/src/state.rs`: `AppState` struct with `server_url`, `user_email`, `is_authenticated`
- [ ] On app launch: check keychain for existing token → if found, skip Setup page, go straight to Files
- [ ] Setup page: show error message on wrong credentials (red text under form, not an alert box)
- [ ] Settings page stub with "Sign out" button that calls `logout()` and returns to Setup

### M3 — File browser (read-only)
- [ ] `src/pages/Files.tsx`: call `GET /api/v1/files?path=/` on mount, render folder tree
- [ ] `src/components/FileTree.tsx`: folder/file list with icons, click folder to navigate into it
- [ ] Breadcrumb navigation bar (Home → folder → subfolder)
- [ ] File row shows: icon, name, size (human-readable), last modified date
- [ ] Empty folder state: "No files here" message
- [ ] Loading skeleton while fetching
- [ ] Error state: "Could not load files — check your connection" with retry button

### M4 — Upload engine (core MVP feature)
- [ ] R2 bucket CORS policy configured (do this before writing any upload code)
- [ ] `src-tauri/src/upload.rs`:
  - [ ] `upload_file(local_path, remote_path, server_url, token)` command
  - [ ] Files < 10 MB: `GET /api/v1/files/presign/put` → single PUT to R2 → `POST /api/v1/files` to record metadata
  - [ ] Files ≥ 10 MB: multipart path:
    - [ ] `POST /api/v1/files/presign/multipart/start` → get `upload_id` + chunk URLs
    - [ ] Split file into 5 MB chunks using `tokio::fs` streaming reads (never load whole file into RAM)
    - [ ] Upload 4 chunks in parallel using `tokio::spawn` + `reqwest`
    - [ ] Emit `upload_progress` Tauri event after each chunk: `{ path, bytes_done, total_bytes, speed_bps }`
    - [ ] On chunk 5xx: retry up to 3× with 1s/2s/4s backoff
    - [ ] On chunk 403 (presign expired): request new URLs for remaining chunks, continue
    - [ ] On chunk 400 EntityTooSmall: abort, restart with 10 MB chunks
    - [ ] All chunks done: `POST /api/v1/files/presign/multipart/complete` with ETag list
- [ ] `src/hooks/useTransfers.ts`: `listen('upload_progress', ...)` → update Zustand store
- [ ] `src/pages/Transfers.tsx`: list of active uploads with progress bar, speed (MB/s), ETA
- [ ] `src/components/TransferItem.tsx`: single row — filename, progress bar, `X of Y MB`, speed, ETA
- [ ] Drag-and-drop onto file browser area triggers upload to current folder path
- [ ] "Upload files" button (file picker) as fallback for drag-and-drop
- [ ] After upload completes: file list refreshes automatically

### M5 — MVP polish
- [ ] App icon (1024×1024 PNG → Tauri generates all sizes)
- [ ] Window title: "workin" (not "workin-desktop")
- [ ] Keyboard shortcuts: `Cmd/Ctrl+U` opens file picker, `Backspace` navigates up one folder
- [ ] File browser: right-click context menu → "Upload here", "Create folder"
- [ ] Error boundary in React: if the whole UI crashes, show "Something went wrong — restart the app" instead of a blank white window
- [ ] `pnpm tauri build` produces a working `.exe` installer
- [ ] Manual test on a clean Windows 10/11 VM: install, login, upload 1 GB file, verify it appears on web

**MVP is done when:** a real user on a real Windows machine can install, log in with their existing account, and upload a 10 GB file successfully.

---

## Full version plan

---

## v0.1.0 — MVP (target: ~1 week)

Everything in the MVP checklist above.

**Scope:**
- Login / logout
- Read-only file browser
- Upload any size (multipart, 4 parallel chunks, retry)
- Transfer queue with progress
- Basic `.exe` installer (unsigned)

**Not included:** download, sync, folder watch, tray, auto-update, conflict resolution.

---

## v0.2.0 — Download + basic tray (target: +3–4 days)

### Download
- [ ] `src-tauri/src/download.rs`:
  - [ ] `download_file(remote_path, local_path)` command
  - [ ] `GET /api/v1/files/presign/download?path=...` → presigned GET URL
  - [ ] Stream bytes from R2 to disk (4 MB buffer, never load full file into RAM)
  - [ ] Emit `download_progress` event every 256 KB: `{ path, bytes_done, total_bytes }`
  - [ ] On network drop: retry from start of current 4 MB buffer segment
- [ ] File browser right-click → "Download" option
- [ ] Download destination: system "Downloads" folder by default, user can change in Settings
- [ ] Downloads appear in Transfers queue alongside uploads

### System tray
- [ ] `src-tauri/src/tray.rs`: tray icon with 4 states (idle/syncing/conflict/offline)
- [ ] Tray right-click menu: Open workin / View transfers / Settings / Quit
- [ ] Closing the window hides it to tray (app keeps running)
- [ ] Tray tooltip: "workin — All files synced" (static for now, dynamic in v0.3)

### File operations
- [ ] Create folder (right-click → New folder → inline rename)
- [ ] Rename file/folder (right-click → Rename → inline edit)
- [ ] Delete file (right-click → Delete → confirmation dialog → soft delete via API)

---

## v0.3.0 — Sync engine (target: +4–5 days)

### Backend prerequisite
- [ ] `GET /api/v1/files/sync` endpoint on backend:
  - [ ] Query params: `since` (ISO timestamp), `cursor` (opaque), `limit` (default 200, max 500)
  - [ ] Returns: `{ changes: [{path, action, updated_at, size}], next_cursor, has_more }`
  - [ ] Cursor = base64(`last_updated_at:last_doc_id`) for stable Firestore pagination
  - [ ] Filters to files owned by the requesting user only
  - [ ] Includes soft-deleted records (`action: "deleted"`)

### Sync engine
- [ ] `src-tauri/src/sync.rs`:
  - [ ] Background `tokio::task` that runs every 30 seconds
  - [ ] `GET /api/v1/files/sync?since=<cursor>&limit=200`
  - [ ] If `has_more`: immediately fetch next page (no gap in changes)
  - [ ] For `created`/`updated`: refresh file tree in React via Tauri event
  - [ ] For `deleted`: remove from file tree cache
  - [ ] Save cursor to `%APPDATA%\workin-desktop\sync_state.json` after each successful cycle
  - [ ] On network failure: exponential backoff (30s → 60s → 120s → 300s, cap at 300s)
  - [ ] On reconnect: run sync immediately (don't wait for next 30s tick)
- [ ] State file versioning: `sync_state.json` has `schema_version: 1`
- [ ] File tree cache: `file_cache.json` with `schema_version: 1`, used for offline display
- [ ] Tray tooltip now shows sync status: "Syncing (12 changes)..." / "All files synced"
- [ ] Tray spinner animation while sync in progress
- [ ] `src/hooks/useSyncStatus.ts`: subscribe to sync events, update status badge in file browser

### Offline mode
- [ ] When API unreachable: load file tree from `file_cache.json`
- [ ] Tray icon → grey "offline" state
- [ ] Banner in file browser: "You're offline — showing cached files"
- [ ] Upload queue paused (items stay in queue, resume on reconnect)

---

## v0.4.0 — Folder watcher + conflict resolution (target: +4–5 days)

### Folder watcher
- [ ] `src-tauri/src/watcher.rs`:
  - [ ] Uses `notify` crate (`RecommendedWatcher`, recursive mode)
  - [ ] User selects a local folder in Settings → Sync → Add folder
  - [ ] Watcher events debounced by 500ms (avoid duplicate events for save-then-modify patterns)
  - [ ] `Created` / `Modified` → queue `upload_file()` to corresponding remote path
  - [ ] `Deleted` → call soft-delete API for corresponding remote path
  - [ ] `Renamed` → call rename API (not delete + create)
  - [ ] Watcher ignores: `.DS_Store`, `Thumbs.db`, `desktop.ini`, `~$*` (Office temp files), `*.tmp`
  - [ ] Watched folders stored in `preferences.json`, restored on next launch
- [ ] Settings → Sync page:
  - [ ] List of watched folders with local path, remote path mapping, and remove button
  - [ ] "Add folder" → native folder picker → maps to a remote path (user configures)
  - [ ] "Pause sync" toggle per folder

### Conflict resolution
- [ ] `src-tauri/src/conflict.rs`:
  - [ ] `detect(local_modified_at, remote_updated_at, path)` → `bool`
  - [ ] On conflict detected: emit `conflict_detected` Tauri event with both file metadata
  - [ ] "Keep remote" strategy: save local as `filename (conflict copy YYYY-MM-DD).ext`, download remote
  - [ ] "Keep local" strategy: queue upload, skip download
  - [ ] "Ask me" strategy (default): emit event, wait for user decision via `resolve_conflict` command
- [ ] `src/components/ConflictDialog.tsx`:
  - [ ] Modal: file name, local modified time, local size vs remote modified time, remote size
  - [ ] "Keep mine" and "Keep cloud version" buttons
  - [ ] "Apply to all conflicts" checkbox (batch resolution)
- [ ] `src/hooks/useConflicts.ts`: subscribe to `conflict_detected` events, queue dialogs
- [ ] Settings → Sync: conflict strategy selector (Ask / Keep remote / Keep local)
- [ ] Tray → orange exclamation when unresolved conflicts exist

---

## v0.5.0 — Polish + security hardening (target: +3 days)

### Security
- [ ] Tauri file system permissions in `tauri.conf.json`: restrict to user-selected paths only
- [ ] `session_invalidated` event handler: clear UI, show re-login prompt without losing transfer queue
- [ ] Token refresh race condition fix: if two requests trigger refresh simultaneously, only one refresh runs (mutex in `auth.rs`)
- [ ] Audit all `unwrap()` calls in Rust — replace with proper error handling

### State file migration infrastructure
- [ ] `src-tauri/src/migrations.rs`: sequential migration runner
- [ ] `migrate_sync_state(path)`: reads version, runs `v1_to_v2` etc., writes back
- [ ] On corrupt file: rename to `.corrupt.json`, write fresh default, log warning
- [ ] Unit tests for each migration function

### UX polish
- [ ] Windows toast notification: upload complete (files > 100 MB only)
- [ ] Windows toast notification: conflict detected (with "Resolve now" action button)
- [ ] Keyboard shortcut: `F5` / `Ctrl+R` refreshes file list
- [ ] Keyboard shortcut: `Delete` key on selected file → delete confirmation
- [ ] File browser: multi-select with `Shift+click` and `Ctrl+click`
- [ ] Upload: "Upload folder" option (recursively enqueues all files in folder)
- [ ] Transfers page: "Cancel all" and "Clear completed" buttons
- [ ] Empty state for Transfers page: "No transfers — drag files here to upload"

---

## v1.0.0 — Production release (target: +2 days)

### Release infrastructure
- [ ] Code signing certificate acquired (EV or OV)
- [ ] Tauri `signtool.exe` signing configured in `tauri.conf.json`
- [ ] GitHub Actions workflow: build on Windows runner, sign, upload to GitHub Releases
- [ ] Auto-updater `latest.json` generated and published on every release
- [ ] Auto-updater tested: v0.5.0 → v1.0.0 update works silently
- [ ] Installer includes: start menu entry, optional "Start with Windows" checkbox, uninstaller

### Testing
- [ ] All Rust unit tests passing (`cargo test`)
- [ ] Integration test suite passing against R2 test bucket (`cargo test --features integration`)
- [ ] React/Vitest tests passing (`pnpm test`)
- [ ] Manual pre-release checklist completed on clean Windows 10 VM
- [ ] Manual pre-release checklist completed on clean Windows 11 VM

### Documentation
- [ ] `README.md`: what it is, download link, system requirements
- [ ] `CHANGELOG.md`: v0.1 through v1.0 changes
- [ ] In-app: "About" dialog with version number, link to changelog

**v1.0.0 is done when:** signed installer is on GitHub Releases, auto-update works, all tests pass, manual checklist passes on two fresh Windows VMs.

---

## v1.x roadmap (post-1.0)

| Version | Feature |
|---|---|
| v1.1 | Selective sync: per-subfolder include/exclude rules |
| v1.2 | Upload speed throttle setting (MB/s cap) |
| v1.3 | Scoped device tokens (revocable from web app Settings → Devices) |
| v1.4 | Firestore real-time listener replaces 30s polling (zero-latency sync) |
| v1.5 | macOS support (Tauri already cross-platform — mainly packaging + testing) |
| v2.0 | Client-side AES-256 encryption (files encrypted before leaving the machine) |
