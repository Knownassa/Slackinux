# Phase 4 Task 1 — Notifications & Settings

**Date:** 2026-07-30  
**Project:** Slackinux v0.1.0  
**Task:** NotificationManager (DND, dedup, click-to-focus) and persisted settings (zoom, DND).

---

## Acceptance criteria

| #  | Criterion                                   | Status |
|----|---------------------------------------------|--------|
| 1  | `cargo fmt` passes                          | PASS   |
| 2  | `cargo clippy` with warnings denied         | PASS   |
| 3  | `cargo test` passes — 17 tests              | PASS   |
| 4  | `cargo build --release` passes              | PASS   |
| 5  | DND flag toggled from Account menu (`Ctrl+D`) | PASS |
| 6  | DND suppresses WebKit `show-notification`   | PASS   |
| 7  | Notification click focuses window           | PASS   |
| 8  | Notification dedup by tag hash              | PASS   |
| 9  | `Hint::Resident` + `Hint::Category` set     | PASS   |
| 10 | Settings load/save to `settings.json`       | PASS   |
| 11 | Zoom persisted across restarts              | PASS   |
| 12 | DND persisted across restarts               | PASS   |

---

## Files changed

| File                          | Change |
|-------------------------------|--------|
| `src/notifications.rs`        | **New** — `NotificationManager`, tag dedup, click-to-focus |
| `src/settings.rs`             | **New** — `Settings { zoom_level, dnd }` + load/save + 3 tests |
| `src/main.rs`                 | DND toggle, settings wiring, save on zoom/DND change |
| `src/renderer/webkit.rs`      | Suppress notifications when DND; `clear_cache` on trait |

---

## Architecture

### NotificationManager (`src/notifications.rs`)
- `dnd: AtomicBool` — thread-safe flag
- `set_dnd()` / `is_dnd()`; toggling logs state
- WebKitGTK `show-notification` handler:
  - DND → `wkn.close()` and suppress (no native popup)
  - Otherwise render a `notify-rust` notification:
    - `Hint::Resident(true)` so it stays until dismissed
    - `Hint::Category("im.received")` for messaging-style grouping
    - `action("default", "Open Slackinux")` → click focuses window
    - Tag-hash `id()` for dedup (Slack tags messages/channels)
- If `notify-rust` fails, falls back to the WebKitGTK bubble

### Settings (`src/settings.rs`)
```rust
pub struct Settings { pub zoom_level: u16, pub dnd: bool }
```
- `load()` reads `settings.json`; corrupt/missing → `Default` (zoom 1.0x, DND off)
- `save()` writes pretty JSON
- 3 unit tests: missing → defaults, round-trip, corrupt JSON → defaults

### Wiring (`src/main.rs`)
- Settings loaded in `setup`, applied: `notif_mgr.set_dnd(...)`, zoom stored
- Saved zoom applied to renderer before navigating to Slack
- Zoom change and DND toggle both persist immediately

---

## Settings tests (3)

```
defaults_when_missing           ✓
round_trip_save_load            ✓
defaults_on_corrupt_json        ✓
```

---

## Data dir layout

```
~/.local/share/com.slackinux.desktop/
├── settings.json               # zoom_level, dnd
└── downloads/                  # files saved from Slack
```

Identifier migration: prior builds used `com.slackinux.app`; startup
auto-migrates `settings.json` and `downloads/` to the new dir if present.
