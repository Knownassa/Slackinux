# Slackinux

An unofficial, resource-conscious Linux desktop shell for Slack Web.

Slackinux loads the official Slack Web interface in a native WebKitGTK
webview, wrapped in a Tauri 2 shell — no Electron, no bundled Chromium, and
no reverse-engineered Slack APIs. Slack runs entirely as it does in your
browser; Slackinux provides the desktop shell around it.

**Not affiliated with or endorsed by Slack Technologies.**

## Features

- **Native rendering** — WebKitGTK 4.1, not Electron. Low memory and CPU footprint
- **Zero-IPC security** — Slack's webview has an empty Tauri capability set; no
  remote code can reach the host through the shell
- **WebRTC** — audio/video calls and screen sharing work through PipeWire/portals
- **Native notifications** — click a Slack notification to focus the window
- **Do Not Disturb** (`Ctrl+D`) — suppress Slack notifications system-wide
- **Unread badge** — tray tooltip shows the pending-message count from the title
- **Tray support** — close-to-tray, left-click toggle, "Show/Quit" menu
- **Zoom controls** (`Ctrl++` / `Ctrl+-` / `Ctrl+0`) — persisted across restarts
- **Downloads** — saved to the app data directory (`downloads/`)
- **Spellcheck** — enabled (en_US) in the webview
- **Crash recovery** — the WebKit web process auto-reloads on crash
- **Clear cache & restart** — wipe all Slack website data from the Account menu
- **Signed updates** — checks GitHub Releases for a newer version about 20
  seconds after startup (at most once per day), notifies when one exists, and
  installs verified updates from the Help menu
- **Single instance** — launching again focuses the existing window
- **Window state** — geometry is restored between sessions
- **Custom frame** — frameless transparent window with all four corners
  rounded; the app menu sits at the left of the titlebar with minimize,
  maximize, and close buttons on the right (drag to move, double-click to
  maximize, right-click for a window menu); chrome and web content
  automatically follow the system light/dark scheme
- **Security model** — navigations are classified at the WebKitGTK policy level:
  main-frame Slack pages load in-app, external links open in your browser,
  third-party sub-frames (analytics, SSO) load normally, everything else is
  denied; sign-in/SSO popups open in-app so cookies stay shared

## Build

Prerequisites:

- Rust (stable) and Cargo
- Node.js and npm
- WebKitGTK 4.1 development libraries
- Linux packaging tools (see below)

On Arch-based systems (WebKitGTK 4.1):

```bash
sudo pacman -S webkitgtk-6.0 gtk3
```

On Debian/Ubuntu:

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential \
  libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
```

Build:

```bash
cd apps/desktop
npm install          # frontend tooling (vite + bootstrap page)
cd src-tauri
cargo build --release
```

The binary is written to `target/release/slackinux`.

## Install

Download the current Linux installers from the
[latest GitHub release](https://github.com/Knownassa/Slackinux/releases/latest):

- **Debian/Ubuntu:** download the `.deb`, then run
  `sudo apt install ./Slackinux_*_amd64.deb`
- **Other Linux distributions:** download the `.AppImage`, make it executable
  with `chmod +x Slackinux_*.AppImage`, then run it

Bundle artifacts are produced with the Tauri CLI:

```bash
cargo install tauri-cli --version "^2"
cd apps/desktop/src-tauri

# AppImage (run anywhere, requires libfuse2 or AppImageLauncher)
NO_STRIP=1 cargo tauri build --bundles appimage

# Debian package
cargo tauri build --bundles deb

# RPM (requires rpmbuild installed)
cargo tauri build --bundles rpm
```

Artifacts land in `target/release/bundle/`. On Arch-based distros set
`NO_STRIP=1` for AppImage builds to avoid `.relr.dyn` linker errors.

Run the AppImage directly:

```bash
./Slackinux_0.2.0_amd64.AppImage
```

If FUSE is unavailable, use `--appimage-extract-and-run`. Install the `.deb`
with `sudo apt install ./Slackinux_0.2.0_amd64.deb`.

## Usage

| Action | Shortcut |
| --- | --- |
| Zoom in / out / reset | `Ctrl++` / `Ctrl+-` / `Ctrl+0` |
| Reload Slack | `Ctrl+R` |
| Do Not Disturb | `Ctrl+D` |
| Minimize | `Ctrl+M` |
| Quit | `Ctrl+Q` |

Menus:

- **Account** — sign in to Slackinux, open Slack Web in your browser, toggle
  Do Not Disturb, clear cache & restart
- **View** — zoom and reload controls
- **Window** — minimize, maximize/restore (accelerators work without a visible menubar)
- **Help** — Check for Updates, Release Notes, About Slackinux

The window is frameless and transparent, with all four corners rounded. The
app menu (Account, View, Window, Help) is shown at the left of the titlebar,
with minimize, maximize, and close buttons on the right. Drag the titlebar to
move the window, double-click to maximize, and right-click it for a window
menu (minimize, maximize/restore, close to tray, quit). The **Window** menu
still offers minimize and maximize/restore shortcuts (`Ctrl+M`). Closing the
window hides it to the tray. Left-click the tray icon to toggle the window,
or use the tray menu's "Quit" to fully exit. Unread Slack messages appear as
a count in the tray tooltip.

The UI follows the system theme automatically: on GNOME it reads
`org.gnome.desktop.interface` `color-scheme`/`gtk-theme` (live, via
GSettings) and applies dark/light to both the titlebar and the web content.
Hardware acceleration is forced and smooth scrolling enabled in WebKitGTK for
a responsive UI.

Sign-in/SSO popups (workspace sign-in, Google, etc.) open in an in-app window
that shares cookies with the main webview, so sign-in can complete without
losing the app session. Use **Account → Sign In to Slack** to authenticate
Slackinux. **Account → Open Sign-In in Browser** opens Slack Web in your
system browser; it does not transfer that browser session into Slackinux.
Slack's browser-to-desktop handoff targets the official client's proprietary
`slack://` login flow, which Slackinux deliberately does not claim or override.

## Data & Configuration

All data lives under the platform app-data directory
(`~/.local/share/com.slackinux.desktop/` on Linux):

- `settings.json` — persisted zoom level, DND state, and update-check settings
  (`auto_check_updates`, `last_update_check_unix`)
- `downloads/` — files saved from Slack

Settings are written when you change zoom or toggle DND; there is no config
file to hand-edit. Logs go to stderr; use `RUST_LOG=debug` for verbose output.

## Updates

Slackinux checks the latest GitHub Release about 20 seconds after startup, but
never more than once per day, and stays silent when offline or when nothing new
is available. **Help → Check for Updates…** checks manually at any time and
always reports the outcome.

How an update is applied depends on how Slackinux was installed:

- **AppImage** — downloads, verifies the signature, installs, and restarts
  in place.
- **Debian/Ubuntu (`.deb`) or any package-managed install** — opens the GitHub
  Release page instead of replacing files behind the package manager's back.
- **Development builds** — never check automatically.

Updates are verified with the public key embedded in Slackinux. Release builds
and `latest.json` are generated by the GitHub Actions release workflow, which
signs the AppImage with the private key stored in the repository's encrypted
Actions secrets. Tauri's signature verification cannot be disabled; a tampered
or invalid artifact is rejected before installation.

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
├── src/bootstrap/             # static bootstrap page (dark theme + spinner)
└── src-tauri/capabilities/    # empty capability set for the Slack webview
```

Version bumps touch three files (`package.json`, `Cargo.toml`,
`tauri.conf.json`); keep them in sync with `scripts/set-version.sh`, and verify
with `scripts/check-version-consistency.sh` (also run in CI).

The shell is written against the `SlackRenderer` trait — `navigate`, zoom,
eval, reload, cache clearing. WebKitGTK specifics are confined to
`renderer/webkit.rs`, so a future CEF backend can replace it without touching
the shell (tray, notifications, downloads, window lifecycle).

Security: the Slack webview has zero Tauri capabilities. Its only paths to
the system are the ones a normal browser gives a website — native
notifications, downloads, and audio/video devices — all routed through
WebKitGTK's own permission prompts.

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

## License

Slackinux is released under the [MIT License](LICENSE).

Not affiliated with or endorsed by Slack Technologies.
