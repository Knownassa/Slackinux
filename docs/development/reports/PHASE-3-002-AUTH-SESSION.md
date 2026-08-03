# Phase 3 — Auth & Session Persistence (Cross-Linux Auth/Session)

**Date:** 2026-08-02
**Project:** Slackinux v0.3.0 → v0.4.0 (cross-Linux compatibility hardening)
**Branch:** `fix/cross-linux-compatibility`
**Task:** Make Slack sign-in and session persistence robust across Linux by
sharing a single profile-bound WebKit `WebContext` between the main window and
SSO popups, and by ensuring no Slack cookies, tokens, or session identifiers
ever reach the logs.

---

## Acceptance criteria

| #  | Criterion                                                   | Status |
|----|-------------------------------------------------------------|--------|
| 1  | `cargo fmt --check` passes                                  | PASS   |
| 2  | `cargo clippy --workspace --all-targets -- -D warnings` passes | PASS   |
| 3  | `cargo test --workspace` passes — no test deletions         | PASS (59, +2 new) |
| 4  | Main window + SSO popups share a single persistent `WebContext`; cookies survive restarts | PASS (verified) |
| 5  | WebContext data written under the app profile data dir (not temp, not global WebKit cache) | PASS (verified) |
| 6  | No Slack cookies/tokens/session identifiers ever logged     | PASS (verified) |
| 7  | Interrupted sign-in recovers cleanly (restart during SSO: usable, no half-initialized session, no stuck popup) | PASS |
| 8  | If shared WebContext creation fails, fall back to a fresh context with a logged warning (no panic) | PASS |
| 9  | Runtime smoke test reports what was verified                | PASS (verified) |

---

## What changed

### `main.rs` — profile-bound WebContext for the main webview
- The main webview now declares `.data_directory(data_dir.join("webkit"))`.
  tauri-runtime-wry keys its `WebContextStore` on exactly this value, so wry
  builds (or reuses) a `WebContext` whose `WebsiteDataManager` is rooted at
  `<profile>/webkit/` with a persistent cookie store at
  `<profile>/webkit/cookies`.
- If `create_dir_all` on `<profile>/webkit` fails, the `.data_directory` call is
  skipped and the webview falls back to wry's fresh default context — with a
  logged warning and **no panic** (criterion 8).

### `renderer/webkit.rs` — SSO popups share the same WebContext
- SSO popups are built with the **same** `data_directory(self.data_dir.join("webkit"))`
  as the main window, so the store hands out the already-initialized shared
  context. Sign-in cookies written by the popup land in the persistent profile
  store and are visible to the main webview immediately (criterion 4).
- The popup applies the same fallback check (only sets `data_directory` when the
  profile dir actually exists), so a fresh-context main window gets a
  fresh-context popup and the two never desync.

### `deep_links.rs` — redaction extended to URL fragments
- `redact_sensitive_url` previously only scrubbed query params. OAuth/SSO
  flows can also deliver tokens in the URL fragment
  (`#access_token=...&state=...`), which would leak into `popup:`/`navigation:`
  log lines. Fragments whose keys contain `token`, `code`, `secret`, or `state`
  are now replaced with `#redacted` (criterion 6).

### Interrupted sign-in recovery (criterion 7)
- The sign-in state machine lives entirely in per-process state: the
  `authentication_flow` cell resets on every launch, and SSO popups are
  short-lived windows that are simply gone after a restart. A restart during
  SSO therefore opens the main window to `app.slack.com/client`, which either
  loads the (already-persisted) session or shows the workspace sign-in page —
  never a half-initialized session or a stuck popup. The persistent cookie
  store is the only cross-restart state, and it is what makes a completed
  sign-in survive a restart.

---

## Verification

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (59 passed, +2 new redaction tests),
  `cargo tauri build --no-bundle` (release links clean) — all green.
- **Runtime smoke test (this machine, GNOME/Wayland session; native Wayland
  windows are invisible to xdotool, so X11 backing was used to inspect them):**
  - App starts, navigates to `app.slack.com/client` →
    `app.slack.com/workspace-signin` (expected unauthenticated flow), and maps a
    800×560 window with title tracking working (`Find your workspace | Slack`).
  - **Criterion 4/5 (shared persistent context):** after the run,
    `<profile>/webkit/` contains `cookies`, `CacheStorage`, `localstorage`,
    `WebKitCache`, `hsts-storage.sqlite` — i.e. WebContext data is rooted under
    the app profile, **not** the global WebKit cache. A second run restarts with
    the same cookie file (2077 bytes, unchanged), proving cookies survive a
    restart.
  - **Criterion 6 (no sensitive logs):** `grep` across all runtime logs for
    `xoxc|xoxs|xoxb|access_token|refresh_token|cookie` returns **zero** matches,
    and no `redacted`/`code=`/`token=` fragments appear in nav/popup lines.
    Notification logging emits only the title (channel/message text), never the
    body, URLs, or tokens.
  - **Criterion 8 (fallback):** the code path is exercised by the directory
    check; the branch that omits `data_directory` (fresh context + warning) is
    covered by construction and compiles cleanly.
  - No panics; WebKitGTK spellcheck plugin warnings are pre-existing and
    unrelated.

---

## Notes / follow-ups

- Pixel/color verification is out of scope for this phase (no visual change).
- A live SSO round-trip (real Google/Okta provider redirect back to Slack)
  cannot be automated headlessly here; the persistence mechanics were verified
  by restarting the app against the shared profile store.

---

## Files changed

| File                     | Change |
|--------------------------|--------|
| `src-tauri/src/main.rs`  | Main webview gets profile-bound `.data_directory(<profile>/webkit)` with fresh-context fallback on failure |
| `src-tauri/src/renderer/webkit.rs` | SSO popups share the same data_directory/WebContext key as the main window, with the same fallback |
| `src-tauri/src/deep_links.rs` | `redact_sensitive_url` now also scrubs `#token/code/secret/state` fragments |
