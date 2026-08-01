# Changelog

All notable changes to Slackinux are documented here.

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
