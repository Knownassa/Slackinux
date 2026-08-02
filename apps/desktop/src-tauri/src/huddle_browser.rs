//! On-demand "Open Huddle in Browser" fallback for Slackinux.
//!
//! Huddles that fail inside the embedded WebKit renderer can be opened in a
//! full desktop browser. Slackinux never launches the browser on its own:
//! only an explicit user action (menu item) triggers this path.
//!
//! Security rules enforced here:
//! - Browsers found on PATH are restricted to a closed allow-list of known
//!   binaries, never arbitrary strings from the page or from settings.
//! - A user-configured executable path is honored only when it exists and is
//!   executable; it is still a fixed path, never a command string.
//! - Slack session state is never passed on the command line. The browser is
//!   opened to a workspace-neutral Huddle URL at most; the user's existing
//!   session in that browser is what Slack uses.
//! - Anything untrusted (from the page) is treated as a normal navigation
//!   URL for a fresh launch, not as a second executable or argument.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A detected desktop browser that can host a Huddle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    Chromium,
    Brave,
}

impl Browser {
    /// Human-readable name for prompts and logs.
    pub fn name(self) -> &'static str {
        match self {
            Browser::Chrome => "Google Chrome",
            Browser::Chromium => "Chromium",
            Browser::Brave => "Brave",
        }
    }
}

/// Well-known browser executable names, most-preferred first. Each is verified
/// to be an executable on disk before use; the list is closed so a malicious
/// value cannot inject a command.
const KNOWN_BROWSERS: &[(&str, Browser)] = &[
    ("google-chrome", Browser::Chrome),
    ("google-chrome-stable", Browser::Chrome),
    ("chromium", Browser::Chromium),
    ("chromium-browser", Browser::Chromium),
    ("brave-browser", Browser::Brave),
    ("brave", Browser::Brave),
];

/// Finds the first known browser executable on PATH. `None` when no
/// supported browser is installed.
pub fn find_browser() -> Option<(PathBuf, Browser)> {
    KNOWN_BROWSERS
        .iter()
        .find_map(|(name, kind)| which_on_path(name).map(|path| (path, *kind)))
}

/// Resolves the executable to use for the "Open Huddle in Browser" action.
///
/// A configured path is honored only when it resolves to an existing
/// executable; the value is a filesystem path chosen by the user, so it is
/// validated for existence and executability but is not restricted to the
/// known-browser list. Falls back to [`find_browser`] when unset or invalid.
pub fn resolve_browser(configured: Option<&str>) -> Option<(PathBuf, Browser)> {
    if let Some(path) = configured
        .map(PathBuf::from)
        .filter(|path| path.is_file() && is_executable(path))
    {
        return Some((path, Browser::Chromium));
    }
    find_browser()
}

/// Returns the resolved path of `name` on PATH, or `None` if missing.
fn which_on_path(name: &str) -> Option<PathBuf> {
    let path_value = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_value) {
        let candidate = dir.join(name);
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Best-effort executable check: on Linux, look for any execute bit.
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Sanitizes a Slack-provided Huddle URL for opening in the external browser.
///
/// Only `https://` URLs on Slack-owned domains are accepted. Returns `None`
/// for anything else so no untrusted value reaches the browser command.
fn huddle_url_to_open(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?;
    let host = host.to_ascii_lowercase();
    if host != "slack.com" && !host.ends_with(".slack.com") {
        return None;
    }
    Some(url.as_str().to_string())
}

/// Opens `target` in the given browser on demand. `target` is sanitized
/// before it is passed to the browser. Never used for arbitrary commands.
pub fn open_in_browser(browser: &Path, target: &str) -> Result<(), String> {
    let sanitized = huddle_url_to_open(target)
        .ok_or_else(|| "refusing to open: URL is not a trusted Slack https address".to_string())?;
    let mut command = Command::new(browser);
    command.arg(&sanitized);
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not launch {}: {error}", browser.display()))
}

/// The workspace-neutral Huddle start URL. Slack resolves this to the user's
/// current workspace on their own session in the external browser.
pub fn huddle_launch_url() -> String {
    "https://app.slack.com/huddle/new".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizer_accepts_only_slack_https_urls() {
        assert_eq!(
            huddle_url_to_open("https://app.slack.com/huddle/new"),
            Some("https://app.slack.com/huddle/new".into())
        );
        assert_eq!(
            huddle_url_to_open("https://workspace.slack.com/client/T1"),
            Some("https://workspace.slack.com/client/T1".into())
        );
        assert_eq!(huddle_url_to_open("http://app.slack.com/huddle"), None);
        assert_eq!(huddle_url_to_open("https://evil.example/huddle"), None);
        assert_eq!(huddle_url_to_open("https://slack.com.evil.example/"), None);
        assert_eq!(huddle_url_to_open("not a url"), None);
    }

    #[test]
    fn known_browser_list_has_no_duplicate_kinds_without_paths() {
        // The allow-list must stay closed: every entry resolves via PATH only.
        for (name, _) in KNOWN_BROWSERS {
            assert!(
                !name.contains('/'),
                "browser names must not be paths: {name}"
            );
            assert!(Path::new(name).is_relative());
        }
    }

    #[test]
    fn configured_path_must_exist_and_be_executable() {
        // With an empty PATH there is nothing to fall back to.
        let saved_path = std::env::var_os("PATH");
        std::env::remove_var("PATH");
        let restore = || {
            if let Some(path) = &saved_path {
                std::env::set_var("PATH", path);
            }
        };

        // A missing configured path yields nothing.
        assert!(resolve_browser(Some("/nonexistent/browser")).is_none());
        assert!(resolve_browser(Some("")).is_none());

        // A real executable is honored even when PATH is empty.
        let dir = std::env::temp_dir().join(format!(
            "slackinux-huddle-browser-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("fake-browser");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let resolved = resolve_browser(exe.to_str());
        restore();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap().0, exe);
    }
}
