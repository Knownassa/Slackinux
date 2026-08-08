use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use log::{info, warn};
use tauri::Manager;

use super::SlackRenderer;
use crate::error::{AppError, AppResult};
use crate::notifications::{self, NotificationManager};

static POPUP_ID: AtomicU32 = AtomicU32::new(0);

pub struct WebKitRenderer {
    window: tauri::WebviewWindow,
    download_dir: std::path::PathBuf,
    data_dir: std::path::PathBuf,
    bootstrap_url: url::Url,
}

impl WebKitRenderer {
    pub fn new(
        window: tauri::WebviewWindow,
        download_dir: std::path::PathBuf,
        data_dir: std::path::PathBuf,
    ) -> Self {
        let bootstrap_url = window.url().unwrap_or_else(|_| {
            url::Url::parse("tauri://localhost/bootstrap/index.html")
                .expect("the static local bootstrap URL is valid")
        });
        info!("local recovery page: {bootstrap_url}");
        Self {
            window,
            download_dir,
            data_dir,
            bootstrap_url,
        }
    }

    fn recovery_url(&self, key: &str, value: &str) -> String {
        let mut url = self.bootstrap_url.clone();
        url.set_query(None);
        url.query_pairs_mut().append_pair(key, value);
        url.into()
    }

    #[cfg(target_os = "linux")]
    pub fn setup_linux<F>(
        &self,
        update_tooltip: F,
        notif_mgr: Arc<NotificationManager>,
        app_handle: tauri::AppHandle,
        permission_broker: Arc<crate::permissions::PermissionBroker>,
        media_activity: Arc<MediaActivity>,
    ) where
        F: Fn(&str) + Send + 'static,
    {
        self.enable_webrtc();
        self.enable_spellcheck();
        self.setup_navigation_policy(app_handle.clone());
        self.setup_load_recovery();
        self.setup_permissions(permission_broker, media_activity);
        self.setup_crash_recovery(app_handle.clone());
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
                    info!("Rendering: software compositing (explicit or confirmed fallback)");
                } else {
                    settings.set_hardware_acceleration_policy(HardwareAccelerationPolicy::OnDemand);
                    info!("Rendering: hardware acceleration on demand");
                }
                if crate::gpu::dmabuf_disabled() {
                    info!("Rendering: DMABUF disabled (compatibility mode)");
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
        let load_error_url = self.recovery_url("error", "load");
        let timeout_url = self.recovery_url("error", "timeout");
        let _ = self.window.with_webview(move |pw| {
            use webkit2gtk::{LoadEvent, WebViewExt};
            let wk = pw.inner();
            let load_generation = std::rc::Rc::new(std::cell::Cell::new(0_u64));

            wk.connect_load_failed(move |webview, _event, failing_uri, error| {
                if !failing_uri.starts_with("http://") && !failing_uri.starts_with("https://") {
                    return false;
                }
                warn!("Slack page failed to load: {error}");
                webview.load_uri(&load_error_url);
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
                let timeout_url = timeout_url.clone();
                gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(45), move || {
                    let uri = pending.uri().unwrap_or_default();
                    if current_generation.get() == generation
                        && pending.is_loading()
                        && (uri.starts_with("http://") || uri.starts_with("https://"))
                    {
                        warn!("Slack page remained blank/loading for 45 seconds; showing recovery");
                        pending.stop_loading();
                        pending.load_uri(&timeout_url);
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
        let webkit_data_dir = self.data_dir.join("webkit");
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

            // Slack's client gate only admits desktop Chrome (see
            // compatibility/manifest.json). WebKitGTK's real UA is not on that
            // allow-list, so before a main-frame navigation to a Slack-owned
            // host we mask the UA as desktop Chrome; everywhere else we keep
            // WebKitGTK's truthful UA. This must happen inside this handler
            // because WebKitGTK stops signal emission once a decide-policy
            // callback returns true.
            let (real_ua, slack_ua) = {
                use webkit2gtk::SettingsExt;
                let real_ua = wk.settings().and_then(|s| s.user_agent()).map(String::from);
                (real_ua, crate::navigation::slack_masked_user_agent())
            };

            let authentication_flow = std::rc::Rc::new(std::cell::Cell::new(false));
            wk.connect_decide_policy(move |webview, decision, decision_type| {
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
                        let host = url::Url::parse(uri.as_str())
                            .ok()
                            .and_then(|u| u.host_str().map(str::to_string));
                        if let Some(host) = host {
                            // Mask as desktop Chrome only for Slack's own hosts.
                            // Third-party SSO/analytics keep WebKitGTK's real UA.
                            let masked = crate::navigation::is_slack_owned_host(&host);
                            if let Some(settings) = webview.settings() {
                                use webkit2gtk::SettingsExt;
                                let target = if masked {
                                    Some(slack_ua.as_str())
                                } else {
                                    real_ua.as_deref()
                                };
                                if target != settings.user_agent().as_deref() {
                                    settings.set_user_agent(target);
                                    info!(
                                        "UA: {} Chrome mask for {host}",
                                        if masked { "applied" } else { "removed" }
                                    );
                                }
                            }
                        }
                        if webview
                            .uri()
                            .is_some_and(|current| is_slack_auth_page(current.as_str()))
                        {
                            authentication_flow.set(true);
                        }
                        if is_slack_client_page(uri.as_str()) {
                            authentication_flow.set(false);
                        }
                        match classify(uri.as_str()) {
                            Some(crate::navigation::NavigationDecision::AllowInternal) => {
                                info!(
                                    "navigation: {} -> AllowInternal",
                                    crate::deep_links::redact_sensitive_url(uri.as_str())
                                );
                                decision.use_();
                                true
                            }
                            Some(crate::navigation::NavigationDecision::OpenExternally) => {
                                // An SSO provider must stay in the same WebKit cookie
                                // context until it redirects back to Slack. This state
                                // is entered only from a Slack-owned sign-in page and
                                // ends as soon as `/client` is reached.
                                if authentication_flow.get() && is_https_url(uri.as_str()) {
                                    info!(
                                        "authentication navigation: {} -> AllowInternal",
                                        crate::deep_links::redact_sensitive_url(uri.as_str())
                                    );
                                    decision.use_();
                                    return true;
                                }
                                info!(
                                    "navigation: {} -> OpenExternally",
                                    crate::deep_links::redact_sensitive_url(uri.as_str())
                                );
                                let _ = crate::portal::open_uri(uri.as_str());
                                decision.ignore();
                                true
                            }
                            _ => {
                                warn!(
                                    "blocked navigation: {}",
                                    crate::deep_links::redact_sensitive_url(uri.as_str())
                                );
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
                                let from_authentication = authentication_flow.get()
                                    || webview
                                        .uri()
                                        .is_some_and(|current| {
                                            is_slack_auth_page(current.as_str())
                                        });
                                if !from_authentication {
                                    info!(
                                        "popup: {} -> OpenExternally",
                                        crate::deep_links::redact_sensitive_url(uri.as_str())
                                    );
                                    let _ = crate::portal::open_uri(uri.as_str());
                                    decision.ignore();
                                    return true;
                                }
                                if let Ok(url) = parsed {
                                    let id = POPUP_ID.fetch_add(1, Ordering::Relaxed);
                                    info!(
                                        "popup: {} -> in-app window popup-{id}",
                                        crate::deep_links::redact_sensitive_url(uri.as_str())
                                    );
                                    // Share the main window's profile-bound
                                    // WebContext (same data_directory key) so
                                    // SSO cookies land in the persistent session
                                    // and survive restarts. If the profile dir is
                                    // unavailable (fallback mode), build the
                                    // popup with a fresh context too.
                                    let popup_builder = tauri::WebviewWindowBuilder::new(
                                        &app_handle,
                                        format!("popup-{id}"),
                                        tauri::WebviewUrl::External(url),
                                    )
                                    .title("Slackinux — Sign in")
                                    .inner_size(900.0, 720.0);
                                    let popup_builder = if webkit_data_dir.exists() {
                                        popup_builder.data_directory(webkit_data_dir.clone())
                                    } else {
                                        popup_builder
                                    };
                                    let popup = popup_builder.build();
                                    match popup {
                                        Ok(popup) => {
                                            let main = app_handle.get_webview_window("main");
                                            let popup_to_close = popup.clone();
                                            let _ = popup.with_webview(move |popup_webview| {
                                                use webkit2gtk::{LoadEvent, WebViewExt};
                                                popup_webview.inner().connect_load_changed(
                                                    move |webview, event| {
                                                        if event != LoadEvent::Finished {
                                                            return;
                                                        }
                                                        let Some(uri) = webview.uri() else {
                                                            return;
                                                        };
                                                        if !is_slack_client_page(uri.as_str()) {
                                                            return;
                                                        }
                                                        info!(
                                                            "SSO completed; opening workspace in main window"
                                                        );
                                                        if let Some(main) = main.as_ref() {
                                                            if let Ok(url) =
                                                                url::Url::parse(uri.as_str())
                                                            {
                                                                let _ = main.navigate(url);
                                                                let _ = main.show();
                                                                let _ = main.set_focus();
                                                            }
                                                        }
                                                        let _ = popup_to_close.close();
                                                    },
                                                );
                                            });
                                        }
                                        Err(error) => warn!("could not create SSO window: {error}"),
                                    }
                                } else {
                                    warn!("popup: {uri} -> invalid URL, blocked");
                                }
                            }
                            "mailto" | "tel" => {
                                info!("popup: {uri} -> OpenExternally");
                                let _ = crate::portal::open_uri(uri.as_str());
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
    fn setup_permissions(
        &self,
        broker: Arc<crate::permissions::PermissionBroker>,
        activity: Arc<MediaActivity>,
    ) {
        self.setup_media_indicators(activity);
        let _ = self.window.with_webview(move |pw| {
            use gtk::prelude::*;
            use webkit2gtk::glib::Cast;
            use webkit2gtk::{
                NotificationPermissionRequest, PermissionRequestExt, UserMediaPermissionRequest,
                WebViewExt,
            };
            let wk = pw.inner();
            wk.connect_permission_request(move |webview, request| {
                let origin = webview
                    .uri()
                    .and_then(|value| url::Url::parse(value.as_str()).ok())
                    .and_then(|url| url.host_str().map(str::to_ascii_lowercase));
                let Some(host) = origin else {
                    // No origin means we cannot authorize media access.
                    request.deny();
                    warn!("permission request ignored: no origin available");
                    return true;
                };

                if request
                    .downcast_ref::<NotificationPermissionRequest>()
                    .is_some()
                {
                    let decision =
                        broker.decide(crate::permissions::MediaKind::Notifications, &host);
                    apply_decision(
                        &broker,
                        crate::permissions::MediaKind::Notifications,
                        &host,
                        decision,
                        request,
                        webview.toplevel().as_ref(),
                    );
                    true
                } else if request
                    .downcast_ref::<UserMediaPermissionRequest>()
                    .is_some()
                {
                    let kind = user_media_kind(request);
                    let decision = broker.decide(kind, &host);
                    apply_decision(
                        &broker,
                        kind,
                        &host,
                        decision,
                        request,
                        webview.toplevel().as_ref(),
                    );
                    true
                } else {
                    info!("permission request: unhandled type");
                    false
                }
            });
        });
    }

    #[cfg(target_os = "linux")]
    fn setup_crash_recovery(&self, app_handle: tauri::AppHandle) {
        let data_dir = self.data_dir.clone();
        let _ = self.window.with_webview(move |pw| {
            use webkit2gtk::{LoadEvent, WebViewExt};
            let wk = pw.inner();
            let reset_crashes = data_dir.clone();
            wk.connect_load_changed(move |_, event| {
                if event == LoadEvent::Finished {
                    // A clean load resets the staged recovery counters so a
                    // single transient crash does not cascade into fallback
                    // modes (GPU module keys state by session signature).
                    crate::gpu::record_success(&reset_crashes);
                }
            });
            wk.connect_web_process_terminated(move |webview, reason| {
                use webkit2gtk::WebProcessTerminationReason;
                match reason {
                    WebProcessTerminationReason::Crashed => {
                        let stage = crate::gpu::crash_stage();
                        let data_dir = data_dir.clone();
                        let ah = app_handle.clone();
                        match crate::gpu::record_crash(&data_dir) {
                            crate::gpu::CrashAction::Reload => {
                                warn!(
                                    "Web process crashed — reloading (recovery stage {stage} -> 1)"
                                );
                                webview.reload();
                            }
                            crate::gpu::CrashAction::RetryWithCompatibility => {
                                warn!("Web process crashed again — reloading with DMABUF disabled");
                                webview.reload();
                            }
                            crate::gpu::CrashAction::OfferSoftware => {
                                warn!(
                                    "Web process crashed repeatedly — offering software rendering"
                                );
                                offer_software_rendering(&ah, &data_dir);
                                webview.load_uri("about:blank");
                            }
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
        // Paths already handed out this session. `unique_download_path` checks
        // the filesystem, but WebKit materializes the destination file
        // asynchronously, so a burst of same-named downloads decided in one
        // main-loop iteration would otherwise all see the base name as free.
        // Reserving in memory closes that window within a single session.
        let reserved = std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
        let _ = self.window.with_webview(move |pw| {
            use webkit2gtk::{DownloadExt, WebContextExt, WebViewExt};
            let wk = pw.inner();
            if let Some(ctx) = wk.context() {
                ctx.connect_download_started(move |_ctx, download| {
                    let destination_directory = dd.clone();
                    let reserved = reserved.clone();
                    download.connect_decide_destination(move |download, suggested_filename| {
                        let name = safe_download_name(suggested_filename);
                        let destination = {
                            let mut reserved = match reserved.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            unique_download_path(
                                &destination_directory,
                                name.as_str(),
                                &mut reserved,
                            )
                        };
                        let Ok(destination_uri) = url::Url::from_file_path(&destination) else {
                            warn!("download blocked: invalid destination path");
                            return false;
                        };
                        download.set_destination(destination_uri.as_str());
                        info!("download started");
                        true
                    });
                    download.connect_failed(|_, error| warn!("download failed: {error}"));
                    download.connect_finished(|_| info!("download finished"));
                });
            }
        });
    }
}

/// Classifies a `UserMediaPermissionRequest` as microphone, camera, or screen
/// sharing. WebKitGTK 2.34+ reports the display-device flag separately from
/// the audio/video flags, so screen capture can be brokered on its own.
#[cfg(target_os = "linux")]
fn user_media_kind(request: &webkit2gtk::PermissionRequest) -> crate::permissions::MediaKind {
    use webkit2gtk::glib::translate::ToGlibPtr;
    use webkit2gtk::glib::Cast;
    use webkit2gtk::{UserMediaPermissionRequest, UserMediaPermissionRequestExt};
    if let Some(media) = request.downcast_ref::<UserMediaPermissionRequest>() {
        // The safe binding only exposes audio/video flags; the display flag
        // requires the raw symbol (v2_34), which the pinned crate gates behind
        // the `v2_34` feature enabled in Cargo.toml.
        let is_display = unsafe {
            webkit2gtk::ffi::webkit_user_media_permission_is_for_display_device(
                media.to_glib_none().0,
            ) != 0
        };
        if is_display {
            return crate::permissions::MediaKind::ScreenShare;
        }
        if media.is_for_audio_device() {
            return crate::permissions::MediaKind::Microphone;
        }
        return crate::permissions::MediaKind::Camera;
    }
    crate::permissions::MediaKind::Camera
}

/// Applies a broker decision to a live WebKitGTK permission request, prompting
/// the user when the broker asks. The decision is recorded before the request
/// is allowed/denied so subsequent requests honor it.
#[cfg(target_os = "linux")]
fn apply_decision(
    broker: &crate::permissions::PermissionBroker,
    kind: crate::permissions::MediaKind,
    host: &str,
    decision: crate::permissions::PermissionDecision,
    request: &webkit2gtk::PermissionRequest,
    parent: Option<&gtk::Widget>,
) {
    use webkit2gtk::glib::Cast;
    use webkit2gtk::PermissionRequestExt;

    let decision = match decision {
        crate::permissions::PermissionDecision::AskEveryTime => {
            let parent_window = parent
                .and_then(|widget| widget.downcast_ref::<gtk::Window>())
                .cloned();
            let choice = crate::permissions::prompt_user(kind, host, parent_window.as_ref());
            broker.record(kind, host, choice);
            choice
        }
        other => other,
    };

    match decision {
        crate::permissions::PermissionDecision::AlwaysAllow
        | crate::permissions::PermissionDecision::AllowOnce => {
            request.allow();
            info!("permission allowed: {} for {}", kind.label(), host);
        }
        crate::permissions::PermissionDecision::Block
        | crate::permissions::PermissionDecision::AskEveryTime => {
            request.deny();
            warn!(
                "permission denied: {} for {} ({decision:?})",
                kind.label(),
                host
            );
        }
    }
}

/// Live capture state for the three media sources Slackinux brokers. Updated
/// from WebKitGTK capture-state notifications so the UI can show an indicator
/// without polling.
#[cfg(target_os = "linux")]
#[derive(Default)]
pub struct MediaActivity {
    microphone: std::sync::atomic::AtomicBool,
    camera: std::sync::atomic::AtomicBool,
    screen_share: std::sync::atomic::AtomicBool,
}

#[cfg(target_os = "linux")]
impl MediaActivity {
    /// Snapshot of which media sources are currently capturing.
    pub fn active(&self) -> CaptureActive {
        CaptureActive {
            microphone: self.microphone.load(std::sync::atomic::Ordering::Relaxed),
            camera: self.camera.load(std::sync::atomic::Ordering::Relaxed),
            screen_share: self.screen_share.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

/// Snapshot returned by [`MediaActivity::active`].
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaptureActive {
    pub microphone: bool,
    pub camera: bool,
    pub screen_share: bool,
}

impl CaptureActive {
    /// `true` when any media source is capturing (e.g. during a Huddle).
    pub fn any(self) -> bool {
        self.microphone || self.camera || self.screen_share
    }
}

impl WebKitRenderer {
    /// Enables live capture-state indicators on the main webview. Call before
    /// the window is shown so the first state change is observed.
    #[cfg(target_os = "linux")]
    pub fn setup_media_indicators(&self, activity: Arc<MediaActivity>) {
        let _ = self.window.with_webview(move |pw| {
            use webkit2gtk::MediaCaptureState;
            use webkit2gtk::WebViewExt;
            let wk = pw.inner();
            let mic = activity.clone();
            wk.connect_microphone_capture_state_notify(move |webview| {
                let capturing = webview.microphone_capture_state() == MediaCaptureState::Active;
                mic.microphone
                    .store(capturing, std::sync::atomic::Ordering::Relaxed);
                info!(
                    "microphone capture state: {}",
                    if capturing { "active" } else { "inactive" }
                );
            });
            let cam = activity.clone();
            wk.connect_camera_capture_state_notify(move |webview| {
                let capturing = webview.camera_capture_state() == MediaCaptureState::Active;
                cam.camera
                    .store(capturing, std::sync::atomic::Ordering::Relaxed);
                info!(
                    "camera capture state: {}",
                    if capturing { "active" } else { "inactive" }
                );
            });
            let screen = activity.clone();
            wk.connect_display_capture_state_notify(move |webview| {
                let capturing = webview.display_capture_state() == MediaCaptureState::Active;
                screen
                    .screen_share
                    .store(capturing, std::sync::atomic::Ordering::Relaxed);
                info!(
                    "screen capture state: {}",
                    if capturing { "active" } else { "inactive" }
                );
            });
        });
    }
}

/// Shown after repeated web-process crashes: asks the user to confirm software
/// rendering. Only a confirmed choice persists, so a bad GPU driver cannot
/// silently degrade the experience; the user always owns the fallback decision.
#[cfg(target_os = "linux")]
fn offer_software_rendering(app_handle: &tauri::AppHandle, data_dir: &std::path::Path) {
    use tauri_plugin_dialog::DialogExt;
    use tauri_plugin_dialog::MessageDialogButtons;
    use tauri_plugin_dialog::MessageDialogKind;

    let data_dir = data_dir.to_path_buf();
    let handle = app_handle.clone();
    app_handle
        .dialog()
        .message(
            "Slackinux detected repeated rendering failures in the embedded web engine.\n\n\
             Switch to software rendering? This usually fixes blank or crashing Slack views \
             on affected graphics drivers.\n\n\
             You can change this later from the Graphics menu.",
        )
        .title("Slackinux — Rendering Problem")
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Switch to Software".into(),
            "Keep Trying".into(),
        ))
        .show(move |confirmed| {
            if confirmed {
                info!("software rendering confirmed by the user");
                crate::gpu::confirm_software(&data_dir);
                crate::restart_app(&handle);
            } else {
                info!("user declined software rendering; keeping the current mode");
            }
        });
}

fn is_https_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| url.scheme() == "https")
}

fn is_slack_auth_page(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let slack_host = host == "slack.com" || host.ends_with(".slack.com");
    slack_host
        && [
            "/signin",
            "/workspace-signin",
            "/sso",
            "/oauth",
            "/gantry/auth",
        ]
        .iter()
        .any(|path| url.path().starts_with(path))
}

fn is_slack_client_page(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    url.scheme() == "https"
        && (host == "app.slack.com" || host.ends_with(".slack.com"))
        && url.path().starts_with("/client")
}

fn safe_download_name(suggested: &str) -> String {
    let name = std::path::Path::new(suggested)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .unwrap_or("download");
    let sanitized = name
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '/' | '\\') {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "download".into()
    } else {
        sanitized
    }
}

fn unique_download_path(
    directory: &std::path::Path,
    name: &str,
    reserved: &mut std::collections::HashSet<std::path::PathBuf>,
) -> std::path::PathBuf {
    let original = directory.join(name);
    let taken = |path: &std::path::Path| path.exists() || reserved.contains(path);
    if !taken(&original) {
        reserved.insert(original.clone());
        return original;
    }

    let path = std::path::Path::new(name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 1..10_000 {
        let candidate_name = match extension {
            Some(extension) => format!("{stem} ({index}).{extension}"),
            None => format!("{stem} ({index})"),
        };
        let candidate = directory.join(candidate_name);
        if !taken(&candidate) {
            reserved.insert(candidate.clone());
            return candidate;
        }
    }
    let fallback = directory.join(format!("download-{}", std::process::id()));
    reserved.insert(fallback.clone());
    fallback
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

    #[cfg(target_os = "linux")]
    fn probe_media_codecs(&self, on_result: Box<dyn FnOnce(Option<String>) + Send + 'static>) {
        let _ = self.window.with_webview(move |pw| {
            use javascriptcore::ValueExt;
            use webkit2gtk::gio::Cancellable;
            use webkit2gtk::WebViewExt;
            let script = crate::huddles::codec_probe_script();
            pw.inner().evaluate_javascript(
                script,
                None,
                None,
                None::<&Cancellable>,
                move |result| match result {
                    Ok(value) => on_result(Some(value.to_str().to_string())),
                    Err(error) => {
                        warn!("huddle codec probe failed: {error}");
                        on_result(None);
                    }
                },
            );
        });
    }
}

#[cfg(test)]
mod download_tests {
    use super::{
        is_slack_auth_page, is_slack_client_page, safe_download_name, unique_download_path,
    };

    #[test]
    fn recognizes_only_slack_authentication_and_client_pages() {
        assert!(is_slack_auth_page("https://app.slack.com/workspace-signin"));
        assert!(is_slack_auth_page("https://example.slack.com/sso/start"));
        assert!(!is_slack_auth_page("https://evil.example/signin"));
        assert!(is_slack_client_page(
            "https://app.slack.com/client/T123/C456"
        ));
        assert!(!is_slack_client_page("https://evil.example/client/T123"));
    }

    #[test]
    fn sanitizes_download_names_and_avoids_overwriting() {
        assert_eq!(safe_download_name("../report.pdf"), "report.pdf");
        assert_eq!(safe_download_name(".."), "download");
        assert_eq!(safe_download_name("bad\nname.txt"), "bad_name.txt");

        let directory =
            std::env::temp_dir().join(format!("slackinux-download-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("report.pdf"), b"existing").unwrap();
        let mut reserved = std::collections::HashSet::new();
        assert_eq!(
            unique_download_path(&directory, "report.pdf", &mut reserved),
            directory.join("report (1).pdf")
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn burst_of_same_named_downloads_never_share_a_path() {
        let directory =
            std::env::temp_dir().join(format!("slackinux-download-burst-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut reserved = std::collections::HashSet::new();
        let mut paths = std::collections::HashSet::new();
        for _ in 0..50 {
            let path = unique_download_path(&directory, "report.pdf", &mut reserved);
            assert!(
                paths.insert(path.clone()),
                "duplicate download path {path:?}"
            );
        }
        assert_eq!(paths.len(), 50);
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn reserved_paths_still_skipped_after_files_removed() {
        let directory = std::env::temp_dir().join(format!(
            "slackinux-download-reserved-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let mut reserved = std::collections::HashSet::new();
        let first = unique_download_path(&directory, "report.pdf", &mut reserved);
        std::fs::write(&first, b"data").unwrap();
        std::fs::remove_file(&first).unwrap();
        let second = unique_download_path(&directory, "report.pdf", &mut reserved);
        assert_ne!(first, second);
        let _ = std::fs::remove_dir_all(directory);
    }
}
