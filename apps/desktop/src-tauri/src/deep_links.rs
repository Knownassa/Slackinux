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
    writeln!(desktop, "Name=Slackinux URL Handler").unwrap();
    writeln!(desktop, "NoDisplay=true").unwrap();
    writeln!(desktop, "Exec=\"{escaped}\" %u").unwrap();
    writeln!(desktop, "MimeType=x-scheme-handler/slack;").unwrap();
    writeln!(desktop, "Terminal=false").unwrap();

    let handler = applications.join("slackinux-handler.desktop");
    write_atomic(&handler, desktop.as_bytes())?;

    let status = std::process::Command::new("xdg-mime")
        .args([
            "default",
            "slackinux-handler.desktop",
            "x-scheme-handler/slack",
        ])
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("xdg-mime exited with {status}")),
        Err(error) => Err(format!("could not run xdg-mime: {error}")),
    }
}

#[cfg(target_os = "linux")]
fn write_atomic(path: &std::path::Path, content: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension("desktop.tmp");
    std::fs::write(&temporary, content).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())
}

/// Finds the first Slack callback passed by a desktop launcher or a second
/// process and converts it to a safe Slack Web destination.
pub fn slack_url_from_args(args: &[String]) -> Option<Url> {
    args.iter().find_map(|arg| slack_deep_link_to_web(arg))
}

pub fn slack_deep_link_to_web(value: &str) -> Option<Url> {
    let link = Url::parse(value).ok()?;
    if link.scheme() != "slack" {
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
}
