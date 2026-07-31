//! Custom window frame — rounded corners on a frameless transparent window.
//!
//! Linux-only. The window is frameless and transparent; the GTK app menubar
//! plus a thin titlebar with window controls sit at the top and the web
//! content fills the rest, all inside a rounded silhouette so all four corners
//! are rounded. The webview is forced opaque so the interior is never
//! see-through — only the corner cutouts stay transparent. The whole window
//! (chrome and Slack content) follows the system light/dark scheme.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtk::gdk;
use gtk::glib::{Cast, Propagation};
use gtk::prelude::*;
use log::{debug, info, warn};

pub fn apply_custom_frame(app: &tauri::AppHandle, window: &tauri::WebviewWindow) {
    let app = app.clone();
    let _ = window.with_webview(move |pw| {
        let webview = pw.inner();
        let Some(toplevel) = webview.toplevel() else {
            debug!("custom frame: no toplevel window yet");
            return;
        };
        let Some(win) = toplevel.downcast::<gtk::Window>().ok() else {
            debug!("custom frame: toplevel is not a GtkWindow");
            return;
        };

        // The webview — and, once the app menu is attached, the menubar — live
        // in the window's content box (tauri's default_vbox). Keep that box as
        // the window's direct child so tauri's undecorated-resize handler still
        // finds the GtkWindow from the webview's parent chain.
        let Some(content_vbox) = win.child().and_then(|c| c.downcast::<gtk::Box>().ok()) else {
            debug!("custom frame: no content box found");
            return;
        };

        let header = build_titlebar(&win, &app);

        // tao installs its own Wayland CSD titlebar via set_titlebar; on an
        // undecorated window GTK collapses that area to 1px. Replace it with an
        // empty, background-less box so no stray sliver renders across the top.
        let empty = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        empty.style_context().add_class("transparent");
        win.set_titlebar(Some(&empty));

        // Pack the titlebar into the window's content box, above the webview.
        // The webview stays a direct child of that box, so tauri's
        // undecorated-resize handler still finds the GtkWindow from the
        // webview's parent chain (window → box → webview).
        content_vbox.pack_start(&header, false, false, 0);
        content_vbox.reorder_child(&header, 0);
        header.show_all();

        // The content box is the window's opaque "card". The webview's draw
        // clip below provides the rounded bottom corners without reserving
        // visible space beneath the page.
        content_vbox.style_context().add_class("card");
        content_vbox.style_context().add_class("rounded");

        // Manual drag-to-move: GTK3 hides a set_titlebar titlebar on an
        // undecorated window, so the header lives in the content and we move
        // the window ourselves.
        setup_drag(&header, &win);

        // The app menubar (tauri/muda) is attached to the content box; move it
        // into the titlebar's left side. Handle the case where it was attached
        // before the frame ran, and watch for it arriving after setup.
        let menubar_added = Arc::new(AtomicBool::new(false));
        for child in content_vbox.children() {
            if child.is::<gtk::MenuBar>() {
                content_vbox.remove(&child);
                header.pack_start(&child);
                menubar_added.store(true, Ordering::Relaxed);
                break;
            }
        }
        let flag = menubar_added.clone();
        let watch_vbox = content_vbox.clone();
        let watch_header = header.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            if flag.load(Ordering::Relaxed) {
                return gtk::glib::ControlFlow::Break;
            }
            for child in watch_vbox.children() {
                if child.is::<gtk::MenuBar>() {
                    watch_vbox.remove(&child);
                    watch_header.pack_start(&child);
                    watch_header.show_all();
                    flag.store(true, Ordering::Relaxed);
                    debug!("custom frame: menubar moved into titlebar");
                    break;
                }
            }
            gtk::glib::ControlFlow::Continue
        });

        // Follow the system light/dark scheme for the chrome and the opaque
        // webview background.
        let chrome = gtk::CssProvider::new();
        setup_system_theme(&webview, &win, &chrome);

        // Keep the chrome's corners square when maximized so the window fills
        // the screen edge to edge.
        let titlebar_state = header.clone();
        let card_state = content_vbox.clone();
        win.connect_window_state_event(move |_, event| {
            let maximized = event
                .new_window_state()
                .contains(gdk::WindowState::MAXIMIZED);
            if maximized {
                titlebar_state.style_context().remove_class("rounded");
                card_state.style_context().remove_class("rounded");
            } else {
                titlebar_state.style_context().add_class("rounded");
                card_state.style_context().add_class("rounded");
            }
            Propagation::Proceed
        });

        apply_chrome_css(&win, &chrome, detect_dark_system());

        // Make the chrome follow the page, i.e. the Slack theme the user picked
        // in the app. After each finished load, probe the page's effective
        // background and re-style the titlebar/card to match.
        let probe_wv = webview.clone();
        let probe_win = win.clone();
        let probe_chrome = chrome.clone();
        use webkit2gtk::WebViewExt;
        webview.connect_load_changed(move |wv, event| {
            use javascriptcore::ValueExt;
            use webkit2gtk::gio::Cancellable;
            use webkit2gtk::LoadEvent;
            if event != LoadEvent::Finished {
                return;
            }
            let wv2 = probe_wv.clone();
            let win2 = probe_win.clone();
            let chrome2 = probe_chrome.clone();
            wv.evaluate_javascript(
                "(function(){var el=document.elementFromPoint(window.innerWidth/2,120),\
                 bg='rgba(0,0,0,0)',fg=el?getComputedStyle(el).color:'';\
                 while(el){var c=getComputedStyle(el).backgroundColor;\
                 if(c&&c!=='transparent'&&c!=='rgba(0, 0, 0, 0)'){bg=c;break;}\
                 el=el.parentElement;}\
                 return JSON.stringify({bg:bg,\
                 fg:fg,\
                 dark:matchMedia('(prefers-color-scheme: dark)').matches});})()",
                None,
                None,
                None::<&Cancellable>,
                move |res| {
                    if let Ok(v) = res {
                        let s = v.to_str();
                        let dark = page_is_dark(&s);
                        debug!(
                            "custom frame: page scheme -> {} ({})",
                            if dark { "dark" } else { "light" },
                            s.trim()
                        );
                        apply_theme(&wv2, &win2, &chrome2, dark);
                    }
                },
            );
        });

        debug!("custom frame: rounded corners + titlebar applied");
        if log::log_enabled!(log::Level::Debug) {
            let mut lines = Vec::new();
            dump_widget_tree(win.upcast_ref(), 0, &mut lines);
            debug!("custom frame: widget tree:\n{}", lines.join("\n"));
        }

        // Second dump after the menubar attaches, to confirm it landed in the
        // titlebar and the frame settled.
        let tree_win = win.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(3000), move || {
            if log::log_enabled!(log::Level::Debug) {
                let mut lines = Vec::new();
                dump_widget_tree(tree_win.upcast_ref(), 0, &mut lines);
                debug!("custom frame: settled widget tree:\n{}", lines.join("\n"));
                let w = tree_win.downcast_ref::<gtk::Window>().unwrap();
                debug!(
                    "custom frame: decorated={} mapped={} size={}x{}",
                    w.is_decorated(),
                    w.is_mapped(),
                    w.allocation().width(),
                    w.allocation().height()
                );
                for child in w.children() {
                    if child.is::<gtk::EventBox>() {
                        let a = child.allocation();
                        debug!(
                            "custom frame: tao eventbox alloc={}x{} at {},{} visible={}",
                            a.width(),
                            a.height(),
                            a.x(),
                            a.y(),
                            child.is_visible()
                        );
                    }
                }
                dump_frame_metrics(w);
            }
            gtk::glib::ControlFlow::Break
        });
    });
}

fn dump_widget_tree(widget: &gtk::Widget, depth: usize, out: &mut Vec<String>) {
    let name = widget.type_().name().to_string();
    let mut line = format!("{}{}", "  ".repeat(depth), name);
    if widget.is_visible() {
        line.push_str(" [v]");
    } else {
        line.push_str(" [h]");
    }
    let classes = widget
        .style_context()
        .list_classes()
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(",");
    if !classes.is_empty() {
        line.push_str(&format!(" <{classes}>"));
    }
    out.push(line);
    if let Some(container) = widget.downcast_ref::<gtk::Container>() {
        for child in container.children() {
            dump_widget_tree(&child, depth + 1, out);
        }
    }
}

/// A compact titlebar: app menubar on the left, title in the center, and
/// familiar window controls on the right.
fn build_titlebar(win: &gtk::Window, app: &tauri::AppHandle) -> gtk::HeaderBar {
    let header = gtk::HeaderBar::new();
    header.set_show_close_button(false);
    header.set_has_subtitle(false);
    header.set_decoration_layout(Some(":"));
    header.style_context().add_class("rounded");
    header.style_context().add_class("titlebar");

    let title = gtk::Label::new(Some("Slackinux"));
    title.style_context().add_class("app-title");
    header.set_custom_title(Some(&title));

    let minimize =
        gtk::Button::from_icon_name(Some("window-minimize-symbolic"), gtk::IconSize::Button);
    let maximize =
        gtk::Button::from_icon_name(Some("window-maximize-symbolic"), gtk::IconSize::Button);
    let close = gtk::Button::from_icon_name(Some("window-close-symbolic"), gtk::IconSize::Button);

    for button in [&minimize, &maximize, &close] {
        button.set_relief(gtk::ReliefStyle::None);
        button.set_focus_on_click(false);
        button.style_context().add_class("window-control");
    }
    minimize.style_context().add_class("minimize");
    maximize.style_context().add_class("maximize");
    close.style_context().add_class("close");
    minimize.set_tooltip_text(Some("Minimize"));
    maximize.set_tooltip_text(Some("Maximize"));
    close.set_tooltip_text(Some("Close to tray"));

    let win_min = win.clone();
    minimize.connect_clicked(move |_| {
        info!("custom frame: minimize");
        win_min.iconify();
    });

    let win_max = win.clone();
    maximize.connect_clicked(move |_| {
        if win_max.is_maximized() {
            win_max.unmaximize();
        } else {
            win_max.maximize();
        }
    });

    // Keep the middle control recognizable after maximizing the window.
    let maximize_state = maximize.clone();
    win.connect_window_state_event(move |_, event| {
        let maximized = event
            .new_window_state()
            .contains(gdk::WindowState::MAXIMIZED);
        let icon = if maximized {
            "window-restore-symbolic"
        } else {
            "window-maximize-symbolic"
        };
        let image = gtk::Image::from_icon_name(Some(icon), gtk::IconSize::Button);
        maximize_state.set_image(Some(&image));
        maximize_state.set_tooltip_text(Some(if maximized { "Restore" } else { "Maximize" }));
        Propagation::Proceed
    });

    let win_close = win.clone();
    close.connect_clicked(move |_| {
        win_close.hide();
    });

    header.pack_end(&close);
    header.pack_end(&maximize);
    header.pack_end(&minimize);

    // Right-click the titlebar for a window menu.
    let win_menu = win.clone();
    let app_menu = app.clone();
    header.connect_button_press_event(move |hb, event| {
        if event.button() == 3 {
            let maximized = hb
                .toplevel()
                .and_then(|t| t.downcast::<gtk::Window>().ok())
                .map(|w| w.is_maximized())
                .unwrap_or(false);
            let menu = build_window_menu(&win_menu, &app_menu, maximized);
            menu.popup_at_pointer(Some(event));
            return Propagation::Stop;
        }
        Propagation::Proceed
    });

    header
}

/// Left-drag on the titlebar's empty area moves the window. Clicks that land
/// on interactive children (window buttons, menubar items) are left alone.
///
/// The headerbar is a no-window widget, so a press on its empty area is
/// delivered either to the headerbar itself (if GTK gave it a window) or to
/// the toplevel window. Install the handler on both; exactly one of them
/// receives each press.
fn setup_drag(header: &gtk::HeaderBar, win: &gtk::Window) {
    let install = |widget: &gtk::Widget, header_ref: gtk::HeaderBar, win_drag: gtk::Window| {
        widget.connect_button_press_event(move |w, event| {
            if event.button() != 1 {
                return Propagation::Proceed;
            }
            let hb = header_ref.allocation();
            let (x, y) = event.position();
            if !(hb.x() as f64..(hb.x() + hb.width()) as f64).contains(&x)
                || !(hb.y() as f64..(hb.y() + hb.height()) as f64).contains(&y)
            {
                return Propagation::Proceed;
            }
            let mut ev = (*event).clone();
            let Some(widget) = gtk::event_widget(&mut ev) else {
                return Propagation::Proceed;
            };
            // Walk up from the press target; if it reaches a clickable
            // control, let the click go to it instead of starting a drag.
            let mut target = Some(widget);
            while let Some(t) = target {
                if t.is::<gtk::Button>() || t.is::<gtk::MenuBar>() || t.is::<gtk::MenuItem>() {
                    return Propagation::Proceed;
                }
                let is_window = t == *w.upcast_ref::<gtk::Widget>();
                target = if is_window { None } else { t.parent() };
            }
            if event.event_type() == gdk::EventType::DoubleButtonPress {
                if win_drag.is_maximized() {
                    win_drag.unmaximize();
                } else {
                    win_drag.maximize();
                }
                return Propagation::Stop;
            }
            let (root_x, root_y) = event.root();
            win_drag.begin_move_drag(1, root_x as i32, root_y as i32, event.time());
            Propagation::Stop
        });
    };
    install(header.upcast_ref(), header.clone(), win.clone());
    install(win.upcast_ref(), header.clone(), win.clone());
}

fn build_window_menu(win: &gtk::Window, app: &tauri::AppHandle, maximized: bool) -> gtk::Menu {
    let menu = gtk::Menu::new();

    let minimize = gtk::MenuItem::with_label("Minimize");
    let win_min = win.clone();
    minimize.connect_activate(move |_| {
        win_min.iconify();
    });
    menu.append(&minimize);

    let maximize = gtk::MenuItem::with_label(if maximized { "Restore" } else { "Maximize" });
    let win_max = win.clone();
    maximize.connect_activate(move |_| {
        if win_max.is_maximized() {
            win_max.unmaximize();
        } else {
            win_max.maximize();
        }
    });
    menu.append(&maximize);

    menu.append(&gtk::SeparatorMenuItem::new());

    let close = gtk::MenuItem::with_label("Close to Tray");
    let win_close = win.clone();
    close.connect_activate(move |_| {
        win_close.hide();
    });
    menu.append(&close);

    let quit = gtk::MenuItem::with_label("Quit");
    let app_quit = app.clone();
    quit.connect_activate(move |_| {
        app_quit.exit(0);
    });
    menu.append(&quit);

    menu.show_all();
    menu
}

/// Thin titlebar + rounded corners via CSS. The chrome (titlebar and the
/// opaque card under the webview) is re-styled whenever the scheme flips.
fn apply_chrome_css(win: &gtk::Window, provider: &gtk::CssProvider, dark: bool) {
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    let css = chrome_css(dark);
    if let Err(e) = provider.load_from_data(css.as_bytes()) {
        warn!("custom frame: css load failed: {e}");
    }
    if !INSTALLED.swap(true, Ordering::Relaxed) {
        if let Some(screen) = gtk::prelude::WidgetExt::screen(win) {
            gtk::StyleContext::add_provider_for_screen(
                &screen,
                provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }
}

fn chrome_css(dark: bool) -> String {
    let card_bg = if dark { "#1d1c1d" } else { "#f6f7f8" };
    let chrome_bg = if dark { "#242126" } else { "#ffffff" };
    let fg = if dark { "#f1eef1" } else { "#29252b" };
    let muted_fg = if dark { "#c9c3ca" } else { "#5f5661" };
    let border = if dark { "#3a363c" } else { "#ddd8de" };
    let hover = if dark { "#38323a" } else { "#f0ebf1" };
    let active = if dark { "#4a414c" } else { "#e5dce7" };
    format!(
        r#"
window, window.csd {{
  background-color: transparent;
  background-image: none;
  box-shadow: none;
  border: none;
}}
box.card {{
  background-color: {card_bg};
}}
box.card.rounded {{
  border-radius: 12px;
}}
headerbar.titlebar {{
  min-height: 32px;
  font-size: 12px;
  padding-top: 0;
  padding-bottom: 0;
  padding-left: 4px;
  padding-right: 4px;
  margin: 0;
  background-image: none;
  background-color: {chrome_bg};
  color: {fg};
  border-image: none;
  box-shadow: none;
  border-bottom: 1px solid {border};
}}
headerbar.titlebar label.app-title {{
  font-size: 12px;
  font-weight: 600;
  padding: 0;
  margin: 0;
  min-height: 0;
  border: none;
  color: {fg};
}}
headerbar.titlebar menubar {{
  font-size: 12px;
  padding: 0;
  margin: 0;
  background: transparent;
  background-image: none;
  color: {fg};
}}
headerbar.titlebar menubar > menuitem {{
  min-height: 24px;
  font-size: 12px;
  padding: 0 7px;
  margin: 3px 1px;
  border-radius: 5px;
  background: transparent;
  background-image: none;
  color: {muted_fg};
}}
headerbar.titlebar menubar > menuitem:hover,
headerbar.titlebar menubar > menuitem:active {{
  background-color: {hover};
  color: {fg};
}}
headerbar.titlebar button.window-control {{
  min-height: 28px;
  min-width: 36px;
  padding: 0;
  margin: 2px 0;
  border: none;
  border-radius: 5px;
  background: transparent;
  background-image: none;
  box-shadow: none;
  color: {muted_fg};
}}
headerbar.titlebar button.window-control:hover {{
  background-color: {hover};
  color: {fg};
}}
headerbar.titlebar button.window-control:active {{
  background-color: {active};
}}
headerbar.titlebar button.window-control.close:hover {{
  background-color: #d92d3a;
  color: white;
}}
window:backdrop headerbar.titlebar label.app-title,
window:backdrop headerbar.titlebar menubar,
window:backdrop headerbar.titlebar button.window-control {{
  opacity: 0.72;
}}
headerbar.titlebar.rounded {{
  border-radius: 12px 12px 0 0;
}}
"#
    )
}

// --- System light/dark theme ---

fn setup_system_theme(webview: &webkit2gtk::WebView, win: &gtk::Window, chrome: &gtk::CssProvider) {
    let dark = detect_dark_system();
    apply_theme(webview, win, chrome, dark);
    debug!(
        "custom frame: initial theme = {}",
        if dark { "dark" } else { "light" }
    );

    // Live-follow GNOME's color-scheme / gtk-theme settings.
    let webview = webview.clone();
    let win = win.clone();
    let chrome = chrome.clone();
    if let Some(source) = gtk::gio::SettingsSchemaSource::default() {
        if source.lookup("org.gnome.desktop.interface", true).is_some() {
            let settings = gtk::gio::Settings::new("org.gnome.desktop.interface");
            settings.connect_changed(None, move |s, key| {
                if key == "color-scheme" || key == "gtk-theme" {
                    let dark = is_dark_from_settings(s);
                    debug!(
                        "custom frame: theme change -> {}",
                        if dark { "dark" } else { "light" }
                    );
                    apply_theme(&webview, &win, &chrome, dark);
                }
            });
        }
    }
}

fn detect_dark_system() -> bool {
    if let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "color-scheme"])
        .output()
    {
        let v = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
        if v.contains("dark") {
            return true;
        }
        if v.contains("light") {
            return false;
        }
    }
    if let Ok(out) = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
        .output()
    {
        let v = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
        if v.contains("dark") {
            return true;
        }
        if v.contains("light") {
            return false;
        }
    }
    if let Ok(theme) = std::env::var("GTK_THEME") {
        let t = theme.to_lowercase();
        if t.contains("dark") && !t.contains("light") {
            return true;
        }
    }
    false
}

fn is_dark_from_settings(s: &gtk::gio::Settings) -> bool {
    let scheme = s.string("color-scheme").to_lowercase();
    if scheme.contains("dark") {
        return true;
    }
    if scheme.contains("light") {
        return false;
    }
    s.string("gtk-theme").to_lowercase().contains("dark")
}

fn apply_theme(
    webview: &webkit2gtk::WebView,
    win: &gtk::Window,
    chrome: &gtk::CssProvider,
    dark: bool,
) {
    // Match the chrome — and, via the GTK application preference that WebKit
    // reads, the page's prefers-color-scheme — to the system scheme, so the
    // Slack UI follows the system's light/dark setting just like a browser.
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
    use webkit2gtk::WebViewExt;
    // Slack's app background (#1d1c1d dark, white light). The webview and
    // surrounding chrome share the color around the clipped corners.
    let (r, g, b) = if dark {
        (0.114, 0.110, 0.114)
    } else {
        (1.0, 1.0, 1.0)
    };
    webview.set_background_color(&gdk::RGBA::new(r, g, b, 1.0));
    apply_chrome_css(win, chrome, dark);
}

/// Decide light vs dark from the page probe. When the page has accidentally
/// produced low-contrast text (Slack's sign-in page can mix dark-mode backing
/// with light-mode text styles), prefer the opposite scheme so WebKit gets a
/// readable page. Otherwise the effective page background wins.
fn page_is_dark(probe: &str) -> bool {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(probe) {
        let bg = json["bg"].as_str().and_then(parse_rgb);
        let fg = json["fg"].as_str().and_then(parse_rgb);
        if let (Some(background), Some(foreground)) = (bg, fg) {
            let bg_lum = luminance(background);
            let fg_lum = luminance(foreground);
            if (bg_lum - fg_lum).abs() < 72.0 {
                return fg_lum >= 128.0;
            }
            return bg_lum < 128.0;
        }
        if let Some(background) = bg {
            return luminance(background) < 128.0;
        }
        // A transparent page with dark text needs a light opaque backing;
        // likewise, light text needs a dark one.
        if let Some(foreground) = fg {
            return luminance(foreground) >= 128.0;
        }
        if let Some(dark) = json["dark"].as_bool() {
            return dark;
        }
    }
    detect_dark_system()
}

fn luminance((r, g, b): (f64, f64, f64)) -> f64 {
    0.299 * r + 0.587 * g + 0.114 * b
}

fn parse_rgb(css: &str) -> Option<(f64, f64, f64)> {
    let inner = css
        .strip_prefix("rgb(")
        .or_else(|| css.strip_prefix("rgba("))?
        .strip_suffix(')')?;
    let mut parts = inner.split(',');
    let r = parts.next()?.trim().parse::<f64>().ok()?;
    let g = parts.next()?.trim().parse::<f64>().ok()?;
    let b = parts.next()?.trim().parse::<f64>().ok()?;
    Some((r, g, b))
}

fn dump_frame_metrics(w: &gtk::Window) {
    use gtk::prelude::WidgetExt;
    let win_alloc = w.allocation();
    debug!(
        "frame: window alloc={}x{} visual_depth={} composited={}",
        win_alloc.width(),
        win_alloc.height(),
        w.visual().map(|v| v.depth()).unwrap_or(0),
        gtk::prelude::WidgetExt::screen(w)
            .map(|s| s.is_composited())
            .unwrap_or(false)
    );
    for child in w.children() {
        if child.is::<gtk::Box>() {
            let children = child.downcast_ref::<gtk::Container>().unwrap().children();
            for gc in children {
                if gc.is::<gtk::HeaderBar>() {
                    let (min, nat) = gc.preferred_height();
                    debug!(
                        "frame: headerbar preferred_min={} natural={} alloc={}x{} has_window={}",
                        min,
                        nat,
                        gc.allocation().width(),
                        gc.allocation().height(),
                        gc.has_window()
                    );
                    let header_children = gc.downcast_ref::<gtk::Container>().unwrap().children();
                    for bc in header_children {
                        let (_, bnat) = bc.preferred_height();
                        let (hfw_min, hfw_nat) =
                            bc.preferred_height_for_width(bc.allocation().width());
                        let ba = bc.allocation();
                        debug!(
                            "frame:   child type={} class={} natural_height={} hfw={}/{} alloc={}x{} at {},{}",
                            bc.type_().name(),
                            bc.style_context()
                                .list_classes()
                                .iter()
                                .map(|c| c.to_string())
                                .collect::<Vec<_>>()
                                .join(","),
                            bnat,
                            hfw_min,
                            hfw_nat,
                            ba.width(),
                            ba.height(),
                            ba.x(),
                            ba.y()
                        );
                    }
                    let internal: std::cell::RefCell<Vec<String>> =
                        std::cell::RefCell::new(Vec::new());
                    gc.downcast_ref::<gtk::Container>().unwrap().forall(|w| {
                        let (min, nat) = w.preferred_height();
                        let a = w.allocation();
                        internal.borrow_mut().push(format!(
                            "{} class=[{}] min={}/nat={} alloc={}x{}",
                            w.type_().name(),
                            w.style_context()
                                .list_classes()
                                .iter()
                                .map(|c| c.to_string())
                                .collect::<Vec<_>>()
                                .join(","),
                            min,
                            nat,
                            a.width(),
                            a.height()
                        ));
                        if let Some(box_) = w.downcast_ref::<gtk::Box>() {
                            let sub: std::cell::RefCell<Vec<String>> =
                                std::cell::RefCell::new(Vec::new());
                            box_.forall(|c| {
                                let (m, n) = c.preferred_height();
                                sub.borrow_mut().push(format!(
                                    "{} class=[{}] min={}/nat={} text={}",
                                    c.type_().name(),
                                    c.style_context()
                                        .list_classes()
                                        .iter()
                                        .map(|s| s.to_string())
                                        .collect::<Vec<_>>()
                                        .join(","),
                                    m,
                                    n,
                                    c.downcast_ref::<gtk::Label>()
                                        .map(|l| l.text().to_string())
                                        .unwrap_or_default()
                                ));
                            });
                            if !sub.borrow().is_empty() {
                                internal
                                    .borrow_mut()
                                    .push(format!("     box kids: {}", sub.borrow().join(" | ")));
                            }
                        }
                    });
                    debug!("frame:   internal: {}", internal.borrow().join(" | "));
                }
                if gc.is::<webkit2gtk::WebView>() {
                    let a = gc.allocation();
                    debug!(
                        "frame: webview alloc={}x{} at {},{}",
                        a.width(),
                        a.height(),
                        a.x(),
                        a.y()
                    );
                }
            }
        }
    }
    let theme = gtk::IconTheme::default();
    for icon in [
        "window-minimize-symbolic",
        "window-maximize-symbolic",
        "window-close-symbolic",
    ] {
        debug!(
            "frame: icon {icon} available={}",
            theme.as_ref().map(|t| t.has_icon(icon)).unwrap_or(false)
        );
    }

    if let Some(hb) = w.children().iter().find_map(|c| {
        c.downcast_ref::<gtk::Container>()
            .and_then(|bx| bx.children().into_iter().find(|g| g.is::<gtk::HeaderBar>()))
    }) {
        let fresh = gtk::HeaderBar::new();
        fresh.set_show_close_button(false);
        fresh.set_decoration_layout(Some(":"));
        fresh.style_context().add_class("titlebar");
        fresh.style_context().add_class("rounded");
        let (fm, fn_) = fresh.preferred_height();
        let (m, n) = hb.preferred_height();
        debug!(
            "frame: fresh headerbar min={}/nat={} vs real min={}/nat={}",
            fm, fn_, m, n
        );
        use gtk::cairo::{Context, Format, ImageSurface};
        const S: i32 = 28;
        let mut surface = ImageSurface::create(Format::ARgb32, S, S).unwrap();
        let cr = Context::new(&surface).unwrap();
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
        let _ = cr.paint();
        let _ = cr.save();
        cr.scale(1.0, 1.0);
        hb.draw(&cr);
        drop(cr);
        surface.flush();
        let data = surface.data().unwrap();
        let px = |x: i32, y: i32| -> u8 { data[(y * S + x) as usize * 4 + 3] };
        debug!(
            "frame: headerbar corner alpha (1,1)={} (12,1)={} (1,12)={}",
            px(1, 1),
            px(12, 1),
            px(1, 12)
        );
    }
}

#[cfg(test)]
mod theme_tests {
    use super::page_is_dark;

    #[test]
    fn keeps_readable_dark_page_dark() {
        let probe = r#"{"bg":"rgb(29, 28, 29)","fg":"rgb(255, 255, 255)","dark":true}"#;
        assert!(page_is_dark(probe));
    }

    #[test]
    fn corrects_dark_text_on_dark_sign_in_page() {
        let probe = r#"{"bg":"rgb(29, 28, 29)","fg":"rgb(20, 20, 20)","dark":true}"#;
        assert!(!page_is_dark(probe));
    }

    #[test]
    fn gives_transparent_dark_text_a_light_backing() {
        let probe = r#"{"bg":"rgba(0, 0, 0, 0)","fg":"rgb(20, 20, 20)","dark":true}"#;
        assert!(!page_is_dark(probe));
    }
}
