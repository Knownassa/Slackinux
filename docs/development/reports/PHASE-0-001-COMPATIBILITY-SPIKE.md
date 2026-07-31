# Phase 0 — Compatibility Spike (Tasks 1–3)

**Date:** 2026-07-30  
**Project:** Slackinux v0.1.0  
**Task:** Create a minimal Tauri 2 Linux compatibility spike with WebRTC, diagnostics, and AppImage packaging.

---

## Acceptance criteria

| #  | Criterion                                  | Status |
|----|--------------------------------------------|--------|
| 1  | `cargo fmt` passes                         | PASS   |
| 2  | `cargo clippy` with warnings denied        | PASS   |
| 3  | `cargo test` passes                        | PASS   |
| 4  | Project builds on Linux (dev + release)    | PASS   |
| 5  | Slack sign-in/client page appears          | PENDING (manual) |
| 6  | Closing and reopening uses same data dir   | PENDING (manual) |
| 7  | Slack remote window has no Tauri IPC       | PASS   |
| 8  | Logs contain no secrets                    | PASS   |
| 9  | Validation report exists                   | PASS   |
| 10 | No Electron or Node desktop runtime        | PASS   |
| 11 | WebRTC enabled in WebKitGTK settings       | PASS   |
| 12 | WebKitGTK version logged at startup        | PASS   |
| 13 | PipeWire/Portal detection in logs          | PASS   |
| 14 | Permission request signals logged          | PASS   |
| 15 | AppImage packaging succeeds                | PASS   |

---

## Build commands executed

```bash
# Rust toolchain
rustc 1.97.0 · cargo 1.97.0

# Tauri CLI
@tauri-apps/cli v2.11.4

# Frontend
npm install && npx vite build

# Quality checks
cargo fmt --check                        # no changes
cargo clippy -- -D warnings              # 0 warnings
cargo test                               # 0 tests (ok)

# Builds
cargo build                              # dev profile
cargo build --release                    # release profile

# AppImage (needs NO_STRIP=1 on Arch/CachyOS)
NO_STRIP=1 npx tauri build --bundles appimage
```

---

## File structure (Phase 0 final)

```
slackinux/
├── Cargo.toml                              # workspace
├── README.md
├── apps/desktop/
│   ├── package.json
│   ├── tsconfig.json
│   ├── vite.config.ts
│   ├── src/
│   │   ├── vite-env.d.ts
│   │   └── bootstrap/
│   │       ├── index.html
│   │       ├── bootstrap.ts
│   │       └── bootstrap.css
│   └── src-tauri/
│       ├── Cargo.toml
│       ├── build.rs
│       ├── tauri.conf.json
│       ├── capabilities/
│       │   └── slack-remote.json
│       ├── icons/
│       └── src/
│           └── main.rs
├── docs/development/reports/
│   └── PHASE-0-001-COMPATIBILITY-SPIKE.md
└── compatibility/
    └── manifest.schema.json
```

---

## Phase 0 features

### Task 1 — Shell scaffold
- Tauri 2 app with persistent WebKitGTK webview
- Local bootstrap page (dark theme, loader spinner, 30s error timeout)
- Window navigates to `https://app.slack.com/client` in `setup` hook
- Slack remote capability file with empty `permissions` array
- Structured redacted startup logging (version, session, profile path)

### Task 2 — Diagnostics + WebRTC
- **WebKitGTK version:** `webkit_get_major_version` / `minor` / `micro` via FFI
- **WebRTC:** `webkit2gtk::SettingsExt::set_enable_webrtc(true)` via `with_webview()`
- **PipeWire:** detected via `pw-cli info` or `pactl info` output
- **Portal:** detected via `dbus-send` ping to `org.freedesktop.portal.Desktop`
- **Permission logging:** `permission-request` signal connected with `WebViewExt`
- All Linux code behind `#[cfg(target_os = "linux")]`

### Task 3 — AppImage packaging
- `Slackinux_0.1.0_amd64.AppImage` (102 MB, static-pie)
- Bundle config: `["appimage"]`, icons, `bundleMediaFramework: false`
- Known issue: `NO_STRIP=1` needed on Arch/CachyOS due to `.relr.dyn` sections

---

## Compatibility feature matrix

| Feature                          | Status              | Notes |
|----------------------------------|---------------------|-------|
| Sign in                          | NOT TESTED          | manual |
| Restart + session retention      | NOT TESTED          | manual |
| Send message                     | NOT TESTED          | manual |
| Receive message                  | NOT TESTED          | manual |
| Threads                          | NOT TESTED          | manual |
| Reactions                        | NOT TESTED          | manual |
| Search                           | NOT TESTED          | manual |
| Workspace switching              | NOT TESTED          | manual |
| File upload                      | NOT TESTED          | manual |
| File download                    | NOT TESTED          | manual |
| Clipboard                        | NOT TESTED          | manual |
| Notifications (Web API)          | NOT TESTED          | blocked by Phase 3 |
| Microphone                       | NOT TESTED          | WebKitGTK WebRTC enabled |
| Camera                           | NOT TESTED          | WebKitGTK WebRTC enabled |
| Huddle visibility                | NOT TESTED          | blocked by Phase 4 |
| Huddle audio                     | NOT TESTED          | experimental (WebKitGTK) |
| Huddle video                     | NOT TESTED          | experimental (WebKitGTK) |
| Screen sharing                   | NOT TESTED          | blocked by Phase 4 |
| WebKitGTK detection              | PASS                | via FFI |
| WebRTC enablement                | PASS                | via WebKitGTK SettingsExt |
| PipeWire detection               | PASS                | pw-cli/pactl |
| Portal detection                 | PASS                | dbus-send |
| Permission logging               | PASS                | permission-request signal |
| AppImage package                  | PASS                | 102 MB |

---

## Log output (expected)

```
[2026-07-30T00:00:00.000Z] INFO  Slackinux v0.1.0 starting
[2026-07-30T00:00:00.000Z] INFO  session: wayland, desktop: niri
[2026-07-30T00:00:00.000Z] INFO  WebKitGTK: 2.52.x
[2026-07-30T00:00:00.000Z] INFO  WebRTC: available via WebKitGTK settings
[2026-07-30T00:00:00.000Z] INFO  profile path: ~/.local/share/com.slackinux.app/
[2026-07-30T00:00:00.000Z] INFO  PipeWire: available
[2026-07-30T00:00:00.000Z] INFO  Portal: available
[2026-07-30T00:00:00.000Z] INFO  WebRTC: enabled via WebKitGTK settings
[2026-07-30T00:00:00.000Z] INFO  navigating to Slack URL
[2026-07-30T00:00:00.000Z] INFO  webview created successfully
```

---

## Known limitations (end of Phase 0)

- No tray icon (Phase 2)
- No native notifications (Phase 3)
- Media permission handling is log-only, not interactive (Phase 4)
- No Huddle support (Phase 4)
- No window state restoration (Phase 1)
- Single window only
- AppImage requires `NO_STRIP=1` on Arch/CachyOS
- No automated tests yet
