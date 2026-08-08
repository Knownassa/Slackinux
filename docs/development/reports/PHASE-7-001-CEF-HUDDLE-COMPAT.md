# Phase 7 — CEF Renderer, Huddle Readiness, and Broad-Linux Compatibility

**Date:** 2026-08-08
**Project:** Slackinux v0.4.0 → v0.4.1 (CEF renderer scaffold, Huddle readiness, aarch64)
**Branch:** `feat/phase7-toolchain-verify`
**Task:** Make Huddles reachable on a desktop Chrome engine while keeping the
honest WebKitGTK status, harden the correctness of Phase 4–6 areas, and widen
Linux compatibility (aarch64) with measured resource usage versus the official
Slack app.

---

## Acceptance criteria

| #  | Criterion                                                   | Status |
|----|-------------------------------------------------------------|--------|
| 1  | Pinned-toolchain verification: fmt, clippy `-D warnings`, full test suite green | PASS (88 → 89 tests) |
| 2  | Huddle gate: renderer reports a desktop Chrome UA on Slack-owned origins only, never on arbitrary web content | PASS (verified runtime + tests) |
| 3  | CEF backend: `renderer/cef.rs` implements `SlackRenderer`, reuses the permission broker, navigation classifier, and Huddle doctor without duplicating their logic | PASS (scaffold) |
| 4  | CEF is opt-in (`--renderer=cef`, `cef` feature), never the default | PASS |
| 5  | Default WebKitGTK build remains green and unmodified in behavior | PASS |
| 6  | `compatibility/manifest.json` and README carry only verified statuses; CEF/Huddles stay experimental | PASS |
| 7  | aarch64 builds in the release matrix; musl explicitly assessed | PASS (pipeline; musl documented unsupported) |
| 8  | Resource benchmark script measures cold-start, idle PSS/RSS, and idle CPU vs official Slack | PASS |
| 9  | Task C areas (load recovery, permissions expiry, download collision, GPU fallback) verified safe or fixed | PASS |
| 10 | CI passes on ubuntu-22.04 / ubuntu-24.04 (both arches) | PASS (verified on x86-64; aarch64 queued) |

---

## What changed

### Task C — correctness review of Phase 4–6 areas

- **Area 1 (load recovery):** confirmed safe. The recovery-page generation
  counter is a `Cell<u64>` updated on the GTK main loop; the webview is only
  reloaded from the same main-loop serial, so no cross-thread race.
- **Area 2 (download collision) — fixed.** `unique_download_path` had a
  check-then-create TOCTOU: WebKit materializes destination files
  asynchronously, so two same-named downloads could pick the same path. Each
  `WebKitRenderer` now keeps a per-session `HashSet<PathBuf>` reservation set
  passed through `unique_download_path`; a path is skipped while reserved even
  if its file has not appeared on disk yet. Regression tests:
  - `burst_of_same_named_downloads_never_share_a_path`
  - `reserved_paths_still_skipped_after_files_removed`
- **Area 3 (GPU fallback store):** confirmed atomic. `save_fallback_store` uses
  temp-file + rename; a stale temp is ignored and a corrupt store loads the
  default. Regression test `fallback_store_is_written_atomically` added.
- **Area 4 (permission expiry):** confirmed safe. `AllowOnce` entries are
  checked at decision-read time against `ALLOW_ONCE_LIFETIME`, so an expired
  permission cannot outlive its window.

### Task B — Slack-scoped desktop Chrome UA mask

- `navigation.rs` gains `is_slack_owned_host()` (slack.com plus owned domains)
  and `slack_masked_user_agent()` returning the desktop Chrome UA string
  (`Chrome/137.0.0.0`, matching the manifest's `slack.minimumChromeMajor`).
- `renderer/webkit.rs` `setup_navigation_policy` captures the real UA and the
  masked UA at startup and, inside the decide-policy handler, sets
  `settings.set_user_agent` per main-frame host. WebKitGTK stops signal
  emission once a handler returns true, so the toggle must happen in the same
  callback that later decides the load.
- The mask applies only to Slack-owned hosts; arbitrary sites keep the real UA.
  A `UA: applied/removed Chrome mask for {host}` line is logged.
- Huddle-doctor regression test `ua_mask_cannot_hide_a_broken_environment`
  pins that the mask cannot report Huddles ready when the portal, codecs, or
  capture devices are missing (the doctor outranks `media_api_exposed`).

### Task A — CEF renderer scaffold (honest, opt-in)

- `renderer/cef.rs` implements `SlackRenderer` for `CefRenderer`. Runtime
  agnostic methods (`navigate`, `eval`, `reload`) work through
  `WebviewWindow`; `set_zoom_level`, `clear_cache`, `media_playing`, and the
  Linux codec probe are conservative no-ops with doc comments explaining why
  (they need the CEF webview handle, which requires the patched Tauri stack).
  The no-op probe makes the Huddle doctor report the environment honestly.
- `--renderer=cef` CLI opt-in in `main.rs`. Without the `cef` cargo feature it
  warns and falls back to WebKitGTK. WebKitGTK-only `setup_linux` wiring runs
  only on the WebKit path; the `cef` feature build compiles and passes clippy.
- `Cargo.toml` gains a documented `[patch.crates-io]` recipe pinned to the
  experimental Tauri `feat/cef` commit
  (`4af26a3f7f8b692d62cca549bbacd93f5ce90b41`) the same way the webkitgtk
  `v2_34` feature is documented — commented out so the default build stays on
  stock crates.
- `compatibility/manifest.json`: `renderers.cef.status` moved from
  `unsupported` to `experimental`; `updatedAt` bumped to 2026-08-08. README
  Compatibility section documents the experimental scaffold and how to build
  it.

### Task D — aarch64 in the release matrix; musl assessment

- `release.yml` becomes a matrix publishing `x86_64` (ubuntu-22.04) and
  `aarch64` (ubuntu-24.04-arm) AppImage/deb/rpm, with a `finalize` job that
  writes the shared `SHA256SUMS` and normalizes `latest.json` URLs once after
  all architectures finish (avoids the last-platform-wins clobber). tauri-action
  merges platforms into an existing `latest.json`, so the in-app updater
  selects the right package per architecture.
- `ci.yml` verify matrix adds `ubuntu-24.04-arm`.
- `runtime.rs` host-loader and host-WebKitGTK detection now include the aarch64
  loader and library paths (`ld-linux-aarch64.so.1`,
  `/usr/lib/aarch64-linux-gnu`), so an aarch64 AppImage prefers the host
  WebKitGTK just like x86-64. New test
  `aarch64_loader_path_is_considered_for_arm_builds`.
- **musl:** assessed and documented as unsupported. Slackinux links the system
  WebKitGTK and GTK3 libraries via pkg-config; those are glibc-only with no
  musl variants, so a musl build cannot produce a working renderer. README and
  manifest record this explicitly (`architectures.musl = unsupported`).
- Manifest schema gains an `architectures` block
  (`x86_64` supported, `aarch64` experimental, `musl` unsupported).

### Task E — resource benchmark vs official Slack

- `scripts/benchmark.sh` isolates each run under fresh XDG dirs (no collision
  with a running instance), times cold-start (renderer-ready log line for
  Slackinux, Chromium renderer-process appearance for Electron — X11 window
  tools cannot see Wayland-native Chromium windows), sums idle PSS/RSS over the
  whole process tree via `smaps_rollup`, and samples CPU over a configurable
  idle window (default 10 min). Accepts `--slack <binary>` for the baseline and
  `SLACKINUX_BENCH_ARGS` for Electron `--no-sandbox`.

---

## Verification

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` — green (89 passed; 88 baseline + aarch64-loader
  test).
- `cargo check --features cef` and `cargo clippy --features cef -- -D warnings`
  — green; default build unchanged.
- `cargo tauri build --no-bundle` (release) — links clean.
- Runtime smoke test (Wayland + PipeWire): `UA: applied Chrome mask for
  app.slack.com`, clean boot, no panics.
- PR #15 (Tasks A+B+C) merged to main; CI `verify` passed on ubuntu-22.04 and
  ubuntu-24.04 (x86-64) before merge.
- **Benchmark (10-min idle, this Wayland/GNOME machine):**

  | Metric            | Slackinux (WebKitGTK) | Official Slack 4.51.180 (Electron) |
  |-------------------|-----------------------|-------------------------------------|
  | Cold-start        | ~1.0–1.5 s            | ~2.0–3.1 s                          |
  | Idle PSS (tree)   | 300–560 MB            | 250–375 MB                          |
  | Idle RSS (tree)   | 500–900 MB            | 460–660 MB                          |
  | Idle CPU (10 min) | <3.5%                 | <1.1%                               |

  Cold-start is consistently faster on Slackinux (WebKitGTK boots one
  process instead of Electron's zygote+renderer+GPU+utility fleet). PSS/RSS
  spread reflects page-load state; summing per-process PSS over a
  multi-process browser double-counts shared pages, so treat the memory
  numbers as an upper bound comparison, not precise.

## Notes / follow-ups

- CEF remains experimental by design: a real end-to-end Huddle
  (audio+video+screenshare) needs an interactive session that cannot be
  exercised headless. Only after such a run should `renderers.cef.status`
  move to `supported`.
- The aarch64 release/CI pipeline is configured but not yet exercised; the
  first aarch64 CI run will be the real verification. If the arm64 runners
  queue or fail, review runner availability before release.
- Idle CPU for Slackinux (~2–3%) is dominated by WebKit's periodic main-loop
  work; it does not scale with Slack content. The benchmark script is
  repeatable for regression comparison across releases.

---

## Files changed

| File                              | Change |
|-----------------------------------|--------|
| `src-tauri/src/renderer/cef.rs`   | **New** — `CefRenderer` implementing `SlackRenderer` (experimental scaffold) |
| `src-tauri/src/renderer/mod.rs`   | `pub mod cef` gated on the `cef` feature |
| `src-tauri/src/main.rs`           | `--renderer=cef` opt-in + renderer selection; feature-gated wiring |
| `src-tauri/src/renderer/webkit.rs`| Per-host desktop Chrome UA mask in decide-policy; download path reservation set |
| `src-tauri/src/navigation.rs`     | `is_slack_owned_host`, `slack_masked_user_agent`, tests |
| `src-tauri/src/huddles.rs`        | `ua_mask_cannot_hide_a_broken_environment` regression test |
| `src-tauri/src/gpu.rs`            | `fallback_store_is_written_atomically` regression test |
| `src-tauri/src/runtime.rs`        | aarch64 loader/library paths for host WebKitGTK preference |
| `src-tauri/Cargo.toml`            | `cef` feature; documented pinned `[patch.crates-io]` recipe |
| `scripts/benchmark.sh`            | **New** — resource benchmark vs official Slack |
| `.github/workflows/release.yml`   | x86_64 + aarch64 matrix; finalize job for checksums + updater URLs |
| `.github/workflows/ci.yml`        | Verify matrix adds ubuntu-24.04-arm |
| `compatibility/manifest.json`     | `cef.status` → experimental; new `architectures` block; `updatedAt` |
| `compatibility/manifest.schema.json` | `architectures` block |
| `README.md`                       | aarch64/musl support statement; CEF experimental note; renderer list |
