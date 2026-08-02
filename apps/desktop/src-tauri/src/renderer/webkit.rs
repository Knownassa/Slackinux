use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use log::{info, warn};

use super::SlackRenderer;
use crate::error::{AppError, AppResult};
use crate::notifications::{self, NotificationManager};

static POPUP_ID: AtomicU32 = AtomicU32::new(0);

pub struct WebKitRenderer {
    window: tauri::WebviewWindow,
    download_dir: std::path::PathBuf,
}

impl WebKitRenderer {
    pub fn new(window: tauri::WebviewWindow, download_dir: std::path::PathBuf) -> Self {
        Self {
            window,
            download_dir,
        }
    }

    #[cfg(target_os = "linux")]
    pub fn setup_linux<F>(
        &self,
        update_tooltip: F,
        notif_mgr: Arc<NotificationManager>,
        app_handle: tauri::AppHandle,
    ) where
        F: Fn(&str) + Send + 'static,
    {
        self.enable_webrtc();
        self.enable_spellcheck();
        self.setup_navigation_policy(app_handle.clone());
        self.setup_load_recovery();
        self.setup_permissions();
        self.setup_crash_recovery();
        self.setup_notifications(notif_mgr, app_handle);
        self.setup_title_tracking(update_tooltip);
        self.setup_download_handler();
        self.probe_color_scheme();
    }

    #[cfg(target_os = "linux")]
    fn enable_webrtc(&self) {
        let _ = self.window.with_webview(|pw| {
            use webkit2gtk::{HardwareAccelerationPolicy, SettingsExt, WebViewExt};
            let wk = pw.inner();
            if let Some(settings) = wk.settings() {
                settings.set_enable_webrtc(true);
                if crate::gpu::software_rendering_enabled() {
                    settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::Never);
                    info!("Rendering: software compositing (Wayland/NVIDIA compatibility)");
                } else {
                    settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::OnDemand);
                    info!("Rendering: hardware acceleration on demand");
                }
                settings.set_enable_smooth_scrolling(true);
                info!("WebRTC: enabled via WebKitGTK settings");
                info!("Rendering: smooth scrolling enabled");
            } else {
                warn!("WebRTC: could not get WebKitGTK settings");
            }
        });
    }

    #[cfg(target_os = "linux")]
    fn setup_load_recovery(&self) {
        let _ = self.window.with_webview(|pw| {
            use webkit2gtk::{LoadEvent, WebViewExt};
            let wk = pw.inner();
            let load_generation = std::rc::Rc::new(std::cell::Cell::new(0_u64));

            wk.connect_load_failed(|webview, _event, failing_uri, error| {
                if !failing_uri.starts_with("http://") && !failing_uri.starts_with("https://") {
                    return false;
                }
                warn!("Slack page failed to load: {error}");
                webview.load_uri("tauri://localhost/bootstrap/index.html?error=load");
                true
            });

            wk.connect_load_changed(move |webview, event| {
                if event != LoadEvent::Started {
                    return;
                }
                let generation = load_generation.get().wrapping_add(1);
                load_generation.set(generation);
                let current_generation = load_generation.clone();
                let pending = webview.clone();
                gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(45), move || {
                    let uri = pending.uri().unwrap_or_default();
                    if current_generation.get() == generation
                        && pending.is_loading()
                        && (uri.starts_with("http://") || uri.starts_with("https://"))
                    {
                        warn!("Slack page remained blank/loading for 45 seconds; showing recovery");
                        pending.stop_loading();
                        pending.load_uri("tauri://localhost/bootstrap/index.html?error=timeout");
                    }
                });
            });
        });
    }

    #[cfg(target_os = "linux")]
    fn enable_spellcheck(&self) {
        let _ = self.window.with_webview(|pw| {
            use webkit2gtk::{WebContextExt, WebViewExt};
            let wk = pw.inner();
            if let Some(ctx) = wk.context() {
                ctx.set_spell_checking_enabled(true);
                ctx.set_spell_checking_languages(&["en_US"]);
                info!("Spellcheck: enabled (en_US)");
            } else {
                warn!("Spellcheck: could not get WebKitGTK context");
            }
        });
    }

    #[cfg(target_os = "linux")]
    fn setup_navigation_policy(&self, app_handle: tauri::AppHandle) {
        let _ = self.window.with_webview(move |pw| {
            use webkit2gtk::glib::Cast;
            use webkit2gtk::{
                NavigationPolicyDecisionExt, PolicyDecisionExt, PolicyDecisionType,
                ResponsePolicyDecisionExt, URIRequestExt, WebViewExt,
            };

            let classify = |uri: &str| -> Option<crate::navigation::NavigationDecision> {
                url::Url::parse(uri)
                    .ok()
                    .map(|u| crate::navigation::classify_url(&u))
            };

            let wk = pw.inner();
            wk.connect_decide_policy(move |_webview, decision, decision_type| {
                match decision_type {
                    PolicyDecisionType::Response => {
                        let Some(resp) =
                            decision.downcast_ref::<webkit2gtk::ResponsePolicyDecision>()
                        else {
                            return false;
                        };
                        // Sub-frames (iframes) and sub-resources are part of the Slack
                        // page (analytics, SSO, embeds): let them load normally and
                        // never open the external browser for them.
                        if !resp.is_main_frame_main_resource() {
                            return false;
                        }
                        let Some(uri) = resp.request().and_then(|r| r.uri()) else {
                            return false;
                        };
                        match classify(uri.as_str()) {
                            Some(crate::navigation::NavigationDecision::AllowInternal) => {
                                info!("navigation: {uri} -> AllowInternal");
                                decision.use_();
                                true
                            }
                            Some(crate::navigation::NavigationDecision::OpenExternally) => {
                                info!("navigation: {uri} -> OpenExternally");
                                let _ = open::that_detached(uri.as_str());
                                decision.ignore();
                                true
                            }
                            _ => {
                                warn!("blocked navigation: {uri}");
                                decision.ignore();
                                true
                            }
                        }
                    }
                    PolicyDecisionType::NewWindowAction => {
                        let Some(nav) =
                            decision.downcast_ref::<webkit2gtk::NavigationPolicyDecision>()
                        else {
                            return false;
                        };
                        let Some(uri) = {
                            #[allow(deprecated)]
                            nav.request()
                        }
                        .and_then(|r| r.uri()) else {
                            decision.ignore();
                            return true;
                        };
                        let parsed = url::Url::parse(uri.as_str());
                        let scheme = parsed
                            .as_ref()
                            .map(|u| u.scheme().to_string())
                            .unwrap_or_default();
                        match scheme.as_str() {
                            // Auth/SSO popups (Slack workspace sign-in, Google,
                            // etc.) open in an in-app window so cookies are
                            // shared with the main webview and the sign-in
                            // completes without leaving the app.
                            "http" | "https" => {
                                if let Ok(url) = parsed {
                                    let id = POPUP_ID.fetch_add(1, Ordering::Relaxed);
                                    info!("popup: {uri} -> in-app window popup-{id}");
                                    let _ = tauri::WebviewWindowBuilder::new(
                                        &app_handle,
                                        format!("popup-{id}"),
                                        tauri::WebviewUrl::External(url),
                                    )
                                    .title("Slackinux — Sign in")
                                    .inner_size(900.0, 720.0)
                                    .build();
                                } else {
                                    warn!("popup: {uri} -> invalid URL, blocked");
                                }
                            }
                            "mailto" | "tel" => {
                                info!("popup: {uri} -> OpenExternally");
                                let _ = open::that_detached(uri.as_str());
                            }
                            _ => {
                                info!("popup: {uri} -> blocked");
                            }
                        }
                        decision.ignore();
                        true
                    }
                    // NavigationAction decisions are handled by Tauri's layer; other
                    // decision types use WebKitGTK's defaults.
                    _ => false,
                }
            });
        });
    }

    #[cfg(target_os = "linux")]
    fn probe_color_scheme(&self) {
        let _ = self.window.with_webview(|pw| {
            use webkit2gtk::{LoadEvent, WebViewExt};
            let wk = pw.inner();
            wk.connect_load_changed(|webview, event| {
                if event != LoadEvent::Finished || !log::log_enabled!(log::Level::Debug) {
                    return;
                }
                use javascriptcore::ValueExt;
                use webkit2gtk::gio::Cancellable;
                webview.evaluate_javascript(
                    "JSON.stringify({dark: matchMedia('(prefers-color-scheme: dark)').matches, \
                     scheme: getComputedStyle(document.documentElement).colorScheme, \
                     bg: getComputedStyle(document.body).backgroundColor})",
                    None,
                    None,
                    None::<&Cancellable>,
                    |res| {
                        if let Ok(v) = res {
                            info!("web probe: page color-scheme -> {}", v.to_str());
                        }
                    },
                );
            });
        });
    }

    #[cfg(target_os = "linux")]
    fn setup_permissions(&self) {
        let _ = self.window.with_webview(|pw| {
            use webkit2gtk::glib::Cast;
            use webkit2gtk::{
                NotificationPermissionRequest, PermissionRequestExt, UserMediaPermissionRequest,
                WebViewExt,
            };
            let wk = pw.inner();
            wk.connect_permission_request(|webview, request| {
                if request
                    .downcast_ref::<NotificationPermissionRequest>()
                    .is_some()
                {
                    request.allow();
                    info!("notification permission allowed");
                    true
                } else if request
                    .downcast_ref::<UserMediaPermissionRequest>()
                    .is_some()
                {
                    let trusted = webview
                        .uri()
                        .and_then(|value| url::Url::parse(value.as_str()).ok())
                        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
                        .is_some_and(|host| host == "slack.com" || host.ends_with(".slack.com"));
                    if trusted {
                        request.allow();
                        info!("camera/microphone permission allowed for Slack");
                    } else {
                        request.deny();
                        warn!("camera/microphone permission denied for an untrusted origin");
                    }
                    true
                } else {
                    info!("permission request: unhandled type");
                    false
                }
            });
        });
    }

    #[cfg(target_os = "linux")]
    fn setup_crash_recovery(&self) {
        let _ = self.window.with_webview(|pw| {
            use webkit2gtk::{LoadEvent, WebViewExt};
            let wk = pw.inner();
            let consecutive_crashes = std::rc::Rc::new(std::cell::Cell::new(0_u8));
            let reset_crashes = consecutive_crashes.clone();
            wk.connect_load_changed(move |_, event| {
                if event == LoadEvent::Finished {
                    reset_crashes.set(0);
                }
            });
            wk.connect_web_process_terminated(move |webview, reason| {
                use webkit2gtk::WebProcessTerminationReason;
                match reason {
                    WebProcessTerminationReason::Crashed => {
                        let attempts = consecutive_crashes.get().saturating_add(1);
                        consecutive_crashes.set(attempts);
                        if attempts <= 2 {
                            warn!("Web process crashed — reload attempt {attempts}/2");
                            webview.reload();
                        } else {
                            warn!(
                                "Web process crashed repeatedly — stopping automatic reload loop"
                            );
                        }
                    }
                    WebProcessTerminationReason::ExceededMemoryLimit => {
                        warn!("Web process exceeded memory limit — reloading");
                        webview.reload();
                    }
                    WebProcessTerminationReason::TerminatedByApi => {
                        info!("Web process terminated by API");
                    }
                    _ => {
                        warn!("Web process terminated — reloading");
                        webview.reload();
                    }
                }
            });
        });
    }

    #[cfg(target_os = "linux")]
    fn setup_notifications(&self, mgr: Arc<NotificationManager>, ah: tauri::AppHandle) {
        let _ = self.window.with_webview(|pw| {
            notifications::setup_notification_handler(&pw.inner(), mgr, ah);
        });
    }

    #[cfg(target_os = "linux")]
    fn setup_title_tracking<F>(&self, update_tooltip: F)
    where
        F: Fn(&str) + Send + 'static,
    {
        let _ = self.window.with_webview(|pw| {
            use webkit2gtk::WebViewExt;
            let wk = pw.inner();
            wk.connect_title_notify(move |webview| {
                let title: String = webview.title().unwrap_or_default().into();
                update_tooltip(&title);
            });
        });
    }

    #[cfg(target_os = "linux")]
    fn setup_download_handler(&self) {
        let dd = self.download_dir.clone();
        let _ = self.window.with_webview(move |pw| {
            use webkit2gtk::{DownloadExt, URIRequestExt, WebContextExt, WebViewExt};
            let wk = pw.inner();
            if let Some(ctx) = wk.context() {
                ctx.connect_download_started(move |_ctx, download| {
                    let name = download
                        .request()
                        .and_then(|r| r.uri())
                        .and_then(|u| {
                            std::path::Path::new(u.as_str())
                                .file_name()
                                .map(|f| f.to_string_lossy().into_owned())
                        })
                        .unwrap_or_else(|| "download".into());
                    let dest = dd.join(&name);
                    download.set_destination(dest.to_string_lossy().as_ref());
                    info!("download started: {name}");
                });
            }
        });
    }
}

impl SlackRenderer for WebKitRenderer {
    fn navigate(&self, url: &str) -> AppResult<()> {
        let parsed = url
            .parse::<url::Url>()
            .map_err(|e| AppError::InvalidUrl(e.to_string()))?;
        self.window
            .navigate(parsed)
            .map_err(|e| AppError::NavigationFailed(e.to_string()))
    }

    fn set_zoom_level(&self, level: f64) -> AppResult<()> {
        #[cfg(target_os = "linux")]
        {
            let _ = self.window.with_webview(move |pw| {
                use webkit2gtk::WebViewExt;
                pw.inner().set_zoom_level(level);
            });
        }
        Ok(())
    }

    fn eval(&self, js: &str) -> AppResult<()> {
        self.window
            .eval(js)
            .map_err(|e| AppError::NavigationFailed(e.to_string()))
    }

    fn reload(&self) -> AppResult<()> {
        self.eval("location.reload()")
    }

    fn clear_cache(&self) -> AppResult<()> {
        #[cfg(target_os = "linux")]
        {
            let _ = self.window.with_webview(|pw| {
                use webkit2gtk::gio::Cancellable;
                use webkit2gtk::glib::TimeSpan;
                use webkit2gtk::WebsiteDataManagerExtManual;
                use webkit2gtk::{WebContextExt, WebViewExt, WebsiteDataTypes};
                if let Some(ctx) = pw.inner().context() {
                    if let Some(mgr) = ctx.website_data_manager() {
                        mgr.clear(
                            WebsiteDataTypes::ALL,
                            TimeSpan(0),
                            None::<&Cancellable>,
                            |result| {
                                if result.is_ok() {
                                    info!("website data cleared");
                                } else {
                                    warn!("failed to clear website data: {result:?}");
                                }
                            },
                        );
                    }
                }
            });
        }
        Ok(())
    }

    fn media_playing(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            let playing = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let result = playing.clone();
            let _ = self.window.with_webview(move |pw| {
                use webkit2gtk::WebViewExt;
                result.store(
                    pw.inner().is_playing_audio(),
                    std::sync::atomic::Ordering::Relaxed,
                );
            });
            playing.load(std::sync::atomic::Ordering::Relaxed)
        }

        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }
}
