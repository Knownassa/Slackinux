# Phase 5 Task 1 — Release Polish & Packaging

**Date:** 2026-07-31  
**Project:** Slackinux v0.1.0  
**Task:** Clear-cache action, About dialog, spellcheck, identifier migration, AppImage + deb bundles.

---

## Acceptance criteria

| #  | Criterion                                   | Status |
|----|---------------------------------------------|--------|
| 1  | `cargo fmt` passes                          | PASS   |
| 2  | `cargo clippy` with warnings denied         | PASS   |
| 3  | `cargo test` passes — 17 tests              | PASS   |
| 4  | `cargo build --release` passes              | PASS   |
| 5  | "Clear Cache & Restart" menu action         | PASS   |
| 6  | About dialog via `tauri-plugin-dialog`      | PASS   |
| 7  | Spellcheck enabled (en_US)                  | PASS   |
| 8  | Identifier `com.slackinux.desktop` (no `.app` warning) | PASS |
| 9  | Legacy `com.slackinux.app` data migrated    | PASS   |
| 10 | AppImage bundle builds (FUSE run verified)  | PASS   |
| 11 | `.deb` bundle target configured             | PASS   |
| 12 | Production smoke test of release binary     | PASS   |
| 13 | Sub-frame navigations stay in-app (no browser popup) | PASS |
| 14 | Main-frame policy applied at WebKitGTK policy level | PASS |

---

## Files changed

| File                          | Change |
|-------------------------------|--------|
| `src/renderer/mod.rs`         | Added `clear_cache` to `SlackRenderer` trait |
| `src/renderer/webkit.rs`      | `clear_cache` impl (`WebsiteDataManager::clear`); `enable_spellcheck` |
| `src/main.rs`                 | Menu item + handler; About dialog; identifier migration helper |
| `src-tauri/Cargo.toml`        | Added `tauri-plugin-dialog` |
| `src-tauri/tauri.conf.json`   | Identifier → `com.slackinux.desktop`; bundle targets → appimage, deb |
| `README.md`                   | Full rewrite: features, build, install, usage, architecture, troubleshooting |

---

## Architecture

### Clear Cache & Restart (`main.rs`)
- Menu handler calls `renderer.clear_cache()` (async `WebsiteDataManager::clear`,
  `WebsiteDataTypes::ALL`, all time)
- Spawns a fresh copy of the current executable, then exits the current process
  after 800 ms so the async clear completes

### About dialog
- `tauri-plugin-dialog`; invoked from Rust (Help menu) — no webview capability
  needed, preserving zero-IPC remote security

### Spellcheck (`renderer/webkit.rs`)
- `WebContextExt::set_spell_checking_enabled(true)`
- `set_spell_checking_languages(&["en_US"])`

### Identifier migration (`main.rs`)
- Legacy dir `com.slackinux.app` → `com.slackinux.desktop`
- On startup, migrates `settings.json` (if new absent) and `downloads/` (rename)

### Bundles
- AppImage with `bundleMediaFramework: false`; `NO_STRIP=1` on Arch/CachyOS
- `.deb` target added; RPM available when `rpmbuild` is installed

### Sub-frame navigation fix
The original `on_navigation` hook fired for iframe loads inside Slack's page
(analytics, SSO session-sync), classifying them as top-level navigations and
opening them in the external browser — which also disrupted the page
(white screen). The policy now lives in the WebKitGTK `decide-policy` signal:

- **Response decisions** — `ResponsePolicyDecision::is_main_frame_main_resource()`
  distinguishes sub-frames from main-frame loads; sub-frames load normally and
  never open the browser
- **Main-frame responses** — classified via `classify_url`: Slack allowed,
  external http(s) opened in browser, everything else blocked
- **New-window actions** — http(s) popups open externally; others blocked
- `NavigationAction` decisions are consumed by Tauri's layer (verified: only
  `Response` decisions reach this handler)

Verified against the live sign-in page:
`csxd.contentsquare.net` and `s.company-target.com` report `main=false`
(sub-frames) and stay in-app; `app.slack.com/client` and `workspace-signin`
report `main=true` and are allowed.

---

## Verified logs (release binary)

```
INFO  Slackinux v0.1.0 starting
INFO  session: wayland, desktop: GNOME
INFO  WebKitGTK: 2.52.5
INFO  WebRTC: available via WebKitGTK settings
INFO  loaded settings: zoom=1x, dnd=false
INFO  PipeWire: available
INFO  Portal: available
INFO  Do Not Disturb: disabled
INFO  WebRTC: enabled via WebKitGTK settings
INFO  Spellcheck: enabled (en_US)
INFO  zoom: 1.0x (saved)
INFO  navigating to Slack URL
INFO  webview created successfully
```

---

## Known limitations (end of Phase 5)

- RPM bundle not verified on this host (no `rpmbuild` installed)
- AppImage requires FUSE or `--appimage-extract-and-run`
- Spellcheck language is hard-coded to en_US
- About dialog has no version-gated changelog
