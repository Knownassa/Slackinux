use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub enum NavigationDecision {
    AllowInternal,
    OpenExternally,
    Deny,
}

/// True when `host` is owned by Slack (app.slack.com, any *.slack.com
/// workspace host, or the root/`www` marketing hosts). Used to scope behavior
/// such as the Slack-masked user agent to Slack's own pages only.
pub fn is_slack_owned_host(host: &str) -> bool {
    let host_lower = host.to_lowercase();
    host_lower == "app.slack.com"
        || host_lower.ends_with(".slack.com")
        || host_lower == "slack.com"
        || host_lower == "www.slack.com"
}

/// The desktop Chrome user agent Slackinux reports when navigating to
/// Slack-owned pages. Slack's Huddle client gate only admits desktop Chrome
/// (compatibility/manifest.json: minimumChromeMajor), so WebKitGTK's truthful
/// UA would be rejected outright even when the media stack is fully ready.
///
/// The version tracks `minimumChromeMajor` from the compatibility manifest.
/// Keep the two in sync; do not bump past a released Chrome major without a
/// matching manifest update, and never apply this mask to third-party hosts.
pub fn slack_masked_user_agent() -> String {
    // Firefox-parity reasoning: Slack admitted Firefox after it shipped its own
    // UA workaround; masking WebKitGTK as desktop Chrome is the same class of
    // fix. Chrome 151 is the current stable major (Aug 2026), matching the
    // compatibility manifest. Slack's browser lifecycle drops Chrome <= 142 on
    // November 9, 2026, so keep this at or above current stable and bump both
    // together as Slack's floor moves.
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/151.0.0.0 Safari/537.36"
        .to_string()
}

pub fn classify_url(url: &Url) -> NavigationDecision {
    if url.scheme() == "tauri" {
        return if matches!(url.host_str(), Some("localhost" | "tauri.localhost")) {
            NavigationDecision::AllowInternal
        } else {
            NavigationDecision::Deny
        };
    }

    let host = match url.host_str() {
        Some(h) => h,
        None => {
            match url.scheme() {
                "file" | "javascript" | "data" => return NavigationDecision::Deny,
                "mailto" | "tel" => return NavigationDecision::OpenExternally,
                _ => {}
            }
            return NavigationDecision::Deny;
        }
    };

    let host_lower = host.to_lowercase();

    if matches!(url.scheme(), "http" | "https")
        && (host_lower == "localhost" || host_lower == "tauri.localhost")
    {
        return NavigationDecision::AllowInternal;
    }

    if is_slack_owned_host(host) {
        return if url.scheme() == "https" {
            NavigationDecision::AllowInternal
        } else {
            NavigationDecision::Deny
        };
    }

    match url.scheme() {
        "mailto" | "tel" => NavigationDecision::OpenExternally,
        "http" | "https" => NavigationDecision::OpenExternally,
        "file" | "javascript" | "data" => NavigationDecision::Deny,
        _ => NavigationDecision::Deny,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_app_slack_com() {
        let url = Url::parse("https://app.slack.com/client/T00/B00").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::AllowInternal);
    }

    #[test]
    fn slack_host_helper_matches_only_slack_owned_hosts() {
        assert!(is_slack_owned_host("app.slack.com"));
        assert!(is_slack_owned_host("myworkspace.slack.com"));
        assert!(is_slack_owned_host("slack.com"));
        assert!(is_slack_owned_host("www.slack.com"));
        assert!(!is_slack_owned_host("evilslack.com"));
        assert!(!is_slack_owned_host("slack.com.evil.example"));
        assert!(!is_slack_owned_host("accounts.google.com"));
        assert!(!is_slack_owned_host("example.com"));
    }

    #[test]
    fn slack_ua_mask_reports_desktop_chrome() {
        let ua = slack_masked_user_agent();
        assert!(
            ua.contains("Chrome/151."),
            "UA must report current stable desktop Chrome (151): {ua}"
        );
        assert!(
            ua.contains("X11; Linux"),
            "UA must look like a Linux desktop browser: {ua}"
        );
        assert!(ua.contains("AppleWebKit/537.36"));
    }

    #[test]
    fn allows_slack_subdomains() {
        let url = Url::parse("https://myworkspace.slack.com/").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::AllowInternal);
    }

    #[test]
    fn allows_slack_com() {
        let url = Url::parse("https://slack.com/signin").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::AllowInternal);
    }

    #[test]
    fn allows_www_slack_com() {
        let url = Url::parse("https://www.slack.com/").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::AllowInternal);
    }

    #[test]
    fn opens_external_http() {
        let url = Url::parse("https://github.com").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::OpenExternally);
    }

    #[test]
    fn denies_file_scheme() {
        let url = Url::parse("file:///etc/passwd").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::Deny);
    }

    #[test]
    fn denies_javascript_scheme() {
        let url = Url::parse("javascript:alert(1)").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::Deny);
    }

    #[test]
    fn denies_data_scheme() {
        let url = Url::parse("data:text/html,<script>alert(1)</script>").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::Deny);
    }

    #[test]
    fn opens_mailto_externally() {
        let url = Url::parse("mailto:user@example.com").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::OpenExternally);
    }

    #[test]
    fn opens_tel_externally() {
        let url = Url::parse("tel:+1234567890").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::OpenExternally);
    }

    #[test]
    fn denies_no_host_urls() {
        let url = Url::parse("about:blank").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::Deny);
    }

    #[test]
    fn allows_case_insensitive_slack() {
        let url = Url::parse("https://APP.SLACK.COM/client").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::AllowInternal);
    }

    #[test]
    fn allows_tauri_protocol() {
        let url = Url::parse("tauri://localhost/bootstrap/index.html").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::AllowInternal);
    }

    #[test]
    fn denies_untrusted_tauri_protocol_host() {
        let url = Url::parse("tauri://evil.example/bootstrap/index.html").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::Deny);
    }

    #[test]
    fn denies_insecure_slack_origin() {
        let url = Url::parse("http://app.slack.com/client").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::Deny);
    }

    #[test]
    fn allows_localhost_dev() {
        let url = Url::parse("http://localhost:1420/bootstrap/index.html").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::AllowInternal);
    }
}
