use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use log::{error, info};
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

const LOG_FILE_NAME: &str = "slackinux.log";
const PREVIOUS_LOG_FILE_NAME: &str = "slackinux.previous.log";
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
const ISSUE_URL: &str = "https://github.com/Knownassa/Slackinux/issues/new";

static LOG_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();

struct LogWriter {
    file: File,
}

impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(bytes);
        self.file.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stderr().flush();
        self.file.flush()
    }
}

/// Starts a small persistent log alongside stderr. Only one previous 2 MiB
/// file is retained so diagnostics cannot grow without bound.
pub fn init_logging() {
    let directory = log_directory_from_env();
    let _ = LOG_DIRECTORY.set(directory.clone());

    let mut builder =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
    builder.format_timestamp_millis();

    if let Err(err) = fs::create_dir_all(&directory) {
        eprintln!("Slackinux could not create its log directory: {err}");
        builder.target(env_logger::Target::Stderr).init();
        return;
    }

    let path = directory.join(LOG_FILE_NAME);
    rotate_if_needed(&path);
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => {
            builder.target(env_logger::Target::Pipe(Box::new(LogWriter { file })));
            builder.init();
            info!(
                "diagnostic log: {}",
                crate::redact_path(&path.display().to_string())
            );
        }
        Err(err) => {
            eprintln!("Slackinux could not open its diagnostic log: {err}");
            builder.target(env_logger::Target::Stderr).init();
        }
    }
}

pub fn open_log_folder(app: &tauri::AppHandle) {
    let directory = log_directory();
    if let Err(err) = fs::create_dir_all(&directory)
        .and_then(|_| open::that_detached(&directory).map_err(io::Error::other))
    {
        error!("could not open diagnostic log folder: {err}");
        app.dialog()
            .message(format!(
                "Could not open the diagnostic log folder.\n\n{err}"
            ))
            .title("Slackinux Diagnostics")
            .kind(MessageDialogKind::Error)
            .show(|_| {});
    }
}

pub fn copy_support_report(app: &tauri::AppHandle) {
    let report = support_report(app);
    #[cfg(target_os = "linux")]
    {
        let clipboard = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
        clipboard.set_text(&report);
        clipboard.store();
    }

    app.dialog()
        .message(
            "A privacy-safe diagnostic summary was copied to the clipboard.\n\nNo Slack messages, cookies, tokens, or workspace content are included.",
        )
        .title("Slackinux Diagnostics")
        .kind(MessageDialogKind::Info)
        .show(|_| {});
}

pub fn report_issue(app: &tauri::AppHandle) {
    let url = issue_url(&support_report(app));
    if let Err(err) = open::that_detached(url.as_str()) {
        error!("could not open the Slackinux issue reporter: {err}");
        app.dialog()
            .message(format!(
                "Could not open the GitHub issue reporter.\n\n{err}\n\nOpen {ISSUE_URL} manually."
            ))
            .title("Report a Slackinux Issue")
            .kind(MessageDialogKind::Error)
            .show(|_| {});
    }
}

fn support_report(app: &tauri::AppHandle) -> String {
    let distro = linux_pretty_name().unwrap_or_else(|| "Unknown Linux distribution".into());
    let session = clean_environment_value("XDG_SESSION_TYPE");
    let desktop = clean_environment_value("XDG_CURRENT_DESKTOP");
    let installation = if std::env::var_os("APPIMAGE").is_some() {
        "AppImage"
    } else {
        "Package or development build"
    };
    let graphics = {
        #[cfg(target_os = "linux")]
        {
            crate::gpu::applied()
                .map(|applied| applied.describe())
                .unwrap_or_else(|| "unknown (policy not applied yet)".into())
        }
        #[cfg(not(target_os = "linux"))]
        {
            "unknown".into()
        }
    };

    let media = {
        #[cfg(target_os = "linux")]
        {
            use crate::permissions::MediaKind;
            let state = app.state::<crate::AppState>();
            let capture = state.media_activity.active();
            let mut lines = format!(
                "{} | mic={} camera={} screen={}",
                if capture.any() {
                    "capturing now"
                } else {
                    "not capturing"
                },
                capture.microphone,
                capture.camera,
                capture.screen_share
            );
            let mut any_saved = false;
            for kind in [
                MediaKind::Microphone,
                MediaKind::Camera,
                MediaKind::ScreenShare,
                MediaKind::Notifications,
            ] {
                let saved = state.permission_broker.managed_hosts(kind).len();
                if saved == 0 {
                    continue;
                }
                any_saved = true;
                lines.push_str(&format!(
                    "\n  saved decisions: {saved} host(s) with a saved {} setting",
                    kind.label()
                ));
            }
            if !any_saved {
                lines.push_str("\n  no saved permission decisions (all prompt every time)");
            }
            lines
        }
        #[cfg(not(target_os = "linux"))]
        {
            "unavailable on this platform".into()
        }
    };

    let huddles = {
        #[cfg(target_os = "linux")]
        {
            let report = crate::huddles::probe_environment();
            format!(
                "{} ({}; {})",
                report.classify().label(),
                if report.pipewire_connected {
                    "pipewire up"
                } else {
                    "pipewire down"
                },
                if report.screencast_portal {
                    "portal screencast up"
                } else {
                    "portal screencast down"
                },
            )
        }
        #[cfg(not(target_os = "linux"))]
        {
            "unavailable on this platform".into()
        }
    };

    format!(
        "Slackinux diagnostics\n\
         - Version: {}\n\
         - System: {} ({})\n\
         - Architecture: {}\n\
         - Session: {}\n\
         - Desktop: {}\n\
         - Installation: {}\n\
         - Graphics: {}\n\
         - Media permissions: {}\n\
         - Huddles: {}\n\
         - Log file: {}\n",
        env!("CARGO_PKG_VERSION"),
        distro,
        std::env::consts::OS,
        std::env::consts::ARCH,
        session,
        desktop,
        installation,
        graphics,
        media,
        huddles,
        LOG_FILE_NAME,
    )
}

fn issue_url(report: &str) -> url::Url {
    let mut url = url::Url::parse(ISSUE_URL).expect("static issue URL must be valid");
    let body = format!(
        "## What happened?\n\nDescribe the problem clearly.\n\n\
         ## Steps to reproduce\n\n1. \n2. \n3. \n\n\
         ## Expected behavior\n\nWhat did you expect?\n\n\
         ## Diagnostics\n\n```text\n{report}```\n\n\
         If useful, attach `{LOG_FILE_NAME}` from **Help → Diagnostics → Open Log Folder**. \
         Please review the log before uploading it."
    );
    url.query_pairs_mut()
        .append_pair("labels", "bug")
        .append_pair("title", "[Bug] ")
        .append_pair("body", &body);
    url
}

fn log_directory() -> PathBuf {
    LOG_DIRECTORY
        .get()
        .cloned()
        .unwrap_or_else(log_directory_from_env)
}

fn log_directory_from_env() -> PathBuf {
    nonempty_env("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| nonempty_env("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir)
        .join("slackinux/logs")
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn clean_environment_value(name: &str) -> String {
    nonempty_env(name)
        .map(|value| value.replace(['\r', '\n'], " "))
        .unwrap_or_else(|| "unknown".into())
}

fn linux_pretty_name() -> Option<String> {
    let os_release = fs::read_to_string("/etc/os-release").ok()?;
    parse_pretty_name(&os_release)
}

fn parse_pretty_name(os_release: &str) -> Option<String> {
    os_release.lines().find_map(|line| {
        let value = line.strip_prefix("PRETTY_NAME=")?.trim();
        let value = value.trim_matches('"').replace(['\r', '\n'], " ");
        (!value.is_empty()).then_some(value)
    })
}

fn rotate_if_needed(path: &Path) {
    let should_rotate = fs::metadata(path)
        .map(|metadata| metadata.len() >= MAX_LOG_BYTES)
        .unwrap_or(false);
    if !should_rotate {
        return;
    }

    let previous = path.with_file_name(PREVIOUS_LOG_FILE_NAME);
    let _ = fs::remove_file(&previous);
    if let Err(err) = fs::rename(path, previous) {
        eprintln!("Slackinux could not rotate its diagnostic log: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quoted_distribution_name() {
        assert_eq!(
            parse_pretty_name("NAME=Example\nPRETTY_NAME=\"Example Linux 1\"\n"),
            Some("Example Linux 1".into())
        );
    }

    #[test]
    fn issue_url_contains_report_and_privacy_reminder() {
        let url = issue_url("Slackinux diagnostics\n- Version: 1.2.3\n");
        let query = url.query().unwrap();
        assert!(query.contains("Slackinux+diagnostics"));
        assert!(query.contains("Please+review+the+log"));
    }
}
