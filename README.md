<div align="center">

# Slackinux

### Slack on Linux, without the Electron overhead

<img src="SlackinuxAppLogo.png" alt="Slackinux logo" width="160">

An unofficial, resource-conscious desktop shell for Slack Web,<br>
built with Rust, Tauri 2, and WebKitGTK 4.1.

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
webview, wrapped in a Tauri 2 shell — no Electron and no bundled Chromium.
Slackinux is a desktop shell around Slack's official web interface, not a
replacement Slack API client.

> **Not affiliated with or endorsed by Slack Technologies.**

---

## Why Slackinux?

The official Slack desktop client bundles Chromium through Electron. Slackinux
uses the WebKitGTK engine already available on Linux, while keeping the familiar
Slack Web experience and adding desktop integration around it.

| | Slackinux approach |
|---|---|
| **Renderer** | Host WebKitGTK 4.1 when available; AppImage runtime fallback |
| **Desktop integration** | Tray, unread count, notifications, shortcuts, downloads, and custom window chrome |
| **Updates** | Signed GitHub releases with in-app notification and verification |
| **Security boundary** | Remote Slack content receives zero Tauri host capabilities |
| **Distribution** | AppImage, Debian/Ubuntu `.deb`, and Fedora/RHEL/openSUSE `.rpm` |

Slackinux does not reimplement Slack or use unofficial APIs. It provides a
focused native Linux shell around the official web application.

---

## Highlights

| | |
|---|---|
| **Native rendering** | WebKitGTK 4.1 in a compact Rust/Tauri desktop shell |
| **Zero-IPC security** | The Slack webview has an empty Tauri capability set; remote code cannot reach the host |
| **WebRTC** | Origin-restricted camera/microphone access; Huddles remain experimental |
| **Native notifications** | Click a notification to focus the window; Do Not Disturb (`Ctrl+D`) |
| **Signed updates** | GitHub-hosted, signature-verified, install-and-restart from the AppImage |
| **Tray + unread badge** | Close-to-tray, Show/Quit controls, and pending-message count in the tooltip |
| **Native window frame** | Rounded client-side decorations with edge/corner resizing, tiling, and native controls |
| **Downloads** | Saved to the app data directory (`downloads/`) |
| **Persisted settings** | Theme, zoom, DND, GPU preference, and update cadence survive restarts |

---

## Install

Install the latest compatible package from any POSIX shell (`sh`, `dash`,
`bash`, or `zsh`) with:

```sh
curl -fsSL https://raw.githubusercontent.com/Knownassa/Slackinux/main/install.sh | sh
```

The installer automatically chooses DEB on Debian/Ubuntu, RPM on Fedora/RHEL/
openSUSE, and a per-user AppImage elsewhere. Release packages are downloaded to
a temporary path, SHA-256 verified, and only then installed. Download and inspect
[`install.sh`](install.sh) first if you prefer not to pipe a script into a shell.
Use `sh install.sh --help` to select a package format explicitly, or
`sh install.sh --dry-run` to verify compatibility without installing anything.

Alternatively, download an installer manually from the
[latest GitHub release](https://github.com/Knownassa/Slackinux/releases/latest):

- **Debian / Ubuntu** — download the `.deb` and run
  `sudo apt install ./Slackinux_*_amd64.deb`
- **Fedora / RHEL / openSUSE** — download the `.rpm` and install it with your
  distribution's package manager
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
sudo pacman -S webkit2gtk-4.1 gtk3
```

**Debian / Ubuntu:**

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential \
  libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
```

**Build the production binary:**

```bash
cargo install tauri-cli --version "^2"
cd apps/desktop
npm install
cargo tauri build --no-bundle
```

The binary is written to `target/release/slackinux`. Use Tauri's build command
instead of plain `cargo build`: it runs the frontend build and embeds the local
loading/recovery UI in production mode.

**Bundle installers with the Tauri CLI:**

```bash
cargo install tauri-cli --version "^2"
cd apps/desktop/src-tauri

# AppImage (run anywhere, requires libfuse2 or AppImageLauncher)
NO_STRIP=1 cargo tauri build --bundles appimage

# Debian package
cargo tauri build --bundles deb

# RPM package (requires rpmbuild installed)
cargo tauri build --bundles rpm
```

Artifacts land in `target/release/bundle/`. On Arch-based distros set
`NO_STRIP=1` to avoid `.relr.dyn` linker errors.

## Compatibility

Slackinux currently publishes x86-64 Linux packages and requires glibc 2.34 or
newer. Ubuntu 22.04/24.04 and Debian 12+ are the primary supported targets.
Fedora, RHEL 9+, openSUSE, Arch, CachyOS, Manjaro, and EndeavourOS are supported
through the RPM or AppImage paths but receive less CI coverage. Alpine/musl,
32-bit Linux, ARM/aarch64, Windows, macOS, and mobile devices are not currently
supported.

The AppImage prefers a host `libwebkit2gtk-4.1.so.0` when present, which keeps
the browser engine aligned with distribution security updates and avoids mixed
graphics stacks. It falls back to its bundled runtime when the host library is
not installed. Keeping the OS and WebKitGTK packages updated is strongly
recommended.

Messaging, files, notifications, themes, in-app sign-in, SSO, and normal Slack
navigation are the supported core. Authentication stays in Slackinux's
isolated webview; SSO windows share its cookie store and return to the app when
the provider completes authentication. Camera and microphone requests are
allowed only while the top-level page is on a Slack-owned HTTPS origin. Slack
does not officially list WebKitGTK as a supported Huddles browser, so audio,
video, and screen sharing should be treated as experimental.

---

## Usage

| Action | Shortcut |
| --- | --- |
| Zoom in / out / reset | `Ctrl++` / `Ctrl+-` / `Ctrl+0` |
| Reload Slack | `Ctrl+R` |
| Do Not Disturb | `Ctrl+D` |
| Minimize | `Ctrl+M` |
| Quit | `Ctrl+Q` |

The app menu (File, Edit, View, History, Window, Theme, Help, Graphics,
Account) sits at the left of the custom titlebar, with minimize, maximize, and
close buttons on the right. Drag the titlebar to move the window, double-click
to maximize, and right-click it for a window menu. Closing the window hides it
to the tray; use **Show Slackinux** or **Quit** from the tray menu. If your
desktop does not show legacy tray icons, launching Slackinux again restores the
existing window. Unread messages appear in the tray tooltip where supported.

Use **Theme → System**, **Light**, or **Dark** to change the native top panel
and Slack's preferred color scheme. The selection is saved for future launches.

- **Account** — sign in, Do Not Disturb, clear cache & restart
- **View** — zoom and reload controls
- **Window** — minimize, maximize/restore
- **Help** — Check for Updates, automatic-check toggle, Release Notes,
  Diagnostics, About

Use **Help → Diagnostics** to open the rotating log folder, copy a privacy-safe
system summary, or open a pre-filled GitHub bug report. Slackinux never adds
Slack messages, cookies, tokens, or workspace content to the copied summary.

Choose **Account → Sign In / Add Workspace…** to authenticate inside
Slackinux. Workspace SSO opens in a separate in-app window that shares cookies
with the main Slack view. Ordinary workspace and channel `slack://` links are
supported after authentication.

---

## Updates

Slackinux checks the latest GitHub Release about 20 seconds after startup, at
most once per day, and stays silent when offline or when nothing new is
available. **Help → Check for Updates…** checks manually at any time and always
reports the outcome.

How an update is applied depends on how Slackinux was installed:

- **AppImage** — downloads, verifies the signature, installs, and restarts in place
- **`.deb` / `.rpm` / package-managed install** — opens the GitHub release page
  instead of replacing files behind the package manager's back
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

Settings are written automatically; there is no config file to hand-edit.
Diagnostic logs go to `$XDG_STATE_HOME/slackinux/logs/` (normally
`~/.local/state/slackinux/logs/`) as `slackinux.log` and
`slackinux.previous.log`. Use `RUST_LOG=debug` for verbose output.

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
│       ├── diagnostics.rs     # rotating logs and privacy-safe bug reports
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
  notifications, downloads, and audio/video devices. Camera and microphone
  access is denied unless the top-level page is a Slack-owned HTTPS origin.
- Navigations are classified at the WebKitGTK policy level: main-frame Slack
  pages load in-app, external links open in your browser, third-party
  sub-frames (analytics, SSO) load normally, everything else is denied.
- Updater signatures are enforced by Tauri and cannot be disabled; updates
  travel over HTTPS only.

---

## Troubleshooting

- **Blank window on launch** — AppImage rendering uses compatibility-aware
  hardware acceleration and automatically switches to software compositing on
  Wayland/NVIDIA systems where WebKit's EGL process is unsafe. When WebKitGTK
  4.1 is installed on the host, the AppImage uses it instead of mixing its
  bundled runtime with the host graphics stack. A failed or stuck load
  returns to a visible retry screen; use **Help → Diagnostics → Open Log
  Folder** to find the current and previous logs. Start with `RUST_LOG=debug`
  when more detail is needed.
- **The desktop cannot open a `slack://` workspace/channel link** —
  reinstall the current AppImage with `install.sh`, or launch Slackinux once so
  it registers and repairs the deep-link handler for your user account.
- **External sites open in the browser** — only main-frame navigations open
  externally; third-party iframes inside Slack (analytics, SSO) stay in-app.
- **Calls don't work** — Huddles are experimental because Slack does not
  officially support WebKitGTK. WebRTC also needs PipeWire and the desktop
  portal; the startup log reports their detected state.
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

The fastest way to file a useful bug report is **Help → Diagnostics → Report an
Issue…**. Review logs before attaching them; debug logs can contain navigation
details even though Slackinux never uploads logs automatically.

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
