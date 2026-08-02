# Phase 1 — GPU and Rendering Policy (Cross-Linux Compatibility)

**Date:** 2026-08-02
**Project:** Slackinux v0.3.0 → v0.4.0 (cross-Linux compatibility hardening)
**Branch:** `fix/cross-linux-compatibility`
**Task:** Replace the binary GPU behavior with a rendering-mode state machine.
Slackinux must never disable hardware acceleration merely because NVIDIA is
present, because the machine is a hybrid-GPU laptop, or because the session is
Wayland.

---

## Acceptance criteria

| #  | Criterion                                                   | Status |
|----|-------------------------------------------------------------|--------|
| 1  | `cargo fmt --check` passes                                  | PASS   |
| 2  | `cargo clippy -- -D warnings` passes                        | PASS   |
| 3  | `cargo test` passes — 57 tests (was 47)                     | PASS   |
| 4  | `cargo tauri build --no-bundle` (release) links clean       | PASS   |
| 5  | Automatic mode never selects software rendering on NVIDIA/Wayland | PASS (unit-tested) |
| 6  | Automatic mode keeps the system-selected GPU and hardware acceleration | PASS |
| 7  | Efficient vs Performance selection only on X11 PRIME, where app-side selection applies | PASS |
| 8  | Compatibility disables only DMABUF (keeps acceleration)     | PASS   |
| 9  | Software rendering requires explicit user choice or confirmed repeated failure | PASS |
| 10 | Staged crash recovery: reload → DMABUF-off retry → offer software | PASS |
| 11 | Fallback persists only after user confirmation             | PASS   |
| 12 | GPU/session signature is non-sensitive and stable per environment | PASS |
| 13 | Diagnostics report the effective graphics mode + env overrides | PASS |
| 14 | Legacy `gpu_preference` settings file migrates cleanly      | PASS   |

---

## What changed

### `settings.rs`
- New `GraphicsMode` enum with five modes and a `Display` impl:
  `Automatic` (default), `Efficient`, `Performance`, `Compatibility`, `Software`.
- Kept `gpu_preference: Option<LegacyGpuPreference>` as a serde field purely
  for migration. `LegacyGpuPreference { Auto, Integrated, Discrete }` maps to
  `Automatic` / `Efficient` / `Performance` via `From`.
- `Settings::load` folds the legacy field into `graphics_mode` **only when**
  `graphics_mode` is still `Automatic`, so a newer file with an explicit mode
  never loses its explicit choice.
- Tests added/rewritten: defaults, round-trip save/load, corrupt JSON,
  old-format file defaults to automatic, legacy field migration (all three
  values), explicit graphics mode wins over legacy, update fields default when
  absent.

### `gpu.rs` (rewritten)
- Pure policy decision: `choose(mode, gpus, session, desktop)` — no I/O, so it
  is fully unit-testable without `lspci`.
- `Automatic` / `Compatibility` on Wayland keep the compositor's choice and
  only log the DRM devices in use; on X11 with a discrete GPU, Automatic keeps
  the system default (no implicit NVIDIA steering).
- `Efficient` → `DRI_PRIME=0` (integrated) and `Performance` →
  `__NV_PRIME_RENDER_OFFLOAD` / `DRI_PRIME=1` (discrete), both valid only under
  X11. On Wayland both log a notice that the compositor owns GPU selection and
  keep the default — never a crash, never a wrong guess.
- `Compatibility` sets only `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
- `Software` sets `WEBKIT_DISABLE_COMPOSITING_MODE=1` and disables DMABUF.
- `GpuSessionSignature` fingerprints the environment with non-sensitive values
  only: GPU names, drivers, session type, desktop, WebKitGTK version, kernel
  major/minor. `key()` returns a stable FNV-1a hash; no paths or usernames.
- Fallback state persisted per signature in `gpu-fallback.json` (atomic
  temp+rename write). `record_crash` advances the staged recovery state machine:
  1st crash → reload, 2nd → DMABUF-off retry, 3rd+ → offer software. Only the
  counters and the "compatibility retried" flag are persisted — software mode
  persists **only** when the user confirms (`confirm_software`). A clean load
  resets the counters (`record_success`); `reset_troubleshooting` clears all
  per-signature state.
- `apply(mode, data_dir)` runs before WebKit starts any child process, sets
  environment variables, records the resolved `AppliedGraphics` (mode, software
  flag, DMABUF flag, active env overrides) for diagnostics.
- GPU detection parses `lspci -nnk -D`: VGA / 3D / Display controllers, kernel
  driver attribution, vendor classification (Intel always integrated; NVIDIA
  discrete; AMD discrete unless an APU marker like Renoir/Cezanne/Phoenix).

### `main.rs`
- `AppState.gpu_preference` → `graphics_mode: Arc<Mutex<GraphicsMode>>`;
  settings snapshot always writes `gpu_preference: None` (only read for
  migration).
- Graphics menu now has five check items (Automatic / Efficient / Performance /
  Compatibility / Software) plus **Reset Graphics Troubleshooting** and
  **Restart to Apply**. Menu handler updates the mode, saves, refreshes checks,
  and confirms via dialog; reset clears the fallback store.
- `gpu::apply` call now passes `&data_dir`; applied policy logged at startup.

### `renderer/webkit.rs`
- `setup_crash_recovery` now drives the GPU staged recovery state machine
  instead of its own local counter: reload → compatibility retry → offer
  software. `record_success` runs on a clean `LoadEvent::Finished`.
- `offer_software_rendering` shows a dialog ("Switch to Software" / "Keep
  Trying"); only a confirmed switch persists the fallback and restarts the app.
- `enable_webrtc` reads the shared flags: software → `Never` acceleration,
  otherwise `OnDemand`; DMABUF status logged.
- `WebKitRenderer::new` now also receives `data_dir`.

### `diagnostics.rs`
- Support report gains a `Graphics:` line describing the effective policy
  (e.g. `mode=automatic, dmabuf-disabled, env:DRI_PRIME=1`) via
  `gpu::applied()`.

---

## Verification

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (57 passed), `cargo tauri build --no-bundle`
  (release links clean) — all green.
- New unit tests cover: GPU line parsing (Intel i915 + NVIDIA nvidia),
  APU classification, all five modes × session × GPU-table policy decisions,
  the explicit rule that NVIDIA-on-Wayland starts in Automatic (not software),
  signature stability/change, and the full crash-recovery stage progression.

## Notes / follow-ups

- No image input was available for a visual smoke test this session; runtime
  verification of the menu and dialog is recommended on the next interactive
  run (`--no-bundle` binary at `target/release/slackinux`).
- Phase 2 (window chrome/frame) starts from the existing frameless CSD frame in
  `frame.rs`.

---

## Files changed

| File                              | Change |
|-----------------------------------|--------|
| `src-tauri/src/settings.rs`       | `GraphicsMode` + `LegacyGpuPreference` migration, tests |
| `src-tauri/src/gpu.rs`            | Rewritten: mode state machine, GPU detection, session signature, staged crash recovery, fallback store |
| `src-tauri/src/main.rs`           | `AppState.graphics_mode`, 5-item Graphics menu + Reset, apply call, renderer `new` args |
| `src-tauri/src/renderer/webkit.rs` | Crash recovery drives GPU state machine; software-render offer dialog; renderer takes data_dir |
| `src-tauri/src/diagnostics.rs`    | `Graphics:` line in the support report |
