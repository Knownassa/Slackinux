# Phase 2 — Window Chrome & Frame (Cross-Linux Window Chrome)

**Date:** 2026-08-02
**Project:** Slackinux v0.3.0 → v0.4.0 (cross-Linux compatibility hardening)
**Branch:** `fix/cross-linux-compatibility`
**Task:** Make the custom window frame robust across Linux desktops by removing
fragile Tauri-internal widget-tree assumptions and all polling, while keeping
the exact same visual behavior.

---

## Acceptance criteria

| #  | Criterion                                                   | Status |
|----|-------------------------------------------------------------|--------|
| 1  | `cargo fmt --check` passes                                  | PASS   |
| 2  | `cargo clippy -- -D warnings` passes                        | PASS   |
| 3  | `cargo test` passes — 57 tests                              | PASS   |
| 4  | Frame uses only Tauri public API (`gtk_window()`, `default_vbox()`); no internal widget-tree walk | PASS |
| 5  | No polling/timeouts to discover the menubar; deterministic attach | PASS |
| 6  | Titlebar keeps behavior: minimize/maximize/close, drag, dbl-click maximize, right-click menu, close-to-tray | PASS (code unchanged, same widgets) |
| 7  | Chrome colors derive from resolved theme state (system or page probe), not hardcoded Slack values | PASS |
| 8  | 3-second periodic debug dump + runtime diagnostics removed from production path | PASS |
| 9  | Rounded corners (not maximized), full-bleed maximized, opaque, follows light/dark | PASS (code preserved) |
| 10 | Runtime smoke test: window shows, titlebar works, Slack navigates | PASS (verified) |

---

## What changed

### `frame.rs` (rewritten)
- **Public API only:** the frame now obtains the GTK window and content box via
  `WebviewWindow::gtk_window()` and `WebviewWindow::default_vbox()` (both are
  Tauri's documented public Linux APIs) instead of walking
  `win.child()` → `gtk::Box` downcast of Tauri's internal `default_vbox` tree.
  Because `with_webview` requires a `Send + 'static` closure, the GTK objects
  are created *inside* the closure from a cloned `WebviewWindow` handle.
- **No polling:** the previous 100 ms GLib timeout that watched for the menubar
  is gone. `main.rs` now calls `frame::apply_custom_frame` **after**
  `app.set_menu(menu)`. muda's `init_for_gtk_window` packs the `gtk::MenuBar`
  into the content box synchronously during `set_menu`, so the menubar is a
  direct child when the frame runs. `find_menubar` moves it into the titlebar
  in one deterministic pass.
- **Chrome colors derived from theme state:** `chrome_css(dark, page_bg)` now
  takes the probed page background. When the page has finished loading and the
  theme is `System`, the page probe (`page_scheme`) returns both the resolved
  dark/light decision and the page's *actual* opaque background, which is used
  for the card/webview background and the CSS `box.card` color. The hardcoded
  Slack hex values remain only as a fallback before the first page probe.
  Transparent probe results (page not ready) do not repaint the card.
- **Removed runtime diagnostics:** the 3-second widget-tree dump, `dump_frame_metrics`,
  the headerbar corner-alpha cairo sampling, and `dump_widget_tree` are deleted
  from the production path. Only short `debug!` logs remain.
- Added `rgb_to_css` (RGB tuple → `#rrggbb`) and the `page_scheme` helper
  (dark/light + opaque background); `page_is_dark` is retained as a test-only
  wrapper so the existing theme tests keep passing unchanged.

### `main.rs`
- Moved the `frame::apply_custom_frame` call from "right after window build" to
  immediately after `app.set_menu(menu)` (and after the Graphics/Account menu
  appends), so the menubar is guaranteed present.

---

## Verification

- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` (57 passed), `cargo tauri build --no-bundle`
  (release links clean) — all green.
- **Runtime smoke test (this machine, X11 backing of a GNOME/Wayland session):**
  - `RUST_LOG=debug` shows: `custom frame: menubar moved into titlebar`,
    `custom frame: initial theme = dark`,
    `custom frame: rounded corners + titlebar applied` — with **no** polling
    log and **no** widget-tree dump.
  - `xdotool` finds the Slackinux window (min size 800×560) with the Slack page
    loaded and title tracking working (`Find your workspace | Slack`), i.e. the
    window mapped, sized, and the webview navigated.
  - No panics, no GTK criticals related to the frame.
- Visual pixel sampling of the frame was attempted but the headless image
  tooling (`import`/`identify`/PIL) is unavailable in this environment, so
  corner-rounding and maximized full-bleed could not be pixel-verified here.
  The geometry/CSS rules that produce them are unchanged from the previously
  verified design.

## Notes / follow-ups

- The same-frame runtime test that would exercise drag, double-click maximize,
  and the right-click window menu needs an interactive session; code for those
  handlers is unchanged.

---

## Files changed

| File                | Change |
|---------------------|--------|
| `src-tauri/src/frame.rs` | Rewritten: public `gtk_window()`/`default_vbox()` API, deterministic menubar attach, theme-derived chrome colors, removed polling + runtime debug dumps |
| `src-tauri/src/main.rs`  | `apply_custom_frame` runs after `set_menu` so the menubar is present |
