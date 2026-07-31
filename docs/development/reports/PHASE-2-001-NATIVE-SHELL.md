# Phase 2 Task 1 — Native Shell

**Date:** 2026-07-30  
**Project:** Slackinux v0.1.0  
**Task:** Tray icon, app menu, zoom controls, native notifications, downloads, unread badge.

---

## Acceptance criteria

| #  | Criterion                                   | Status |
|----|---------------------------------------------|--------|
| 1  | `cargo fmt` passes                          | PASS   |
| 2  | `cargo clippy` with warnings denied         | PASS   |
| 3  | `cargo test` passes                         | PASS   |
| 4  | `cargo build --release` passes              | PASS   |
| 5  | Tray icon present with Show/Quit menu       | PASS   |
| 6  | Close window hides to tray (no quit)        | PASS   |
| 7  | Left-click tray toggles window visibility   | PASS   |
| 8  | App menu built programmatically             | PASS   |
| 9  | Zoom in/out/reset via menu + shortcuts      | PASS   |
| 10 | Native notifications via `notify-rust`      | PASS   |
| 11 | Downloads routed to `downloads/` dir        | PASS   |
| 12 | Unread count reflected in tray tooltip      | PASS   |

---

## Files changed

| File                          | Change |
|-------------------------------|--------|
| `src/main.rs`                 | Added tray, menu, zoom state, close-to-tray, title→tooltip, download dir |
| `src-tauri/Cargo.toml`        | Added `notify-rust`, enabled `tray-icon` feature |
| `src-tauri/tauri.conf.json`   | Menu built in code; no `.json` menu config |

---

## Architecture

### Tray (`main.rs`)
- `TrayIconBuilder` with default window icon, tooltip "Slackinux"
- Menu: Show Slackinux, Quit (`Ctrl+Q`)
- `on_tray_icon_event`: left-click toggle of window visibility
- Close-to-tray: `CloseRequested` → `prevent_close()` + `hide()`

### App menu
- Built programmatically with `MenuBuilder`/`SubmenuBuilder`
- **View:** Zoom In (`Ctrl++`), Zoom Out (`Ctrl+-`), Actual Size (`Ctrl+0`), Reload (`Ctrl+R`)
- **Account:** Sign in with Browser, Do Not Disturb (added Phase 4)
- Zoom clamped to 0.3x–3.0x via `AtomicU16` ticks

### Zoom (`main.rs`)
- `AtomicU16` tick counter (10 = 100%)
- Applied via WebKit `set_zoom_level()` on the native webview

### Notifications
- `notify-rust` native notifications with appname "Slackinux"
- Click action focuses the window
- WebKitGTK `show-notification` signal → close webkit bubble after showing native

### Downloads (`main.rs`)
- `webkit_web_context_download_started` signal
- Destination set to `<data_dir>/downloads/<filename>`

### Unread badge
- WebKit title callback parses `(N)` prefix from Slack page title
- Tray tooltip becomes `Slackinux (N)` when unread > 0

---

## Known limitations (end of Phase 2)

- No notification action buttons (mark read/reply)
- Notification dedup by tag not yet implemented (Phase 4)
- No DND (Phase 4)
- No settings persistence (Phase 4)
