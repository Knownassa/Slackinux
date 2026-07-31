# Phase 6 Task 1 — Window Frame (Frameless, All Corners Rounded)

**Date:** 2026-07-31  
**Project:** Slackinux v0.1.0  
**Task:** Decide on and implement the window frame style. Final decision:
frameless transparent window with a native GTK titlebar (window controls) and
all four corners rounded — the "Slack desktop app" look.

---

## Acceptance criteria

| #  | Criterion                                                   | Status |
|----|-------------------------------------------------------------|--------|
| 1  | `cargo fmt` passes                                          | PASS   |
| 2  | `cargo clippy` with warnings denied                         | PASS   |
| 3  | `cargo test` passes — 17 tests                              | PASS   |
| 4  | Window interior is fully opaque (no see-through content)    | PASS   |
| 5  | All four corners are rounded                                | PASS   |
| 6  | Custom titlebar with minimize / maximize / close buttons    | PASS   |
| 7  | Drag-to-move, double-click maximize, right-click menu       | PASS   |
| 8  | Corner square-ness restored when maximized                  | PASS   |
| 9  | App menu (Account/View/Window/Help) shown in the titlebar left | PASS |
| 10 | Chrome + web content follow the system light/dark scheme      | PASS   |
| 11 | Runtime smoke test: window shows, no panics, Slack nav works | PASS  |
| 12 | Release bundles build (AppImage + deb)                      | PASS   |

---

## History (why we ended here)

1. **Transparent frameless attempt** (`.decorations(false).transparent(true)`)
   with a hand-rolled rounded-corner clip and a manual drag strip. Failed:
   tao fills the whole window transparent with `Operator::Source` in its own
   `draw` handler, and the frameless window had no visible window controls —
   the UI looked transparent and unusable.
2. **Custom CSD headerbar** (`frame.rs`, `gtk::HeaderBar` with explicit
   buttons, `set_titlebar`). Worked, but the user preferred the default look.
3. **Default decorations** (`.decorations(true)`). Reverted to stock GNOME
   frame (square corners).
4. **Frameless + rounded + buttons (final).** Frameless transparent window
   with an in-window GTK `HeaderBar` that hosts its own window controls, and a
   rounded clip applied to the web content so all four corners are rounded.

## Implementation notes

- `main.rs`: window built with `.decorations(false).transparent(true)`; on
  Linux, `frame::apply_custom_frame(app.handle(), &window)` runs after the
  window is built. The **Window** menu (Minimize `Ctrl+M`, Maximize/Restore),
  tray menu, and close-to-tray behavior remain.
- `frame.rs` (recreated): `with_webview` grabs the platform `WebKitWebView`
  and:
  - Sets an opaque background color on the webview so page content never shows
    the desktop through the transparent window.
  - Keeps tauri's content box (`default_vbox`, which holds the webview and the
    app menubar) as the content area; the GTK `MenuBar` that muda/tauri
    attaches to it after setup is detected (immediately and via a short GLib
    timeout) and reparented into the titlebar's **left** side.
  - Packs everything into a vertical box below a `gtk::HeaderBar`.
  - The headerbar carries the app menubar on the left and minimize / maximize
    / close buttons on the right (`window-minimize-symbolic`,
    `window-maximize-symbolic`, `window-close-symbolic`); maximize toggles via
    `is_maximized()`.
  - Titlebar interactions: drag (`begin_move_drag`), double-click toggles
    maximize, right-click pops a window menu (Minimize / Maximize / Close to
    Tray / Quit). Clicks on the menu bar and buttons are consumed by those
    widgets before reaching the drag handler.
  - A `connect_draw` clip on the webview rounds its bottom corners
    (`round_bottom_path`); the CSS provider rounds the headerbar's top corners
    (`headerbar.rounded { border-radius: 12px 12px 0 0; }`).
  - A `window-state-event` handler drops the rounded classes when maximized so
    the window fills the screen edge-to-edge.
  - **System theme**: reads GNOME's `org.gnome.desktop.interface`
    `color-scheme` / `gtk-theme` (via `gio::Settings`, with a `gsettings`
    fallback) and applies `gtk-application-prefer-dark-theme` to the GTK
    chrome plus the matching opaque background color on the webview; a
    `connect_changed` listener live-follows theme switches.
- `Cargo.toml`: Linux target dependency `gtk = "0.18"` restored.
- `renderer/webkit.rs`: HTTP(S) popups (auth / SSO sign-in) now open **in-app**
  in a separate `WebviewWindow` so cookies are shared with the main webview
  and browser-based sign-in completes inside the app (mailto/tel still open
  externally). The **Account → Sign in with Browser** menu item now navigates
  the main webview to `https://app.slack.com/signin` in-app instead of opening
  an external browser. WebKit settings: `HardwareAccelerationPolicy::Always`,
  `enable-smooth-scrolling` for a smoother, faster UI.

## Performance notes

- The transparent window is required on Wayland for real all-corner rounding,
  but it can push WebKitGTK toward software rendering. Mitigations applied:
  opaque webview background (`webkit_web_view_set_background_color`, alpha
  1.0), `HardwareAccelerationPolicy::Always`, and smooth scrolling.
- If the UI is still sluggish, the fallback is an opaque CSD window
  (`decorations(true)` + `set_titlebar` with the same menubar/buttons) which
  is faster but only rounds the top corners (standard GNOME look).

## Verification

- `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`
  (17 passed), `cargo build` — all clean.
- Wayland smoke test: logs show the frame applied
  (`custom frame: webview parent: GtkBox`, `window child: GtkBox`,
  `rounded corners + titlebar applied`), no `gtk_box_pack` criticals, Slack
  navigation proceeds (`app.slack.com/client -> AllowInternal`), no panics.
- Bundle builds: `AppImage` requires `NO_STRIP=1 APPIMAGE_EXTRACT_AND_RUN=1`
  on this system (linuxdeploy's 2024 bundled `strip` cannot parse `.relr.dyn`
  sections and the AppImage runner needs FUSE-less extraction).
- Known benign startup noise: `Gtk-CRITICAL gtk_widget_get_scale_factor`
  (from glycin's image loader, unrelated to the frame) and `libenchant`
  warnings about missing optional spell-check backends.

---

## Files changed

| File                   | Change |
|------------------------|--------|
| `src/frame.rs`         | Recreated: rounded frameless frame, titlebar + window controls, bottom corner clip, maximize handling |
| `src/main.rs`          | Added `mod frame;` (Linux), `.decorations(false).transparent(true)`, `frame::apply_custom_frame` call, in-app sign-in menu item |
| `src/renderer/webkit.rs` | Auth popups open in-app (shared cookies); mailto/tel external |
| `src-tauri/Cargo.toml` | Restored Linux `gtk = "0.18"` dependency |
| `README.md`            | Updated frame/usage sections for the frameless rounded design |
