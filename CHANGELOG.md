# Changelog

All notable changes to Slackinux are documented here.

## Unreleased

- Restored dependable edge and corner resizing, compositor shadows, tiling,
  maximization, and rounded window geometry by using GTK's native custom
  titlebar/client-side decoration path.
- Consolidated sign-in and SSO into Slackinux's isolated, cookie-sharing
  webview and popup flow.
- Added a native permission broker: camera, microphone, screen sharing, and
  notifications are never auto-approved. Slackinux asks every time by default
  and offers allow-once, always-allow, and block choices, restricted to
  Slack-owned origins; decisions persist and can be reset from the Media menu.
- Added a Huddle compatibility check (Help → Diagnostics) that classifies the
  environment — PipeWire session, portal ScreenCast, codecs, and input devices
  — and an on-demand "Open Huddle in Browser" fallback that launches a full
  desktop browser using a closed allow-list of known executables.
- AppImage updates now show live download progress and ask before restarting
  to apply the new version.
- Prevented AppImage host-WebKit launches from inheriting incompatible bundled
  GStreamer plugin paths, avoiding another source of blank webviews.
- Made the per-user installer remove obsolete versioned Slackinux launchers
  that could silently restart an older AppImage.
- Fixed recovery pages, AppImage-aware restarts, duplicate download naming,
  and exposed visible DND/automatic-update check states in the app menus.

## 0.2.5 - 2026-08-02

- Fixed AppImage host-runtime selection by bypassing the packaged binary's
  `$ORIGIN/../lib` RUNPATH through the host dynamic loader, ensuring a detected
  host WebKitGTK is actually loaded.

## 0.2.4 - 2026-08-02

- Replaced all launcher, package, tray, repository, and branding icons with the
  latest Slackinux application logo.
- Fixed AppImage blank screens returning after an in-app update by using
  `APPDIR` as the host-WebKit re-exec guard and preferring an installed host
  WebKitGTK 4.1 runtime on all distributions.
- Added `%U` to packaged launchers and a self-healing `slack://` desktop
  handler for ordinary workspace and channel deep links.
- Consolidated authentication into the cookie-sharing in-app sign-in and SSO
  popup flow.
- Added Slack-origin-restricted camera and microphone permissions and bundled
  AppImage media frameworks; active WebKit audio now postpones updates, while
  Huddles remain experimental.
- Made settings and AppImage installation writes atomic, added release SHA-256
  verification, and fixed manually dispatched release tag resolution.

## 0.2.3 - 2026-08-02

- Added **Help → Diagnostics** actions to open persistent rotating logs, copy a
  privacy-safe system summary, and create a pre-filled GitHub bug report.
- Fixed black AppImage windows on Wayland/NVIDIA systems by automatically
  disabling the WebKit EGL compositing path, using the host WebKitGTK runtime
  on Arch-family distributions, and bounding crash recovery.
- Fixed AppImage updates failing with `403 Forbidden` by using public GitHub
  release-download URLs instead of authenticated asset API URLs.

## 0.2.2 - 2026-08-02

- Added a persistent **Theme** menu to the top panel with System, Light, and
  Dark options that update Slack and the native window chrome immediately.
- Added a POSIX-compatible shell installer with automatic DEB, RPM, and
  per-user AppImage selection.
- Standardized the application and Linux package publisher as Knownassa.
- Registered Slackinux as the Linux `slack://` browser callback handler and
  added safe workspace/channel callback navigation for first and running app instances.
- Switched AppImage rendering to WebKit's on-demand acceleration mode and added
  a visible retry screen for failed or stuck Slack page loads.

## 0.2.1 - 2026-08-01

- Replaced every application, launcher, package, and tray icon size with the
  new Slackinux application logo.
- Added retry and GitHub Releases recovery actions when an update-feed request
  is temporarily unavailable or blocked by the network.
- Added an RPM package target and release artifact for RPM-based distributions.

## 0.2.0 - 2026-07-31

- Added signed update checks backed by GitHub Releases.
- Added **Help → Check for Updates…** and **Help → Release Notes**.
- Added an automatic update check ~20 seconds after startup, at most once per
  day, silent when offline or up to date; cadence persisted in settings.
- AppImage builds download, verify, install, and restart in place; package
  manager and development builds open the GitHub release instead.
- Added a GitHub Actions CI workflow (fmt, clippy, tests, frontend build,
  version consistency) and a release workflow that creates signed Linux
  installers and the updater `latest.json` manifest from version tags.
- Added `scripts/set-version.sh` and `scripts/check-version-consistency.sh` to
  keep `package.json`, `Cargo.toml`, and `tauri.conf.json` versions in sync.
- Refined the custom top panel with clearer spacing, theme-aware menu states,
  improved window controls, and double-click maximize/restore behavior.

## 0.1.0 - 2026-07-31

- Initial public Linux release.
- Added a lightweight Tauri 2 and WebKitGTK Slack Web shell.
- Added native notifications, tray controls, downloads, spellcheck, zoom,
  window-state persistence, crash recovery, and WebRTC support.
- Added a custom rounded Linux frame with native window controls.
- Fixed Slack sign-in contrast when page and system themes disagree.
- Removed the artificial strip below the webview.
- Added separate embedded sign-in and Slack Web browser actions.
