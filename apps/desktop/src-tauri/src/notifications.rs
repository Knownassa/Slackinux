use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use log::{info, warn};
use notify_rust::Hint;
use tauri::Manager;
use webkit2gtk::{NotificationExt, WebView, WebViewExt};

pub struct NotificationManager {
    dnd: AtomicBool,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            dnd: AtomicBool::new(false),
        }
    }

    pub fn set_dnd(&self, enabled: bool) {
        self.dnd.store(enabled, Ordering::Relaxed);
        info!(
            "Do Not Disturb: {}",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    pub fn is_dnd(&self) -> bool {
        self.dnd.load(Ordering::Relaxed)
    }
}

fn hash_tag(tag: &str) -> u32 {
    let mut hash: u32 = 5381;
    for b in tag.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(b as u32);
    }
    hash
}

pub fn setup_notification_handler(
    webview: &WebView,
    mgr: Arc<NotificationManager>,
    app_handle: tauri::AppHandle,
) {
    webview.connect_show_notification(move |_webview, wkn| {
        if mgr.is_dnd() {
            info!(
                "notification suppressed (DND): {}",
                wkn.title().unwrap_or_default()
            );
            wkn.close();
            return true;
        }

        let title: String = wkn.title().unwrap_or_default().into();
        let body: String = wkn.body().unwrap_or_default().into();
        let tag: Option<String> = wkn.tag().map(|t| t.into());

        info!("notification: {title}");

        let mut n = notify_rust::Notification::new();
        n.summary(&title)
            .body(&body)
            .appname("Slackinux")
            .icon("slackinux")
            .action("default", "Open Slackinux")
            .hint(Hint::Category("im.received".into()))
            .hint(Hint::Resident(true));

        if let Some(ref t) = tag {
            n.id(hash_tag(t));
        }

        match n.show() {
            Ok(handle) => {
                wkn.close();
                let ah = app_handle.clone();
                let notif_title = title.clone();
                std::thread::spawn(move || {
                    handle.wait_for_action(|action| {
                        if action == "default" {
                            info!("notification clicked: {notif_title}");
                            if let Some(window) = ah.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    });
                });
            }
            Err(e) => {
                warn!("notify-rust failed: {e}, falling back to WebKitGTK");
                return false;
            }
        }

        true
    });
}
