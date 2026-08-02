use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use log::{debug, error, info};
use tauri::Manager;
use tauri_plugin_dialog::{
    DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};
use tauri_plugin_updater::{Update, UpdaterExt};
use thiserror::Error;

use crate::{settings::Settings, AppState};

/// How long after startup the automatic check runs. Kept off the startup path
/// so opening Slack is never delayed by the network probe.
const STARTUP_CHECK_DELAY: Duration = Duration::from_secs(20);
/// Automatic checks run at most once per day.
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// Fallback page for package-managed and development builds.
const RELEASES_URL: &str = "https://github.com/Knownassa/Slackinux/releases";

/// Serializes checks and installs so the user can never run two at once.
static UPDATE_LOCK: AtomicBool = AtomicBool::new(false);

/// Why an update check was triggered. Automatic checks stay silent on network
/// failures; manual checks always report the outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCheckReason {
    Startup,
    Manual,
}

/// How Slackinux was installed. Only the AppImage is replaced in place;
/// package-managed installs defer to their own update mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallationKind {
    AppImage,
    PackageManaged,
    Development,
}

impl InstallationKind {
    pub fn detect() -> Self {
        Self::classify(std::env::var_os("APPIMAGE"), !cfg!(debug_assertions))
    }

    fn classify(appimage: Option<std::ffi::OsString>, is_release: bool) -> Self {
        if !is_release {
            return Self::Development;
        }
        if appimage.is_some() {
            return Self::AppImage;
        }
        Self::PackageManaged
    }
}

/// A newer release discovered on GitHub.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateMetadata {
    pub version: String,
    pub notes: Option<String>,
    pub published_at: Option<String>,
}

/// Typed failures surfaced to the UI.
#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("an update check or installation is already in progress")]
    AlreadyInProgress,
    #[error("the updater plugin is not available")]
    UpdaterUnavailable,
    #[error("update check failed: {0}")]
    Check(String),
    #[error("the update could not be downloaded or installed: {0}")]
    Install(String),
}

/// Waits briefly after startup, then performs an automatic check. Runs on a
/// background thread so a slow network never delays the window from opening.
pub fn schedule_startup_check(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(STARTUP_CHECK_DELAY);
        check_for_updates(app, UpdateCheckReason::Startup);
    });
}

/// Adds actionable context without claiming that valid GitHub JSON is broken.
/// Tauri also uses this generic error when the endpoint returns a temporary
/// non-success status, so the useful response is to retry or use Releases.
fn friendly_check_error(message: &str) -> String {
    let lower = message.to_ascii_lowercase();
    if lower.contains("release json") || lower.contains("could not fetch") {
        format!(
            "{message}\n\nGitHub did not return the Slackinux update feed. This can be \
             temporary, or GitHub may be blocked by your network. You can retry now or \
             open GitHub Releases and download the update manually."
        )
    } else if lower.contains("connection")
        || lower.contains("timed out")
        || lower.contains("network")
    {
        format!(
            "{message}\n\nCheck your internet connection, proxy, VPN, and system clock. \
             You can retry now or download the update from GitHub Releases."
        )
    } else {
        format!(
            "{message}\n\nYou can retry now or download the update manually from GitHub \
             Releases."
        )
    }
}

/// The single entry point for update checks. Manual checks always notify the
/// user of the outcome; automatic checks stay silent unless an update exists.
pub fn check_for_updates(app: tauri::AppHandle, reason: UpdateCheckReason) {
    tauri::async_runtime::spawn(async move {
        match run_check(&app, reason).await {
            Ok(()) => {}
            Err(UpdateError::AlreadyInProgress) => {
                info!("updates: check skipped, another check is running");
                if reason == UpdateCheckReason::Manual {
                    show_info(
                        &app,
                        "Slackinux is already checking for updates. Wait for the current \
                         check to finish.",
                        "Checking for Updates",
                    );
                }
            }
            Err(UpdateError::Check(message)) if reason == UpdateCheckReason::Manual => {
                show_check_error(&app, &message);
            }
            Err(err) if reason == UpdateCheckReason::Manual => {
                show_error(&app, "Update Check Failed", &err.to_string());
            }
            Err(err) => info!("updates: automatic check failed quietly: {err}"),
        }
    });
}

/// Gives a failed manual check a recovery path instead of a dead-end error.
fn show_check_error(app: &tauri::AppHandle, message: &str) {
    let action_app = app.clone();
    app.dialog()
        .message(friendly_check_error(message))
        .title("Update Check Failed")
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::YesNoCancelCustom(
            "Retry".into(),
            "Open Releases".into(),
            "Close".into(),
        ))
        .show_with_result(move |result| match result {
            MessageDialogResult::Yes => {
                check_for_updates(action_app.clone(), UpdateCheckReason::Manual);
            }
            MessageDialogResult::No => {
                open_release_page(&action_app);
            }
            MessageDialogResult::Custom(label) if label == "Retry" => {
                check_for_updates(action_app.clone(), UpdateCheckReason::Manual);
            }
            MessageDialogResult::Custom(label) if label == "Open Releases" => {
                open_release_page(&action_app);
            }
            _ => {}
        });
}

async fn run_check(app: &tauri::AppHandle, reason: UpdateCheckReason) -> Result<(), UpdateError> {
    let _guard = UpdateLockGuard::acquire().ok_or(UpdateError::AlreadyInProgress)?;

    let kind = InstallationKind::detect();
    info!("updates: installation kind = {kind:?}");

    if reason == UpdateCheckReason::Startup {
        if kind == InstallationKind::Development {
            info!("updates: development build, automatic checks are disabled");
            return Ok(());
        }
        let settings = current_settings(app);
        if !settings.auto_check_updates {
            info!("updates: automatic checks are disabled in settings");
            return Ok(());
        }
        let now = unix_now();
        let since_last = now.saturating_sub(settings.last_update_check_unix);
        if since_last < CHECK_INTERVAL.as_secs() as i64 {
            info!("updates: automatic check skipped, last check was under 24h ago");
            return Ok(());
        }
    }

    let updater = app.updater().map_err(|_| UpdateError::UpdaterUnavailable)?;
    let update = match updater.check().await {
        Ok(update) => update,
        Err(err) => return Err(UpdateError::Check(err.to_string())),
    };
    record_check(app);

    let Some(update) = update else {
        info!("updates: Slackinux is up to date");
        if reason == UpdateCheckReason::Manual {
            show_info(
                app,
                "You already have the latest version of Slackinux.",
                "Slackinux is up to date",
            );
        }
        return Ok(());
    };

    let metadata = UpdateMetadata {
        version: update.version.clone(),
        notes: update
            .body
            .as_deref()
            .filter(|notes| !notes.trim().is_empty())
            .map(ToOwned::to_owned),
        published_at: update.date.map(|date| date.to_string()),
    };
    info!(
        "updates: {} is available (published {:?})",
        metadata.version, metadata.published_at
    );
    prompt_for_update(app, update, &metadata, kind);
    Ok(())
}

/// Asks the user what to do with the discovered update. AppImage builds get a
/// full in-app install; everything else opens the GitHub release page instead
/// of touching files owned by a package manager.
fn prompt_for_update(
    app: &tauri::AppHandle,
    update: Update,
    metadata: &UpdateMetadata,
    kind: InstallationKind,
) {
    let prompt = format!(
        "Slackinux {version} is available.\n\n{notes}\n\nYou are currently using {current}.",
        version = metadata.version,
        notes = metadata
            .notes
            .as_deref()
            .unwrap_or("See the GitHub release for details."),
        current = update.current_version
    );

    let install_app = app.clone();
    match kind {
        InstallationKind::AppImage => {
            app.dialog()
                .message(prompt)
                .title("Slackinux Update Available")
                .kind(MessageDialogKind::Info)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "Download and Restart".into(),
                    "Later".into(),
                ))
                .show(move |download| {
                    if download {
                        if media_capture_active(&install_app) {
                            show_info(
                                &install_app,
                                "A Huddle or screen share appears to be active, so the update \
                                 was postponed. It will not be installed while you are in a call.",
                                "Update Postponed",
                            );
                            return;
                        }
                        // Acquire on the UI thread, then carry the guard into the
                        // download task so nothing can run a second check or
                        // install while this one is in progress.
                        let Some(guard) = UpdateLockGuard::acquire() else {
                            show_info(
                                &install_app,
                                "Another update operation is already in progress.",
                                "Updating Slackinux",
                            );
                            return;
                        };
                        install_update(install_app, update, guard);
                    }
                });
        }
        InstallationKind::PackageManaged | InstallationKind::Development => {
            app.dialog()
                .message(format!(
                    "{prompt}\n\nSlackinux was installed through your package manager, so it \
                     cannot replace itself. Open the GitHub release to install the update \
                     manually."
                ))
                .title("Slackinux Update Available")
                .kind(MessageDialogKind::Info)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "Open GitHub Release".into(),
                    "Later".into(),
                ))
                .show(move |open| {
                    if open {
                        open_release_page(&install_app);
                    }
                });
        }
    }
}

/// Downloads, verifies and installs an update, then restarts the app. Signature
/// verification is enforced by the updater plugin and cannot be disabled.
fn install_update(app: tauri::AppHandle, update: Update, guard: UpdateLockGuard) {
    show_info(
        &app,
        "The update is downloading. Slackinux will restart once it is installed.",
        "Updating Slackinux",
    );

    let progress_app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Held for the whole install so no second check or download can start.
        let _guard = guard;

        let result = update
            .download_and_install(
                |current, total| {
                    let percent = match total {
                        Some(total) if total > 0 => (current as f64 * 100.0 / total as f64) as u8,
                        _ => 0,
                    };
                    debug!(
                        "updates: download {percent}% ({current} of {} bytes)",
                        total.map_or_else(|| "?".into(), |t| t.to_string())
                    );
                },
                || info!("updates: download complete, verifying signature"),
            )
            .await
            .map_err(|err| UpdateError::Install(err.to_string()));

        match result {
            Ok(()) => {
                info!("updates: installed; restarting Slackinux");
                crate::restart_app(&progress_app);
            }
            Err(UpdateError::Install(message)) => {
                error!("updates: install failed: {message}");
                show_error(
                    &progress_app,
                    "Slackinux Update Failed",
                    &format!(
                        "The update could not be installed.\n\n{message}\n\nYou can still \
                         download it from GitHub Releases."
                    ),
                );
            }
            Err(err) => {
                error!("updates: install failed: {err}");
            }
        }
    });
}

/// Opens the GitHub Releases page, used by package-managed and development
/// builds that cannot replace their own files.
fn open_release_page(app: &tauri::AppHandle) {
    if let Err(err) = open::that_detached(RELEASES_URL) {
        error!("updates: could not open release page: {err}");
        show_error(
            app,
            "Open Release Failed",
            &format!("Could not open the GitHub Releases page.\n\n{err}"),
        );
    }
}

/// Uses WebKit's live audio state as the safest available Huddle signal. This
/// intentionally favors postponing an update over interrupting active media.
fn media_capture_active(app: &tauri::AppHandle) -> bool {
    app.try_state::<AppState>()
        .is_some_and(|state| state.renderer.media_playing())
}

fn current_settings(app: &tauri::AppHandle) -> Settings {
    app.try_state::<AppState>()
        .map(|state| state.settings())
        .unwrap_or_default()
}

/// Persists the timestamp of a completed check so the 24-hour automatic gate
/// has a fresh starting point.
fn record_check(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.set_last_update_check(unix_now());
        state.settings().save(&state.data_dir);
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn show_info(app: &tauri::AppHandle, message: &str, title: &str) {
    app.dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Info)
        .show(|_| {});
}

fn show_error(app: &tauri::AppHandle, title: &str, message: &str) {
    app.dialog()
        .message(message)
        .title(title)
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}

/// Released as soon as it is taken; restores the "idle" state on drop.
struct UpdateLockGuard;

impl UpdateLockGuard {
    fn acquire() -> Option<Self> {
        if UPDATE_LOCK.swap(true, Ordering::SeqCst) {
            None
        } else {
            Some(Self)
        }
    }
}

impl Drop for UpdateLockGuard {
    fn drop(&mut self) {
        UPDATE_LOCK.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(arg: &str) -> Option<std::ffi::OsString> {
        Some(std::ffi::OsString::from(arg))
    }

    #[test]
    fn dev_build_never_updates_in_place() {
        assert_eq!(
            InstallationKind::classify(os("/tmp/Slackinux.AppImage"), false),
            InstallationKind::Development
        );
        assert_eq!(
            InstallationKind::classify(None, false),
            InstallationKind::Development
        );
    }

    #[test]
    fn appimage_env_selects_self_update() {
        assert_eq!(
            InstallationKind::classify(os("/tmp/Slackinux.AppImage"), true),
            InstallationKind::AppImage
        );
    }

    #[test]
    fn no_appimage_env_means_package_managed() {
        assert_eq!(
            InstallationKind::classify(None, true),
            InstallationKind::PackageManaged
        );
    }

    #[test]
    fn lock_serializes_checks() {
        let first = UpdateLockGuard::acquire();
        assert!(first.is_some());
        assert!(UpdateLockGuard::acquire().is_none());
        drop(first);
        assert!(UpdateLockGuard::acquire().is_some());
    }

    #[test]
    fn release_feed_error_offers_recovery() {
        let message = friendly_check_error("Could not fetch a valid release JSON from the remote");
        assert!(message.contains("GitHub did not return the Slackinux update feed"));
        assert!(message.contains("retry now"));
        assert!(!message.contains("first Slackinux release"));
    }
}
