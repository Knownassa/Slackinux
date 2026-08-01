use url::Url;

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
