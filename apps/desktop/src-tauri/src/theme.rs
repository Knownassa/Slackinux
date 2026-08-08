//! Theme handling for the native window and webview content.
//!
//! The custom GTK frame was removed in favor of the OS-native titlebar, so
//! theme changes only need to (a) tell Tauri which theme the native window
//! chrome should use and (b) flip the GTK application preference that WebKit
//! reads for `prefers-color-scheme`, so the Slack page follows the system or
//! user-chosen light/dark scheme like a real browser.

use log::{debug, warn};

use crate::settings::ThemePreference;

/// Applies a menu-selected theme immediately to both the native window chrome
/// and the webview content.
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
        let dark = theme_is_dark(preference);
        // WebKit reads this GTK application preference to decide the page's
        // prefers-color-scheme, matching a browser following the OS scheme.
        if let Some(settings) = gtk::Settings::default() {
            use gtk::prelude::*;
            settings.set_gtk_application_prefer_dark_theme(dark);
        }
        // The color seen around the webview's clipped corners before the page
        // paints; matches Slack's known surfaces.
        let (r, g, b) = if dark {
            (0.114, 0.110, 0.114)
        } else {
            (1.0, 1.0, 1.0)
        };
        use webkit2gtk::WebViewExt;
        webview.set_background_color(&gtk::gdk::RGBA::new(r, g, b, 1.0));
        debug!("theme: applied {preference} (dark={dark})");
    });
}

fn theme_is_dark(preference: ThemePreference) -> bool {
    match preference {
        ThemePreference::System => detect_dark_system(),
        ThemePreference::Light => false,
        ThemePreference::Dark => true,
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
