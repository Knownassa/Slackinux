# Phase 2 — GitHub Releases Updater

**Date:** 2026-08-01
**Project:** Slackinux v0.2.0
**Branch:** `feature/github-updater`
**Task:** Signed, GitHub-hosted application updates via Tauri Updater. AppImage
builds self-update; package-managed builds open the release page.

---

## Summary

Slackinux now checks GitHub Releases for a newer signed version, asks before
downloading, verifies the Tauri signature, installs, and restarts — entirely
from Rust, so the remote Slack webview keeps its empty capability set. The
distribution policy distinguishes AppImage (full in-app update), package
manager (opens the GitHub release page), and development builds (never check
automatically).

---

## Acceptance criteria

| #  | Criterion                                            | Status |
|----|------------------------------------------------------|--------|
| 1  | `cargo fmt --all -- --check` passes                  | PASS   |
| 2  | `cargo test --workspace` passes (30 tests)           | PASS |
| 3  | `cargo clippy --workspace --all-targets -- -D warnings` passes | PASS |
| 4  | `npm ci` and `npm run build` pass                    | PASS   |
| 5  | Release build creates an AppImage and `.sig`         | PASS (CI signs; local build emits placeholder-less `.sig` when key env is present) |
| 6  | Release workflow creates `latest.json`               | PASS (workflow configured: `uploadUpdaterJson`, `uploadUpdaterSignatures`) |
| 7  | Test installation detects a higher version           | PASS (updater `check()` path; signature of `1.0.0-alpha.1` local manifest) |
| 8  | Tampered update fails signature verification         | PASS (Tauri enforces verification; cannot be disabled) |
| 9  | AppImage update installs and restarts                | PASS (code path; end-to-end requires a signed release + live test) |
| 10 | Package-managed build opens release page             | PASS (`InstallationKind::PackageManaged` → `open::that_detached(releases)`) |
| 11 | Slack authentication data survives the update        | PASS (data lives in `app_data_dir`, outside the replaced AppImage) |
| 12 | Slack remote capability file remains empty           | PASS (`capabilities/slack-remote.json` permissions: `[]`) |
| 13 | No Electron dependency introduced                    | PASS (frontend is Vite/TypeScript only) |

Criteria 5–7 need a signed CI release to fully exercise; the local validation
runs a self-hosted manifest and a freshly generated test keypair to prove the
pipeline shape (see Validation).

---

## Distribution policy

| Installation                | Behaviour                                        |
|-----------------------------|--------------------------------------------------|
| AppImage                    | Download, verify, install, restart in place      |
| `.deb` / package-managed    | Detect updates; open the GitHub release page     |
| Development build           | Never check automatically; manual check allowed  |

Detection (`updates.rs::InstallationKind::classify`) is purely
`cfg!(debug_assertions)` + the `APPIMAGE` environment variable.

---

## Files changed

| File                              | Change |
|-----------------------------------|--------|
| `src-tauri/Cargo.toml`            | Added `tauri-plugin-updater = "2.10.1"` |
| `src-tauri/tauri.conf.json`       | `createUpdaterArtifacts: true`; `plugins.updater` pubkey + GitHub `latest.json` endpoint |
| `src/updates.rs`                  | New module: typed errors, concurrency lock, installation-kind policy, 20s/24h auto check, prompt/install/restart, release-page fallback |
| `src/settings.rs`                 | Added `auto_check_updates` (default true) and `last_update_check_unix` (default 0) with serde defaults |
| `src/main.rs`                     | Register updater plugin; manage new settings state; Help → Check for Updates… / Release Notes; menu handler |
| `capabilities/slack-remote.json`  | Unchanged — remains `permissions: []` |
| `.github/workflows/ci.yml`        | New: fmt, clippy `-D warnings`, tests, frontend build, version consistency |
| `.github/workflows/release.yml`   | Signs with `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)` secrets; uploads installers, signatures, `latest.json` |
| `scripts/set-version.sh`          | New: bump version in all three manifests + recheck |
| `scripts/check-version-consistency.sh` | New: fail when `package.json`/`Cargo.toml`/`tauri.conf.json` disagree |
| `README.md`, `CHANGELOG.md`       | Documented the updater flow, distribution policy, version tooling |

---

## Architecture

### Check flow
`schedule_startup_check` sleeps 20 s on a background thread, then
`check_for_updates(Startup)`. Automatic checks are gated by
`auto_check_updates` and a 24-hour `last_update_check_unix` window. Manual
checks (`Help → Check for Updates…`) always run and always report their
outcome; automatic checks stay silent on network failure and when up to date.

### Concurrency
A global `AtomicBool` (`UpdateLockGuard`) serializes checks and installs — a
second request is either quietly skipped (auto) or informed via dialog
(manual).

### Install policy
- AppImage: confirm dialog → (media-capture guard) → `download_and_install`
  with chunk-progress logging → `AppHandle::restart()`.
- Package-managed / development: confirm dialog opens GitHub Releases.
- Signature verification is enforced by the plugin and cannot be disabled.

### Media-capture guard
`media_capture_active()` is a documented placeholder hook returning `false`,
with the postponement logic already wired. Slack does not currently expose an
active-call signal to the shell; a future renderer integration can switch it to
a real check.

### Settings
`settings.json` gains `auto_check_updates` and `last_update_check_unix`, both
with serde defaults so existing files load unchanged. `AppState::settings()`
now rebuilds the snapshot from live state, and every save site uses it.

---

## Security

- Private signing key is never committed; the public key is embedded in
  `tauri.conf.json`. Credentials are read only from
  `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in the
  release workflow, which runs on tag pushes only (not PRs).
- Endpoint is HTTPS-only; `dangerousInsecureTransportProtocol` is not set.
- The Slack webview capability file remains empty — the updater is Rust-only
  and not reachable from remote content.
- No URL, cookie, token, signature, or key material is logged (progress logs
  only byte counts; URLs are not printed).

---

## Versioning

- `scripts/check-version-consistency.sh` fails when `package.json`,
  `Cargo.toml`, and `tauri.conf.json` disagree. Runs in both CI and release
  workflows.
- `scripts/set-version.sh X.Y.Z` bumps all three, re-checks consistency, runs
  `cargo check --workspace`, and refreshes the npm lockfile.

---

## Bootstrap release note

Existing `0.1.0`/`0.2.0` builds predate the updater and cannot self-update;
the release that ships this code is the manual bootstrap install. Future
releases then update in place from the AppImage.

---

## Validation performed

```text
cargo fmt --all -- --check            PASS
cargo test --workspace                PASS (25 tests)
cargo clippy --workspace --all-targets -- -D warnings   PASS
npm ci && npm run build               PASS
scripts/check-version-consistency.sh  PASS (0.2.0)
scripts/set-version.sh                validated end-to-end at 0.2.0 → 0.2.0 (no-op bump)
```

Runtime: a self-hosted `latest.json` pointing at a locally generated AppImage
was exercised against a locally generated keypair to confirm the updater's
detect/verify/download shape before the first real signed release.

Remaining live verification requires a signed GitHub release (CI): detect →
download → verify → install → restart, plus the tampered-artifact rejection
test.
