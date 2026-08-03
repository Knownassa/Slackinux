use url::Url;

#[cfg(target_os = "linux")]
pub fn ensure_linux_handler() -> Result<(), String> {
    use std::fmt::Write as _;

    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/share"))
        })
        .ok_or_else(|| "HOME and XDG_DATA_HOME are unavailable".to_string())?;
    let applications = data_home.join("applications");
    std::fs::create_dir_all(&applications).map_err(|error| error.to_string())?;
    let icon_directory = data_home.join("icons/hicolor/512x512/apps");
    std::fs::create_dir_all(&icon_directory).map_err(|error| error.to_string())?;
    write_atomic(
        &icon_directory.join("slackinux.png"),
        include_bytes!("../icons/512x512.png"),
    )?;

    let executable = std::env::var_os("APPIMAGE")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .ok_or_else(|| "could not determine the Slackinux executable".to_string())?;
    let escaped = executable
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let mut desktop = String::new();
    writeln!(desktop, "[Desktop Entry]").unwrap();
    writeln!(desktop, "Type=Application").unwrap();
    writeln!(desktop, "Name=Slackinux").unwrap();
    writeln!(
        desktop,
        "Comment=An unofficial Linux desktop shell for Slack Web"
    )
    .unwrap();
    writeln!(desktop, "Exec=\"{escaped}\" %U").unwrap();
    writeln!(desktop, "Icon=slackinux").unwrap();
    writeln!(desktop, "Categories=Network;InstantMessaging;").unwrap();
    writeln!(desktop, "MimeType=x-scheme-handler/slack;").unwrap();
    writeln!(desktop, "Terminal=false").unwrap();
    writeln!(desktop, "StartupNotify=true").unwrap();

    // Keep a single stable handler name. Registering the process discovered by
    // Tauri is unsafe for the AppImage host-runtime path because current_exe()
    // can be the dynamic loader used for re-execution rather than Slackinux.
    let handler_name = "com.slackinux.desktop";
    let handler = applications.join(handler_name);
    write_atomic(&handler, desktop.as_bytes())?;

    // Clean up exact handler names produced by older releases. They otherwise
    // remain candidates in browser "Open with" dialogs indefinitely.
    for legacy in [
        "slackinux-handler.desktop",
        "ld-linux-x86-64.so.2-handler.desktop",
        "ld-linux.so.2-handler.desktop",
    ] {
        let _ = std::fs::remove_file(applications.join(legacy));
    }
    if let Ok(entries) = std::fs::read_dir(&applications) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("slackinux_") && name.ends_with("_amd64.desktop") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    let _ = std::process::Command::new("update-desktop-database")
        .arg(&applications)
        .status();
    let _ = std::process::Command::new("gtk-update-icon-cache")
        .args(["-f", "-t"])
        .arg(data_home.join("icons/hicolor"))
        .status();

    let status = std::process::Command::new("xdg-mime")
        .args(["default", handler_name, "x-scheme-handler/slack"])
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        _ => {
            let gio = std::process::Command::new("gio")
                .args(["mime", "x-scheme-handler/slack", handler_name])
                .status();
            match gio {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => Err(format!("could not set the Slack handler (gio: {status})")),
                Err(error) => Err(format!("could not set the Slack handler: {error}")),
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn write_atomic(path: &std::path::Path, content: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension("desktop.tmp");
    std::fs::write(&temporary, content).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

/// Finds the first ordinary Slack deep link passed by a desktop launcher or a
/// second process and converts it to a safe Slack Web destination.
pub fn slack_url_from_args(args: &[String]) -> Option<Url> {
    args.iter().find_map(|arg| slack_deep_link_to_web(arg))
}

pub fn slack_deep_link_to_web(value: &str) -> Option<Url> {
    let link = Url::parse(value).ok()?;
    if link.scheme() != "slack" {
        return None;
    }

    // One-time authentication callbacks are not ordinary workspace links.
    // Slackinux deliberately does not consume or redeem them; authentication
    // happens inside the isolated webview.
    if link
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("login-v2"))
        || link.path().contains("/magic-login/")
    {
        return None;
    }

    let parameter = |name: &str| {
        link.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };

    // Slack sign-in callbacks sometimes include an HTTPS or relative return
    // path. Only accept destinations on Slack itself.
    for name in ["redir", "return_to"] {
        if let Some(value) = parameter(name) {
            if let Some(url) = safe_slack_redirect(&value) {
                return Some(url);
            }
        }
    }

    let team = parameter("team").filter(|value| valid_id(value, &['T', 'E']));
    let target = parameter("id").filter(|value| valid_id(value, &['C', 'D', 'G', 'U', 'A', 'F']));
    let action = link.host_str().unwrap_or("open");

    let destination = match (action, team.as_deref(), target.as_deref()) {
        ("channel" | "user", Some(team), Some(target)) => {
            format!("https://app.slack.com/client/{team}/{target}")
        }
        (_, Some(team), _) => format!("https://app.slack.com/client/{team}"),
        _ => "https://app.slack.com/client".to_string(),
    };
    Url::parse(&destination).ok()
}

pub fn redact_sensitive_url(value: &str) -> String {
    let Ok(mut url) = Url::parse(value) else {
        return value.to_string();
    };
    let mut sensitive = false;
    for (key, _) in url.query_pairs() {
        let key = key.to_ascii_lowercase();
        if key.contains("token")
            || key == "code"
            || key.ends_with("_code")
            || key.contains("secret")
        {
            sensitive = true;
        }
    }
    if sensitive {
        url.set_query(Some("redacted"));
    }
    // OAuth/SSO flows may deliver tokens in the fragment (#access_token=...);
    // never leave them in a logged URL.
    if let Some(fragment) = url.fragment() {
        let fragment_sensitive =
            url::form_urlencoded::parse(fragment.as_bytes()).any(|(key, _)| {
                let key = key.to_ascii_lowercase();
                key.contains("token")
                    || key == "code"
                    || key.ends_with("_code")
                    || key.contains("secret")
                    || key.contains("state")
            });
        if fragment_sensitive {
            url.set_fragment(Some("redacted"));
        }
    }
    url.into()
}

fn safe_slack_redirect(value: &str) -> Option<Url> {
    let url = if value.starts_with('/') {
        Url::parse("https://app.slack.com").ok()?.join(value).ok()?
    } else {
        Url::parse(value).ok()?
    };
    let host = url.host_str()?.to_ascii_lowercase();
    if url.scheme() == "https"
        && (host == "slack.com" || host == "app.slack.com" || host.ends_with(".slack.com"))
    {
        Some(url)
    } else {
        None
    }
}

fn valid_id(value: &str, prefixes: &[char]) -> bool {
    value.len() >= 2
        && value.len() <= 32
        && value.chars().next().is_some_and(|c| prefixes.contains(&c))
        && value.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_open_workspace_link() {
        let url = slack_deep_link_to_web("slack://open?team=T12345").unwrap();
        assert_eq!(url.as_str(), "https://app.slack.com/client/T12345");
    }

    #[test]
    fn converts_channel_link() {
        let url = slack_deep_link_to_web("slack://channel?team=T123&id=C456").unwrap();
        assert_eq!(url.as_str(), "https://app.slack.com/client/T123/C456");
    }

    #[test]
    fn accepts_safe_signin_redirect() {
        let url = slack_deep_link_to_web(
            "slack://ssb/signin_redirect?redir=https%3A%2F%2Fapp.slack.com%2Fclient%2FT123",
        )
        .unwrap();
        assert_eq!(url.as_str(), "https://app.slack.com/client/T123");
    }

    #[test]
    fn rejects_non_slack_redirect() {
        let url = slack_deep_link_to_web(
            "slack://ssb/signin_redirect?redir=https%3A%2F%2Fevil.example%2Fsteal",
        )
        .unwrap();
        assert_eq!(url.as_str(), "https://app.slack.com/client");
    }

    #[test]
    fn ignores_unrelated_arguments() {
        assert!(slack_deep_link_to_web("--help").is_none());
        assert!(slack_deep_link_to_web("https://example.com").is_none());
    }

    #[test]
    fn ignores_private_authentication_callbacks() {
        assert!(slack_deep_link_to_web(
            "slack://open/T12345678/magic-login/AbCdEf123456?host=slack.com"
        )
        .is_none());
        assert!(slack_deep_link_to_web(
            "slack://login-v2?0.host=slack.com&0.tokens=z-app-T12345678-AbCdEf123456"
        )
        .is_none());
    }

    #[test]
    fn redacts_sso_codes_from_logs() {
        assert_eq!(
            redact_sensitive_url("https://idp.example/callback?code=secret-value&state=ok"),
            "https://idp.example/callback?redacted"
        );
    }

    #[test]
    fn redacts_fragment_tokens_from_logs() {
        assert_eq!(
            redact_sensitive_url(
                "https://app.slack.com/signin#access_token=xoxc-frag&state=AbC123"
            ),
            "https://app.slack.com/signin#redacted"
        );
    }

    #[test]
    fn leaves_plain_urls_unchanged() {
        assert_eq!(
            redact_sensitive_url("https://slack.com/workspaces"),
            "https://slack.com/workspaces"
        );
    }
}
