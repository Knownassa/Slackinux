# Changelog

All notable changes to Slackinux are documented here.

## 0.2.0 - 2026-07-31

- Added signed update checks backed by GitHub Releases.
- Added automatic update-available notifications after startup.
- Added **Account → Check for Updates** with download, install, and restart.
- Added a GitHub Actions workflow that creates signed Linux installers and
  the updater `latest.json` manifest from version tags.

## 0.1.0 - 2026-07-31

- Initial public Linux release.
- Added a lightweight Tauri 2 and WebKitGTK Slack Web shell.
- Added native notifications, tray controls, downloads, spellcheck, zoom,
  window-state persistence, crash recovery, and WebRTC support.
- Added a custom rounded Linux frame with native window controls.
- Fixed Slack sign-in contrast when page and system themes disagree.
- Removed the artificial strip below the webview.
- Added separate embedded sign-in and Slack Web browser actions.
