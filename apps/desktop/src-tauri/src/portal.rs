//! XDG Desktop Portal integration for Slackinux.
//!
//! Uses the `ashpd` crate to open URIs via the desktop portal, providing a
//! consistent, sandbox-friendly way to launch external applications.

use url::Url;

/// Opens a URI using the XDG Desktop Portal's OpenURI interface.
///
/// Returns `Ok(())` when the portal accepted the open request.
#[cfg(target_os = "linux")]
async fn open_uri_via_portal(uri: &str) -> Result<(), String> {
    use ashpd::desktop::open_uri::OpenFileRequest;

    log::debug!("portal: opening {uri} via XDG Desktop Portal");
    let url = Url::parse(uri).map_err(|e| format!("portal: invalid uri {uri}: {e}"))?;
    OpenFileRequest::default()
        .send_uri(&url)
        .await
        .map(|_| ())
        .map_err(|e| format!("portal: open_uri failed: {e}"))
}

/// Synchronous wrapper that blocks on the portal future.
///
/// Used from synchronous contexts (e.g., WebKitGTK navigation policy
/// decisions). Drives the future on Tauri's async runtime.
#[cfg(target_os = "linux")]
fn open_uri_sync(uri: &str) -> Result<(), String> {
    tauri::async_runtime::block_on(open_uri_via_portal(uri))
}

/// Fallback using the `open` crate (spawns a detached process).
fn open_fallback(uri: &str) -> Result<(), String> {
    log::debug!("portal: opening {uri} via fallback (open crate)");
    open::that_detached(uri).map_err(|e| format!("fallback open failed: {e}"))
}

/// Opens a URI using the best available method.
///
/// On Linux, prefers XDG Desktop Portal; falls back to the `open` crate when
/// the portal is unavailable. On other platforms, uses the `open` crate
/// directly.
pub fn open_uri(uri: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        match open_uri_sync(uri) {
            Ok(()) => Ok(()),
            Err(e) => {
                log::warn!("portal: portal open failed ({e}), falling back to open crate");
                open_fallback(uri)
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        open_fallback(uri)
    }
}
