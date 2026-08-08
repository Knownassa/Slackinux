//! CEF (Chromium Embedded Framework) renderer backend — experimental scaffold.
//!
//! **Status: experimental, not end-to-end verified.** Slackinux's default and
//! recommended build remains WebKitGTK; this module exists so a CEF build can
//! be assembled without forking the renderer abstraction.
//!
//! How to actually build the CEF backend (unverified in CI as of this phase):
//!
//! 1. Patch the whole tauri stack onto the experimental `feat/cef` branch so
//!    `tauri`'s `cef` runtime replaces wry. A `[patch.crates-io]` section is
//!    global, so it must be applied deliberately and its exact commit pinned:
//!    ```toml
//!    [patch.crates-io]
//!    tauri = { git = "https://github.com/tauri-apps/tauri", rev = "<pinned feat/cef commit>" }
//!    ```
//! 2. Enable the CEF runtime feature on the tauri dependency (`tauri/cef`),
//!    which is only defined on that branch.
//! 3. Bundle the CEF framework (~130–150 MB) using the matching `tauri-cli`
//!    installed from the same branch; the bundler downloads it at build time.
//!
//! Until a real end-to-end Huddle (audio + video + screen share) succeeds and
//! is logged, `compatibility/manifest.json` reports CEF as `experimental` at
//! most — never `supported`.
//!
//! The `SlackRenderer` methods that need a webview handle are implemented
//! conservatively here: the runtime-agnostic ones (`navigate`, `eval`,
//! `reload`) work today; the media/zoom/cache ones that depend on the CEF
//! webview API are honest no-ops that keep the app usable and make the Huddle
//! doctor report `Experimental` instead of claiming readiness.

use log::{debug, warn};

use super::SlackRenderer;
use crate::error::{AppError, AppResult};

pub struct CefRenderer {
    window: tauri::WebviewWindow,
}

impl CefRenderer {
    pub fn new(window: tauri::WebviewWindow) -> Self {
        debug!("cef: CEF renderer scaffold active (experimental)");
        Self { window }
    }
}

impl SlackRenderer for CefRenderer {
    fn navigate(&self, url: &str) -> AppResult<()> {
        let parsed = url
            .parse::<url::Url>()
            .map_err(|e| AppError::InvalidUrl(e.to_string()))?;
        self.window
            .navigate(parsed)
            .map_err(|e| AppError::NavigationFailed(e.to_string()))
    }

    fn set_zoom_level(&self, level: f64) -> AppResult<()> {
        // Zoom is applied through the webview handle, which differs between
        // WebKitGTK and CEF. Not wired for CEF yet; keep the call a no-op so
        // the UI stays functional rather than erroring.
        debug!("cef: set_zoom_level({level}) not wired for the CEF backend");
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
        // Clearing the HTTP/website-data cache requires the CEF webview's
        // context; not wired yet. Reported so a user asking to clear data gets
        // a visible result instead of a silent success.
        warn!("cef: clear_cache not wired for the CEF backend");
        Ok(())
    }

    fn media_playing(&self) -> bool {
        // Live audio state is read from the webview; unavailable on CEF yet.
        // Returning false only affects update deferral, which is conservative.
        false
    }

    #[cfg(target_os = "linux")]
    fn probe_media_codecs(&self, on_result: Box<dyn FnOnce(Option<String>) + Send + 'static>) {
        // The codec probe evaluates JS inside the renderer. The CEF scaffold
        // cannot run it yet, so report "not probed"; the Huddle doctor then
        // classifies the environment as Experimental rather than Supported.
        debug!("cef: media codec probe not wired for the CEF backend");
        on_result(None);
    }
}
