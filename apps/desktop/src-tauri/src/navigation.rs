use url::Url;

#[derive(Debug, Clone, PartialEq)]
pub enum NavigationDecision {
    AllowInternal,
    OpenExternally,
    Deny,
}

pub fn classify_url(url: &Url) -> NavigationDecision {
    if url.scheme() == "tauri" {
        return NavigationDecision::AllowInternal;
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

    if host_lower == "localhost" || host_lower == "tauri.localhost" {
        return NavigationDecision::AllowInternal;
    }

    if host_lower == "app.slack.com"
        || host_lower.ends_with(".slack.com")
        || host_lower == "slack.com"
        || host_lower == "www.slack.com"
    {
        return NavigationDecision::AllowInternal;
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
    fn allows_localhost_dev() {
        let url = Url::parse("http://localhost:1420/bootstrap/index.html").unwrap();
        assert_eq!(classify_url(&url), NavigationDecision::AllowInternal);
    }
}
