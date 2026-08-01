<div align="center">

# Slackinux

### Slack on Linux, without the Electron overhead

<img src="branding/slackinux-color.png" alt="Slackinux logo" width="160">

An unofficial, resource-conscious desktop shell for Slack Web,<br>
built with Rust, Tauri 2, and the system WebKitGTK renderer.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/Knownassa/Slackinux)](https://github.com/Knownassa/Slackinux/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/Knownassa/Slackinux/total.svg)](https://github.com/Knownassa/Slackinux/releases)
[![CI](https://github.com/Knownassa/Slackinux/actions/workflows/ci.yml/badge.svg)](https://github.com/Knownassa/Slackinux/actions/workflows/ci.yml)
[![Built with Tauri 2](https://img.shields.io/badge/built%20with-Tauri%202-purple.svg)](https://v2.tauri.app)
[![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Linux-FCC624?logo=linux&logoColor=black)](https://www.kernel.org/)
[![GitHub stars](https://img.shields.io/github/stars/Knownassa/Slackinux?style=flat)](https://github.com/Knownassa/Slackinux/stargazers)

[**Download**](https://github.com/Knownassa/Slackinux/releases/latest)
· [**Features**](#highlights)
· [**Build from source**](#build-from-source)
· [**Security**](#security-model)
· [**Report an issue**](https://github.com/Knownassa/Slackinux/issues/new)

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/G2G8G6CYN)

</div>

**Slackinux** loads the official Slack Web interface in a native WebKitGTK
webview, wrapped in a Tauri 2 shell — no Electron, no bundled Chromium, and no
reverse-engineered Slack APIs. Slack runs exactly as it does in your browser;
Slackinux is the desktop shell around it.

> **Not affiliated with or endorsed by Slack Technologies.**

---

## Why Slackinux?

The official Slack desktop client bundles Chromium through Electron. Slackinux
uses the WebKitGTK engine already available on Linux, while keeping the familiar
Slack Web experience and adding desktop integration around it.

| | Slackinux approach |
|---|---|
| **Renderer** | System WebKitGTK — no bundled browser engine |
| **Desktop integration** | Tray, unread count, notifications, shortcuts, downloads, and custom window chrome |
| **Updates** | Signed GitHub releases with in-app notification and verification |
| **Security boundary** | Remote Slack content receives zero Tauri host capabilities |
| **Distribution** | AppImage for portable use and `.deb` for Debian/Ubuntu |

Slackinux does not reimplement Slack or use unofficial APIs. It provides a
focused native Linux shell around the official web application.

---

## Highlights

| | |
|---|---|
| **Native rendering** | WebKitGTK 4.1 — a fraction of the memory and CPU of Electron |
| **Zero-IPC security** | The Slack webview has an empty Tauri capability set; remote code cannot reach the host |
| **WebRTC** | Audio/video calls and screen sharing via PipeWire and the desktop portal |
| **Native notifications** | Click a notification to focus the window; Do Not Disturb (`Ctrl+D`) |
| **Signed updates** | GitHub-hosted, signature-verified, install-and-restart from the AppImage |
| **Tray + unread badge** | Close-to-tray, left-click toggle, pending-message count in the tooltip |
| **Custom frame** | Frameless rounded window with a native app menu and window controls |
| **Downloads** | Saved to the app data directory (`downloads/`) |
| **Persisted settings** | Zoom, DND, GPU preference, and update cadence survive restarts |

---

## Install

Download the current Linux installers from the
[latest GitHub release](https://github.com/Knownassa/Slackinux/releases/latest):

- **Debian / Ubuntu** — download the `.deb` and run
  `sudo apt install ./Slackinux_*_amd64.deb`
- **Other Linux distributions** — download the `.AppImage`, make it executable
  with `chmod +x Slackinux_*.AppImage`, then run it (if FUSE is unavailable,
  use `--appimage-extract-and-run`)

```bash
chmod +x Slackinux_*.AppImage
./Slackinux_*.AppImage
```

---

## Build from source

Prerequisites: stable Rust, Node.js/npm, and the WebKitGTK 4.1 development
libraries.

**Arch-based systems:**

```bash
sudo pacman -S webkitgtk-6.0 gtk3
```

**Debian / Ubuntu:**

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential \
  libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
```

**Build the debug binary:**

```bash
cd apps/desktop
npm install
cd src-tauri
cargo build --release
```

The binary is written to `target/release/slackinux`.

**Bundle installers with the Tauri CLI:**

```bash
cargo install tauri-cli --version "^2"
cd apps/desktop/src-tauri

# AppImage (run anywhere, requires libfuse2 or AppImageLauncher)
NO_STRIP=1 cargo tauri build --bundles appimage

# Debian package
cargo tauri build --bundles deb
```

Artifacts land in `target/release/bundle/`. On Arch-based distros set
`NO_STRIP=1` to avoid `.relr.dyn` linker errors.

---

## Usage

| Action | Shortcut |
| --- | --- |
| Zoom in / out / reset | `Ctrl++` / `Ctrl+-` / `Ctrl+0` |
| Reload Slack | `Ctrl+R` |
| Do Not Disturb | `Ctrl+D` |
| Minimize | `Ctrl+M` |
| Quit | `Ctrl+Q` |

The app menu (File, Edit, View, History, Window, Help) sits at the left of the
custom titlebar, with minimize, maximize, and close buttons on the right. Drag
the titlebar to move the window, double-click to maximize, and right-click it
for a window menu. Closing the window hides it to the tray; use the tray menu's
**Quit** to fully exit. Unread Slack messages appear as a count in the tray
tooltip.

- **Account** — sign in, open Slack in your browser, Do Not Disturb, clear cache & restart
- **View** — zoom and reload controls
- **Window** — minimize, maximize/restore
- **Help** — Check for Updates, Release Notes, About

Sign-in/SSO popups open in an in-app window that shares cookies with the main
webview, so you can authenticate without losing the session. Slack's
browser-to-desktop `slack://` handoff targets the official client's proprietary
login flow, which Slackinux deliberately does not claim or override.

---

## Updates

Slackinux checks the latest GitHub Release about 20 seconds after startup, at
most once per day, and stays silent when offline or when nothing new is
available. **Help → Check for Updates…** checks manually at any time and always
reports the outcome.

How an update is applied depends on how Slackinux was installed:

- **AppImage** — downloads, verifies the signature, installs, and restarts in place
- **`.deb` / package-managed install** — opens the GitHub release page instead
  of replacing files behind the package manager's back
- **Development builds** — never check automatically

Updates are verified with the public key embedded in Slackinux. The release
workflow signs the AppImage with a private key held only in the repository's
encrypted Actions secrets. Tauri's signature verification cannot be disabled —
a tampered or invalid artifact is rejected before installation.

---

## Data & configuration

All data lives under the platform app-data directory
(`~/.local/share/com.slackinux.desktop/` on Linux):

- `settings.json` — zoom, DND, GPU preference, and update-check settings
  (`auto_check_updates`, `last_update_check_unix`)
- `downloads/` — files saved from Slack

Settings are written automatically; there is no config file to hand-edit. Logs
go to stderr; use `RUST_LOG=debug` for verbose output.

---

## Architecture

```
apps/desktop/
├── src-tauri/
│   └── src/
│       ├── main.rs            # shell: window, tray, menu, shortcuts, settings
│       ├── renderer/mod.rs    # SlackRenderer trait (renderer-agnostic shell)
│       ├── renderer/webkit.rs # WebKitGTK implementation of SlackRenderer
│       ├── navigation.rs      # URL classification engine (allow/deny/open)
│       ├── notifications.rs   # native notifications, DND, click-to-focus
│       ├── settings.rs        # persisted settings
│       ├── updates.rs         # signed GitHub updater (check/install/restart)
│       └── error.rs           # AppError / AppResult (thiserror)
├── src/bootstrap/             # static bootstrap page (spinner + error state)
└── src-tauri/capabilities/    # empty capability set for the Slack webview
```

The shell is written against the `SlackRenderer` trait — `navigate`, zoom,
eval, reload, cache clearing. WebKitGTK specifics are confined to
`renderer/webkit.rs`, so a future renderer backend can replace it without
touching the shell.

Version bumps touch three files (`package.json`, `Cargo.toml`,
`tauri.conf.json`); keep them in sync with `scripts/set-version.sh`, and verify
with `scripts/check-version-consistency.sh` (also run in CI).

---

## Security model

- The Slack webview has **zero Tauri capabilities**. Its only paths to the
  system are the ones a normal browser gives a website — native
  notifications, downloads, and audio/video devices — all routed through
  WebKitGTK's own permission prompts.
- Navigations are classified at the WebKitGTK policy level: main-frame Slack
  pages load in-app, external links open in your browser, third-party
  sub-frames (analytics, SSO) load normally, everything else is denied.
- Updater signatures are enforced by Tauri and cannot be disabled; updates
  travel over HTTPS only.

---

## Troubleshooting

- **Blank window on launch** — check `RUST_LOG=debug`; the bootstrap page has a
  30s timeout and logs before navigating to Slack.
- **External sites open in the browser** — only main-frame navigations open
  externally; third-party iframes inside Slack (analytics, SSO) stay in-app.
- **Calls don't work** — WebRTC needs PipeWire and the desktop portal; the
  startup log reports `PipeWire: available` / `Portal: available`.
- **AppImage fails to mount** — install `libfuse2` or run with
  `--appimage-extract-and-run`.
- **`.relr.dyn` link error on Arch** — rebuild with `NO_STRIP=1`.
- **Spellcheck warnings at startup** — benign; install `aspell`, `hunspell`,
  or `nuspell` plus a dictionary to enable spellcheck.

---

## Contributing

Contributions are welcome. Before making a larger change, open an
[issue](https://github.com/Knownassa/Slackinux/issues) so the approach can be
discussed. For code changes:

1. Fork the repository and create a focused branch.
2. Run `cargo fmt --all`, `cargo test --workspace`, and
   `cargo clippy --workspace --all-targets -- -D warnings`.
3. Open a pull request describing the user impact and how it was tested.

Bug reports should include the Linux distribution, desktop environment,
Wayland/X11 session, installation format, and relevant `RUST_LOG=debug` output.

---

## Support the project

If Slackinux saves you time or system resources, you can support its continued
development on [Ko-fi](https://ko-fi.com/G2G8G6CYN).

Stars, bug reports, documentation fixes, and pull requests help too.

---

## License

Released under the [MIT License](LICENSE).

Not affiliated with or endorsed by Slack Technologies.

<div align="center">
  <sub>Made for the Linux community with Rust, Tauri, and WebKitGTK.</sub>
</div>
