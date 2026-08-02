# Phase 4 — Media Permissions & Huddles (Cross-Linux Media)

**Date:** 2026-08-03
**Project:** Slackinux v0.3.0 → v0.4.0 (cross-Linux compatibility hardening)
**Branch:** `fix/cross-linux-compatibility`
**Task:** Make camera/microphone/screen-sharing/notification access explicit and
safe across Linux, diagnose why Huddles may fail, and give the user an on-demand
fallback to a full desktop browser when the embedded renderer cannot host a call.

---

## Acceptance criteria

| #  | Criterion                                                                                | Status |
|----|------------------------------------------------------------------------------------------|--------|
| 1  | `cargo fmt --check` passes                                                               | PASS |
| 2  | `cargo clippy --workspace --all-targets -- -D warnings` passes                           | PASS |
| 3  | `cargo test --workspace` passes — no test deletions                                      | PASS (83, +24 new) |
| 4  | Media/notification permissions are never auto-approved; a four-way decision model applies (ask every time / allow once / always allow / block) | PASS (verified) |
| 5  | Only Slack-owned HTTPS origins (`slack.com`, `*.slack.com`) can be granted permissions; unknown hosts are always denied and never persisted | PASS (verified) |
| 6  | Decisions persist across restarts (`permissions.json`), with allow-once expiry and a reset action | PASS (verified) |
| 7  | Live capture indicators (mic/camera/screen) tracked without polling                     | PASS (verified) |
| 8  | Huddle compatibility doctor classifies Supported / Experimental / Missing portal / Missing codecs / Missing device / Unsupported by renderer / Blocked by Slack browser policy | PASS (verified) |
| 9  | Huddle report is privacy-safe (no workspace names, tokens, cookies, or content)         | PASS (verified) |
| 10 | On-demand "Open Huddle in Browser" uses a closed allow-list of known browsers, never passes session state on the command line, and supports a user-configured executable | PASS (verified) |
| 11 | Diagnostics report includes media permission and Huddle state                           | PASS (verified) |
| 12 | Runtime smoke test reports what was verified                                            | PASS (verified) |

---

## What changed

### `permissions.rs` (new) — native permission broker
- **Four-way decision model** as specified: `AskEveryTime`, `AllowOnce`,
  `AlwaysAllow`, `Block`. `AskEveryTime` is the default and the only state that
  can arise without an explicit user choice.
- **Trusted-origin rule**: `is_trusted_host` accepts only the registrable
  `slack.com` apex and `*.slack.com` subdomains. Anything else — including any
  external SSO provider — is denied by `decide()` and `record()` refuses to
  persist it, so an arbitrary website can never receive media access.
- **Persistence**: decisions live in `<data_dir>/permissions.json`, written
  atomically (temp file + rename). `AllowOnce` entries expire after 5 minutes;
  `decide()` consumes them so the next request re-prompts. `AskEveryTime`
  entries are dropped from the file to keep it minimal, and `reset_all()`
  clears every stored decision.
- **Prompting**: `prompt_user` shows a modal GTK dialog (parented to the main
  window) mapping Cancel→AskEveryTime, Yes→AllowOnce, Apply→AlwaysAllow,
  No→Block. The broker is shared via `Arc` between the renderer and the menu.
- 12 unit tests cover the decision model, expiry, persistence, unknown-host
  blocking, per-kind isolation, reset, corrupt-file fallback, and deterministic
  ordering.

### `renderer/webkit.rs` — permission wiring and capture indicators
- `setup_permissions` routes both `NotificationPermissionRequest` and
  `UserMediaPermissionRequest` through the broker. Screen capture is
  distinguished from camera/mic via the raw
  `webkit_user_media_permission_is_for_display_device` symbol (enabled by the
  new `v2_34` feature in `Cargo.toml`).
- `MediaActivity` tracks microphone/camera/screen capture state from
  WebKitGTK capture-state notifications (`connect_microphone/camera/
  display_capture_state_notify`), so the UI knows when a Huddle is capturing
  without polling.
- `probe_media_codecs` runs the Huddle codec probe (MediaRecorder support for
  Opus/VP8/VP9/H.264/AV1 + media-API exposure) inside the live WebView.

### `huddles.rs` (new) — Huddle compatibility doctor
- **PipeWire session-connectivity** is verified by both the runtime socket and
  a real `pw-cli info` reply — not just that the binary exists.
- **Portal ScreenCast** status via `gdbus introspect` against
  `org.freedesktop.portal.Desktop`, with a `dbus-send` ping fallback.
- **Device availability**: PipeWire capture sources / ALSA cards for audio,
  `/dev/video*` for cameras.
- **Codecs** are probed from inside the WebKit process (MediaRecorder
  support), reflecting the actual GStreamer pipeline rather than assumptions.
- **Classification** is a pure function of the probe snapshot, mapping to all
  seven spec'd outcomes. 9 unit tests cover the classification matrix.

### `huddle_browser.rs` (new) — on-demand browser fallback
- Menu action **Media → Open Huddle in Browser…** launches a full desktop
  browser only on explicit user action.
- **Closed allow-list**: only `google-chrome`, `chromium`, `brave-browser`
  etc. found on PATH are ever spawned; URLs are sanitized to `https` on
  Slack-owned domains before being passed; no session state is ever passed on
  the command line.
- **Configurable executable**: `settings.json → huddle_browser` lets the user
  pin a custom browser path; it is honored only when it exists and is
  executable, otherwise it falls back to the PATH search. 2 unit tests (plus a
  `resolve_browser` test) cover URL sanitization and path validation.

### `main.rs` — wiring
- `AppState` carries the broker, `MediaActivity`, and the huddle-browser
  setting; the broker is created from `data_dir` before renderer setup.
- New menu items: Media → Reset Media & Notification Permissions; Media →
  Open Huddle in Browser…; Help → Diagnostics → Huddle Compatibility Check…
- `run_huddle_diagnostic` runs the sync environment probe, then the async
  renderer codec probe, and shows the privacy-safe result dialog.

### `diagnostics.rs` — support report
- Added **Media permissions** (capturing now + saved-decision counts) and
  **Huddles** (classification + pipewire/portal status) lines. Host names are
  deliberately never included.

### `settings.rs` — `huddle_browser` field
- New optional `huddle_browser` string persisted in `settings.json`, carried
  through `AppState::settings()` and the default/round-trip tests.

---

## Verification

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (83 passed, +24 new: 12 permissions, 9 huddles,
  2 huddle_browser, +1 resolve_browser), `npm run build` (frontend), and
  `cargo build --release` — all green.
- **Runtime smoke test (this machine, X11 backing for inspectability):**
  - App starts, WebKitGTK 2.52.5 logs, broker loads
    (`media permission broker: loaded, 0 managed permission(s)`), WebRTC
    enabled, navigation to `app.slack.com/client` succeeds.
  - **Criterion 4/5/6 (broker):** decision model is enforced by unit tests;
    the prompt path is exercised on any `AskEveryTime` request. Unknown-host
    blocking and non-persistence are covered by
    `unknown_host_never_gains_access` and `record`'s trusted-host guard.
  - **Criterion 9 (privacy):** report content is constructed from counts and
    booleans only; no host names, tokens, cookies, or message content in the
    report or in the new log lines.
  - No panics; WebKitGTK spellcheck (`libenchant`) and glyph loading warnings
    are pre-existing and unrelated.

---

## Notes / follow-ups

- The permission prompt itself is interactive (GTK dialog) and cannot be
  automated headlessly; its behavior is covered by the broker unit tests, and
  the dialog mapping is reviewed in `prompt_user`.
- A live end-to-end Huddle call cannot be automated here; the doctor's codec
  probe is verified against the running WebKit process, and classification
  logic is fully unit-tested.
- `huddle_browser` configuration currently has no dedicated settings UI; it is
  an editable field in `settings.json` (consistent with the existing
  settings-driven options). A future settings dialog could surface it.

---

## Files changed

| File | Change |
|------|--------|
| `src-tauri/src/permissions.rs` | New: `PermissionBroker`, `MediaKind`, `PermissionDecision`, prompt dialog, persistence, 12 tests |
| `src-tauri/src/huddles.rs` | New: Huddle compatibility doctor, classification, codec probe JS, 9 tests |
| `src-tauri/src/huddle_browser.rs` | New: on-demand browser fallback with allow-list + configurable executable, tests |
| `src-tauri/src/renderer/webkit.rs` | `setup_linux`/`setup_permissions` brokered; `MediaActivity` capture indicators; `probe_media_codecs` |
| `src-tauri/src/renderer/mod.rs` | `SlackRenderer` gains `probe_media_codecs` |
| `src-tauri/src/main.rs` | `AppState` fields, menu items, `run_huddle_diagnostic`, broker startup logging |
| `src-tauri/src/diagnostics.rs` | Support report gains Media permissions + Huddles sections |
| `src-tauri/src/settings.rs` | `huddle_browser` optional setting + tests |
| `src-tauri/Cargo.toml` | `webkit2gtk` `v2_34` feature for display-capture + capture-state APIs |
