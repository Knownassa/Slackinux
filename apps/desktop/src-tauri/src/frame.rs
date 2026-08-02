//! Custom GTK client-side window frame.
//!
//! Linux-only. A `GtkHeaderBar` is installed as the window's real custom
//! titlebar. GTK and the compositor therefore retain their native resize hit
//! areas, shadows, tiling behavior, and rounded surface geometry while the app
//! keeps its compact menu and window controls. The whole window follows the
//! system light/dark scheme and, when in system theme mode, the Slack page's
//! own background.
//!
//! The frame uses only Tauri's public Linux APIs (`WebviewWindow::gtk_window`
//! and `WebviewWindow::default_vbox`) and runs *after* the app menu is set, so
//! the menubar is already present in the content box and can be reparented
//! deterministically without polling or internal widget-tree assumptions.

use std::sync::Arc;

use gtk::gdk;
use gtk::glib::{Cast, Propagation};
use gtk::prelude::*;
use log::{debug, info, warn};

use crate::settings::ThemePreference;

thread_local! {
    /// GTK objects are main-thread-bound; retain the one live provider here so
    /// menu changes update the existing chrome instead of stacking providers.
    static THEME_PROVIDER: std::cell::RefCell<Option<gtk::CssProvider>> =
        const { std::cell::RefCell::new(None) };
}

pub fn apply_custom_frame(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    theme_preference: Arc<std::sync::Mutex<ThemePreference>>,
) {
    // `with_webview` requires a `Send + 'static` closure, so GTK objects are
    // created inside it via Tauri's public `gtk_window()` / `default_vbox()`.
    let app = app.clone();
    let window_in_closure = window.clone();
    let _ = window.with_webview(move |pw| {
        let webview = pw.inner();
        let Ok(win) = window_in_closure.gtk_window() else {
            debug!("custom frame: no GTK window yet");
            return;
        };
        let Ok(content_vbox) = window_in_closure.default_vbox() else {
            debug!("custom frame: no content box yet");
            return;
        };
        let win_widget: &gtk::Window = win.upcast_ref::<gtk::Window>();

        let header = build_titlebar(win_widget, &app);

        // Use GTK's supported custom-titlebar path. Its client-side decoration
        // node owns the invisible resize border and compositor-facing window
        // shape; placing the header inside the content box loses both.
        win.set_decorated(true);
        win.set_resizable(true);
        win.set_titlebar(Some(&header));
        header.show_all();

        // The content box is the window's opaque card. Surface clipping and
        // bottom corners are provided by GTK's CSD decoration node.
        content_vbox.style_context().add_class("card");
        content_vbox.style_context().add_class("rounded");

        // The app menu (tauri/muda) is attached before the frame runs, so the
        // menubar is already a direct child of the content box. Move it into
        // the titlebar's left side.
        if let Some(menubar) = find_menubar(&content_vbox) {
            content_vbox.remove(&menubar);
            header.pack_start(&menubar);
            debug!("custom frame: menubar moved into titlebar");
        } else {
            debug!("custom frame: menubar not present in the content box");
        }

        // Follow the system light/dark scheme for the chrome and the opaque
        // webview background.
        let chrome = gtk::CssProvider::new();
        setup_system_theme(&webview, win_widget, &chrome, theme_preference.clone());

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

        apply_chrome_css(
            win_widget,
            &chrome,
            theme_is_dark(*theme_preference.lock().unwrap()),
            None,
        );

        // Make the chrome follow the page, i.e. the Slack theme the user picked
        // in the app. After each finished load, probe the page's effective
        // background and re-style the titlebar/card to match.
        let probe_wv = webview.clone();
        let probe_win = win.upcast_ref::<gtk::Window>().clone();
        let probe_chrome = chrome.clone();
        use webkit2gtk::WebViewExt;
        let probe_preference = theme_preference.clone();
        webview.connect_load_changed(move |wv, event| {
            use javascriptcore::ValueExt;
            use webkit2gtk::gio::Cancellable;
            use webkit2gtk::LoadEvent;
            if event != LoadEvent::Finished {
                return;
            }
            if *probe_preference.lock().unwrap() != ThemePreference::System {
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
                        let (dark, page_bg) = page_scheme(&s);
                        debug!(
                            "custom frame: page scheme -> {} ({})",
                            if dark { "dark" } else { "light" },
                            s.trim()
                        );
                        apply_theme(&wv2, &win2, &chrome2, dark, page_bg);
                    }
                },
            );
        });

        debug!("custom frame: rounded corners + titlebar applied");
    });
}

/// Finds the app menubar among the direct children of the content box.
fn find_menubar(content_vbox: &gtk::Box) -> Option<gtk::MenuBar> {
    content_vbox
        .children()
        .into_iter()
        .find(|child| child.is::<gtk::MenuBar>())
        .and_then(|child| child.downcast::<gtk::MenuBar>().ok())
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
fn apply_chrome_css(
    win: &gtk::Window,
    provider: &gtk::CssProvider,
    dark: bool,
    page_bg: Option<(f64, f64, f64)>,
) {
    let css = chrome_css(dark, page_bg);
    THEME_PROVIDER.with(|stored| {
        let mut stored = stored.borrow_mut();
        let active = stored.get_or_insert_with(|| {
            if let Some(screen) = gtk::prelude::WidgetExt::screen(win) {
                gtk::StyleContext::add_provider_for_screen(
                    &screen,
                    provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }
            provider.clone()
        });
        if let Err(e) = active.load_from_data(css.as_bytes()) {
            warn!("custom frame: css load failed: {e}");
        }
    });
}

fn chrome_css(dark: bool, page_bg: Option<(f64, f64, f64)>) -> String {
    let card_bg = page_bg
        .map(rgb_to_css)
        .unwrap_or_else(|| if dark { "#1d1c1d" } else { "#f6f7f8" }.to_string());
    let chrome_bg = if dark { "#242126" } else { "#ffffff" };
    let fg = if dark { "#f1eef1" } else { "#29252b" };
    let muted_fg = if dark { "#c9c3ca" } else { "#5f5661" };
    let border = if dark { "#3a363c" } else { "#ddd8de" };
    let hover = if dark { "#38323a" } else { "#f0ebf1" };
    let active = if dark { "#4a414c" } else { "#e5dce7" };
    format!(
        r#"
window.csd decoration {{
  border-radius: 12px;
}}
window.csd.maximized decoration,
window.csd.fullscreen decoration,
window.csd.tiled decoration {{
  border-radius: 0;
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

/// Converts a parsed CSS color to a `#rrggbb` string for the chrome card.
fn rgb_to_css((r, g, b): (f64, f64, f64)) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (r.round() as u32).min(255),
        (g.round() as u32).min(255),
        (b.round() as u32).min(255)
    )
}

// --- System light/dark theme ---

fn setup_system_theme(
    webview: &webkit2gtk::WebView,
    win: &gtk::Window,
    chrome: &gtk::CssProvider,
    preference: Arc<std::sync::Mutex<ThemePreference>>,
) {
    let dark = theme_is_dark(*preference.lock().unwrap());
    apply_theme(webview, win, chrome, dark, None);
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
                    if *preference.lock().unwrap() != ThemePreference::System {
                        return;
                    }
                    let dark = is_dark_from_settings(s);
                    debug!(
                        "custom frame: theme change -> {}",
                        if dark { "dark" } else { "light" }
                    );
                    apply_theme(&webview, &win, &chrome, dark, None);
                }
            });
        }
    }
}

fn theme_is_dark(preference: ThemePreference) -> bool {
    match preference {
        ThemePreference::System => detect_dark_system(),
        ThemePreference::Light => false,
        ThemePreference::Dark => true,
    }
}

/// Applies a menu-selected theme immediately to both Slack and native chrome.
pub fn set_theme(window: &tauri::WebviewWindow, preference: ThemePreference) {
    let tauri_theme = match preference {
        ThemePreference::System => None,
        ThemePreference::Light => Some(tauri::Theme::Light),
        ThemePreference::Dark => Some(tauri::Theme::Dark),
    };
    if let Err(err) = window.set_theme(tauri_theme) {
        warn!("theme: Tauri theme update failed: {err}");
    }
    let _ = window.with_webview(move |platform_webview| {
        let webview = platform_webview.inner();
        let Some(win) = webview
            .toplevel()
            .and_then(|widget| widget.downcast::<gtk::Window>().ok())
        else {
            warn!("theme: could not find the GTK window");
            return;
        };
        let provider = gtk::CssProvider::new();
        let dark = theme_is_dark(preference);
        apply_theme(&webview, &win, &provider, dark, None);
        info!("theme: applied {preference}");
    });
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
    page_bg: Option<(f64, f64, f64)>,
) {
    // Match the chrome — and, via the GTK application preference that WebKit
    // reads, the page's prefers-color-scheme — to the system scheme, so the
    // Slack UI follows the system's light/dark setting just like a browser.
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
    use webkit2gtk::WebViewExt;
    // The webview and surrounding chrome share the color around the clipped
    // corners. Prefer the probed page background (Slack's real canvas); fall
    // back to the known Slack surfaces only when the page has not been probed.
    let (r, g, b) = page_bg.unwrap_or(if dark {
        (0.114, 0.110, 0.114)
    } else {
        (1.0, 1.0, 1.0)
    });
    webview.set_background_color(&gdk::RGBA::new(r, g, b, 1.0));
    apply_chrome_css(win, chrome, dark, page_bg);
}

/// Decides light vs dark from the page probe and extracts the page's opaque
/// background so the chrome can match it. When the page has accidentally
/// produced low-contrast text (Slack's sign-in page can mix dark-mode backing
/// with light-mode text styles), prefer the opposite scheme so WebKit gets a
/// readable page.
fn page_scheme(probe: &str) -> (bool, Option<(f64, f64, f64)>) {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(probe) {
        let bg = json["bg"].as_str().and_then(parse_rgb);
        let fg = json["fg"].as_str().and_then(parse_rgb);
        let dark = if let (Some(background), Some(foreground)) = (bg, fg) {
            let bg_lum = luminance(background);
            let fg_lum = luminance(foreground);
            if (bg_lum - fg_lum).abs() < 72.0 {
                fg_lum >= 128.0
            } else {
                bg_lum < 128.0
            }
        } else if let Some(background) = bg {
            luminance(background) < 128.0
        } else if let Some(foreground) = fg {
            luminance(foreground) >= 128.0
        } else if let Some(dark) = json["dark"].as_bool() {
            dark
        } else {
            detect_dark_system()
        };
        // A transparent page background (e.g. the sign-in page probing too
        // early) must not repaint the card; keep the fallback color instead.
        let opaque_bg = bg.filter(|(r, g, b)| !(*r == 0.0 && *g == 0.0 && *b == 0.0));
        (dark, opaque_bg)
    } else {
        (detect_dark_system(), None)
    }
}

/// Kept for backwards compatibility with existing tests.
#[cfg(test)]
fn page_is_dark(probe: &str) -> bool {
    page_scheme(probe).0
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
