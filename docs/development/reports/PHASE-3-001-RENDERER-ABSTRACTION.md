# Phase 3 Task 1 — Renderer Abstraction

**Date:** 2026-07-30  
**Project:** Slackinux v0.1.0  
**Task:** Extract a renderer-agnostic interface so CEF can replace WebKitGTK without rewriting the shell.

---

## Acceptance criteria

| #  | Criterion                                   | Status |
|----|---------------------------------------------|--------|
| 1  | `cargo fmt` passes                          | PASS   |
| 2  | `cargo clippy` with warnings denied         | PASS   |
| 3  | `cargo test` passes                         | PASS   |
| 4  | `cargo build --release` passes              | PASS   |
| 5  | `SlackRenderer` trait defined               | PASS   |
| 6  | `WebKitRenderer` implements the trait       | PASS   |
| 7  | `main.rs` has no WebKitGTK imports          | PASS   |
| 8  | Renderer held behind `Arc<dyn SlackRenderer>` in `AppState` | PASS |
| 9  | All WebKitGTK calls live in `renderer/webkit.rs` | PASS   |
| 10 | Crash recovery moved into renderer layer    | PASS   |

---

## Files changed

| File                          | Change |
|-------------------------------|--------|
| `src/renderer/mod.rs`         | **New** — `SlackRenderer` trait |
| `src/renderer/webkit.rs`      | **New** — `WebKitRenderer` impl, all WebKitGTK calls |
| `src/main.rs`                 | Removed WebKitGTK imports; shell talks only to the trait |

---

## Architecture

### Trait (`src/renderer/mod.rs`)
```rust
pub trait SlackRenderer: Send + Sync {
    fn navigate(&self, url: &str) -> AppResult<()>;
    fn set_zoom_level(&self, level: f64) -> AppResult<()>;
    fn eval(&self, js: &str) -> AppResult<()>;
    fn reload(&self) -> AppResult<()>;
    fn clear_cache(&self) -> AppResult<()>;
}
```

`clear_cache` was added later (see Phase 4/5) to keep the shell free of
webview-backend specifics.

### Implementation (`src/renderer/webkit.rs`)
- `WebKitRenderer { window, download_dir }`
- `setup_linux()` orchestrates, on Linux only:
  - WebRTC via `SettingsExt::set_enable_webrtc`
  - Permission handling (`NotificationPermissionRequest` auto-allow)
  - Crash recovery via `connect_web_process_terminated`
  - Notification bridging via `setup_notification_handler`
  - Title tracking via `connect_title_notify`
  - Download destination via `connect_download_started`
- `navigate` parses the URL and calls `window.navigate`
- `set_zoom_level` uses `with_webview` → `set_zoom_level`
- `reload` maps to `location.reload()`
- `clear_cache` uses `WebsiteDataManager::clear` with `WebsiteDataTypes::ALL`

### Shell (`src/main.rs`)
- `AppState { renderer: Arc<dyn SlackRenderer>, ... }`
- Managed via `handle.manage(AppState { ... })`
- All menu handlers, tray logic, and settings call through the trait object
- No WebKitGTK imports remain in `main.rs`

---

## Rationale

CEF (Chromium Embedded Framework) offers more faithful Slack rendering and
robust WebRTC, at the cost of bundling Chromium. Keeping the shell behind the
trait means swapping renderers touches one file (`renderer/`) plus wiring in
`main.rs`, leaving tray, notifications, downloads, window lifecycle, and
settings untouched.
