# Phase 1 Task 1 — Secure Shell

**Date:** 2026-07-30  
**Project:** Slackinux v0.1.0  
**Task:** Error model, navigation policy, single-instance support, crash recovery.

---

## Acceptance criteria

| #  | Criterion                                    | Status |
|----|----------------------------------------------|--------|
| 1  | `cargo fmt` passes                           | PASS   |
| 2  | `cargo clippy` with warnings denied          | PASS   |
| 3  | `cargo test` passes — 12 navigation tests    | PASS   |
| 4  | `cargo build --release` passes               | PASS   |
| 5  | Typed error model with `thiserror`           | PASS   |
| 6  | Window created programmatically with builder | PASS   |
| 7  | Navigation policy blocks external URLs       | PASS   |
| 8  | Navigation policy allows Slack domains       | PASS   |
| 9  | Navigation policy opens mailto/tel externally| PASS   |
| 10 | Single-instance plugin registered            | PASS   |
| 11 | Crash recovery (web process terminated)      | PASS   |
| 12 | Logs redact sensitive data                   | PASS   |

---

## Files changed

| File                          | Change |
|-------------------------------|--------|
| `src/main.rs`                 | Major restructure: programmatic window, typed errors, navigation handler, single instance, crash recovery |
| `src/error.rs`                | **New** — typed error enum with `thiserror` |
| `src/navigation.rs`           | **New** — URL classification engine with 12 tests |
| `src-tauri/Cargo.toml`        | Added `thiserror`, `open`, `tauri-plugin-single-instance` |
| `src-tauri/tauri.conf.json`   | Removed static window definition → created programmatically |
| `docs/.../PHASE-1-001-SECURE-SHELL.md` | **New** — this report |

---

## Architecture

### Error model (`src/error.rs`)
- `AppError` enum with variants: `NavigationFailed`, `InvalidUrl`, `Tauri`, `Io`
- `AppResult<T>` type alias for `Result<T, AppError>`
- `#[from]` implementations for `tauri::Error` and `std::io::Error`
- Reserved variants for future use: `WindowNotFound`, `PathResolution`, `Plugin`, `Other`

### Navigation policy (`src/navigation.rs`)
```rust
pub enum NavigationDecision { AllowInternal, OpenExternally, Deny }

pub fn classify_url(url: &Url) -> NavigationDecision
```
- **AllowInternal**: `app.slack.com`, `*.slack.com`, `slack.com`, `www.slack.com`
- **OpenExternally**: external `http`/`https`, `mailto`, `tel`
- **Deny**: `file`, `javascript`, `data`, unknown schemes, no-host URLs
- Case-insensitive host matching

### Window creation (`main.rs:61-82`)
- `WebviewWindowBuilder` creates window with `on_navigation` closure
- External URLs open in system browser via `open::that_detached`
- Blocked navigations are logged as warnings

### Single-instance (`main.rs:31-36`)
- `tauri-plugin-single-instance` registered
- Second instance calls `window.set_focus()` on existing window

### Crash recovery (`main.rs:220-251`)
- `connect_web_process_terminated` signal on WebKitGTK `WebView`
- Handles `Crashed`, `ExceededMemoryLimit`, `TerminatedByApi`
- Crashes and memory limit exceeded trigger `webview.reload()`

---

## Navigation tests (12)

```
allows_app_slack_com           ✓
allows_slack_subdomains        ✓
allows_slack_com               ✓
allows_www_slack_com           ✓
allows_case_insensitive_slack  ✓
opens_external_http            ✓
opens_mailto_externally        ✓
opens_tel_externally           ✓
denies_file_scheme             ✓
denies_javascript_scheme       ✓
denies_data_scheme             ✓
denies_no_host_urls            ✓
```
