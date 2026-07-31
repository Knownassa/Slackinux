use std::time::Duration;

use log::{error, info};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

/// Checks shortly after startup without delaying the Slack window. Network
/// failures stay silent here so an offline launch never interrupts the user.
pub fn schedule_startup_check(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(8));
        check_for_updates(app, false);
    });
}

/// Checks GitHub Releases for a signed update. Manual checks report every
/// outcome; automatic checks only interrupt the user when an update exists.
pub fn check_for_updates(app: tauri::AppHandle, report_current: bool) {
    tauri::async_runtime::spawn(async move {
        let updater = match app.updater() {
            Ok(updater) => updater,
            Err(err) => {
                report_error(&app, report_current, &err.to_string());
                return;
            }
        };

        match updater.check().await {
            Ok(Some(update)) => {
                let version = update.version.clone();
                let notes = update
                    .body
                    .as_deref()
                    .filter(|notes| !notes.trim().is_empty())
                    .unwrap_or("See the GitHub release for details.");
                let prompt = format!(
                    "Slackinux {version} is available.\n\n{notes}\n\nDownload and install it now?"
                );
                let install_app = app.clone();
                app.dialog()
                    .message(prompt)
                    .title("Slackinux Update Available")
                    .kind(MessageDialogKind::Info)
                    .buttons(MessageDialogButtons::OkCancelCustom(
                        "Update now".into(),
                        "Later".into(),
                    ))
                    .show(move |install| {
                        if install {
                            install_update(install_app, update);
                        }
                    });
            }
            Ok(None) => {
                info!("updater: Slackinux is current");
                if report_current {
                    app.dialog()
                        .message("You already have the latest version of Slackinux.")
                        .title("Slackinux Updates")
                        .kind(MessageDialogKind::Info)
                        .show(|_| {});
                }
            }
            Err(err) => report_error(&app, report_current, &err.to_string()),
        }
    });
}

fn install_update(app: tauri::AppHandle, update: tauri_plugin_updater::Update) {
    let progress_app = app.clone();
    app.dialog()
        .message("The update is downloading. Slackinux will restart after installation.")
        .title("Updating Slackinux")
        .kind(MessageDialogKind::Info)
        .show(|_| {});

    tauri::async_runtime::spawn(async move {
        match update.download_and_install(|_, _| {}, || {}).await {
            Ok(()) => {
                info!("updater: update installed; restarting");
                progress_app.restart();
            }
            Err(err) => {
                error!("updater: install failed: {err}");
                progress_app
                    .dialog()
                    .message(format!(
                        "The update could not be installed.\n\n{err}\n\nYou can still download it from GitHub Releases."
                    ))
                    .title("Slackinux Update Failed")
                    .kind(MessageDialogKind::Error)
                    .show(|_| {});
            }
        }
    });
}

fn report_error(app: &tauri::AppHandle, visible: bool, message: &str) {
    error!("updater: check failed: {message}");
    if visible {
        app.dialog()
            .message(format!(
                "Slackinux could not check GitHub for updates.\n\n{message}"
            ))
            .title("Update Check Failed")
            .kind(MessageDialogKind::Error)
            .show(|_| {});
    }
}
