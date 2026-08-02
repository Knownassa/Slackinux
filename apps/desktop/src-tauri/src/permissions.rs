//! Native permission broker for Slackinux.
//!
//! WebKitGTK forwards camera/microphone/screen-capture and notification
//! permission requests to the host. Slackinux must never approve these
//! automatically. This module owns the four-way decision model
//! (`AskEveryTime`, `AllowOnce`, `AlwaysAllow`, `Block`), persists decisions
//! per permission kind, and applies a strict trusted-origin rule so that no
//! arbitrary SSO provider or external website ever receives media access.
//!
//! Storage: a single `permissions.json` in the app data directory. The file
//! is deliberately small and contains only permission decisions keyed by kind
//! and host — never media content, messages, cookies, or tokens.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The permission categories Slackinux brokers. Notifications are brokered
/// separately from the media kinds so a user can mute only Slackinux without
/// changing Slack's web-side notification permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Microphone,
    Camera,
    ScreenShare,
    Notifications,
}

impl MediaKind {
    /// User-facing label used in prompts and diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            MediaKind::Microphone => "microphone",
            MediaKind::Camera => "camera",
            MediaKind::ScreenShare => "screen sharing",
            MediaKind::Notifications => "notifications",
        }
    }
}

/// The four-way decision model required by the phase specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    /// Prompt the user every time this kind is requested.
    #[default]
    AskEveryTime,
    /// Allow this one request, then forget the decision.
    AllowOnce,
    /// Remember the allowance for the trusted origin.
    AlwaysAllow,
    /// Refuse this kind; the denial persists until reset.
    Block,
}

/// A stored decision plus the wall-clock time it was recorded, so
/// `AllowOnce` entries can expire.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredDecision {
    decision: PermissionDecision,
    recorded_unix: u64,
}

/// How long an `AllowOnce` decision stays valid before the user is asked
/// again.
const ALLOW_ONCE_LIFETIME: Duration = Duration::from_secs(5 * 60);

/// Parsed, lower-cased host names that are allowed to receive Slack media
/// permissions. Only the registrable `slack.com` apex and its subdomains.
fn is_trusted_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "slack.com" || host.ends_with(".slack.com")
}

/// A thread-safe permission store shared by the renderer and the UI.
pub struct PermissionBroker {
    inner: std::sync::Mutex<PermissionStore>,
    data_dir: PathBuf,
}

#[derive(Default, Serialize, Deserialize)]
struct PermissionStore {
    #[serde(default, flatten)]
    by_kind: BTreeMap<MediaKind, BTreeMap<String, StoredDecision>>,
}

impl PermissionBroker {
    /// Loads decisions from `<data_dir>/permissions.json`. A missing or
    /// corrupt file yields an empty broker — never a crash.
    pub fn load(data_dir: &Path) -> Self {
        let store = std::fs::read_to_string(data_dir.join("permissions.json"))
            .ok()
            .and_then(|content| serde_json::from_str::<PermissionStore>(&content).ok())
            .unwrap_or_default();
        Self {
            inner: std::sync::Mutex::new(store),
            data_dir: data_dir.to_path_buf(),
        }
    }

    /// Writes decisions to `<data_dir>/permissions.json` atomically.
    fn save(&self) {
        let content = match self
            .inner
            .lock()
            .map(|store| serde_json::to_string_pretty(&*store))
        {
            Ok(Ok(json)) => json,
            _ => return,
        };
        let path = self.data_dir.join("permissions.json");
        let temporary = self.data_dir.join("permissions.json.tmp");
        if std::fs::write(&temporary, content)
            .and_then(|_| std::fs::rename(&temporary, &path))
            .is_err()
        {
            log::warn!("could not save permission decisions atomically");
            let _ = std::fs::remove_file(temporary);
        }
    }

    /// The decision that should govern a new request for `kind` from `host`.
    ///
    /// Unknown hosts are always denied, so the caller never has to guess.
    /// `AllowOnce` entries are consumed here: the caller grants exactly one
    /// request, and the stored entry is removed or replaced by `AskEveryTime`
    /// so the next request re-prompts.
    pub fn decide(&self, kind: MediaKind, host: &str) -> PermissionDecision {
        if !is_trusted_host(host) {
            return PermissionDecision::Block;
        }
        let now = now_unix();
        let mut store = match self.inner.lock() {
            Ok(store) => store,
            Err(_) => return PermissionDecision::Block,
        };
        let Some(entry) = store
            .by_kind
            .get_mut(&kind)
            .and_then(|by_host| by_host.get_mut(&host.to_ascii_lowercase()))
        else {
            return PermissionDecision::AskEveryTime;
        };
        match entry.decision {
            PermissionDecision::AllowOnce => {
                let fresh = entry
                    .recorded_unix
                    .checked_add(ALLOW_ONCE_LIFETIME.as_secs())
                    .is_some_and(|expiry| now < expiry);
                if !fresh {
                    // Expired: reset to the default and re-prompt.
                    entry.decision = PermissionDecision::AskEveryTime;
                    return PermissionDecision::AskEveryTime;
                }
                // Consume the allowance: the next request must ask again.
                entry.decision = PermissionDecision::AskEveryTime;
                PermissionDecision::AllowOnce
            }
            other => other,
        }
    }

    /// Records a decision for `kind` at `host`. Blocking an unknown host is
    /// redundant but harmless; asking is the default and is not persisted
    /// unless explicitly chosen, to keep the file minimal.
    pub fn record(&self, kind: MediaKind, host: &str, decision: PermissionDecision) {
        if !is_trusted_host(host) {
            return;
        }
        let host = host.to_ascii_lowercase();
        let now = now_unix();
        {
            let mut store = match self.inner.lock() {
                Ok(store) => store,
                Err(_) => return,
            };
            let by_host = store.by_kind.entry(kind).or_default();
            match decision {
                // Asking is the default; drop the entry so the file stays clean
                // and a later reset has nothing to walk.
                PermissionDecision::AskEveryTime => {
                    by_host.remove(&host);
                    if by_host.is_empty() {
                        store.by_kind.remove(&kind);
                    }
                }
                other => {
                    by_host.insert(
                        host,
                        StoredDecision {
                            decision: other,
                            recorded_unix: now,
                        },
                    );
                }
            }
        }
        self.save();
    }

    /// Drops every stored decision. Used by the "reset permissions" action.
    pub fn reset_all(&self) {
        if let Ok(mut store) = self.inner.lock() {
            store.by_kind.clear();
        }
        self.save();
    }

    /// The set of hosts with a non-default stored decision for `kind`.
    pub fn managed_hosts(&self, kind: MediaKind) -> Vec<String> {
        self.inner
            .lock()
            .map(|store| {
                store
                    .by_kind
                    .get(&kind)
                    .map(|by_host| by_host.keys().cloned().collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Shows a synchronous GTK prompt asking how the user wants to handle a media
/// or notification request from a trusted Slack origin. Runs a nested GTK main
/// loop (`gtk::Dialog::run`), which is the standard WebKitGTK permission-dialog
/// pattern, so it is safe to call from inside a permission-request handler.
///
/// `parent` should be the top-level `gtk::Window` so the dialog is modal to
/// the application window.
pub fn prompt_user(
    kind: MediaKind,
    host: &str,
    parent: Option<&gtk::Window>,
) -> PermissionDecision {
    use gtk::prelude::*;
    use gtk::ResponseType;

    let dialog = gtk::Dialog::new();
    dialog.set_title("Slackinux — Permission Request");
    dialog.set_modal(true);
    if let Some(parent) = parent {
        dialog.set_transient_for(Some(parent));
    }

    let label = gtk::Label::new(Some(&format!(
        "Slackinux is requesting access to your {}.\n\nOrigin: {}\n\nWhat should Slackinux do?",
        kind.label(),
        host
    )));
    label.set_line_wrap(true);
    dialog.content_area().add(&label);
    dialog.content_area().set_spacing(12);

    dialog.add_button("Ask Every Time", ResponseType::Cancel);
    dialog.add_button("Allow Once", ResponseType::Yes);
    dialog.add_button("Always Allow", ResponseType::Apply);
    dialog.add_button("Block", ResponseType::No);

    dialog.show_all();
    let response = dialog.run();
    dialog.close();

    match response {
        ResponseType::Yes => PermissionDecision::AllowOnce,
        ResponseType::Apply => PermissionDecision::AlwaysAllow,
        ResponseType::No => PermissionDecision::Block,
        _ => PermissionDecision::AskEveryTime,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("slackinux_permissions_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn trusted_hosts_are_slack_only() {
        assert!(is_trusted_host("slack.com"));
        assert!(is_trusted_host("app.slack.com"));
        assert!(is_trusted_host("sub.app.slack.com"));
        assert!(is_trusted_host("SLACK.COM"));
        assert!(!is_trusted_host("notslack.com"));
        assert!(!is_trusted_host("slack.com.evil.example"));
        assert!(!is_trusted_host("slackexample.com"));
        assert!(!is_trusted_host("accounts.google.com"));
        assert!(!is_trusted_host("localhost"));
    }

    #[test]
    fn unknown_host_never_gains_access() {
        let dir = tmp_dir("unknown_host");
        let broker = PermissionBroker::load(&dir);
        assert_eq!(
            broker.decide(MediaKind::Camera, "evil.example"),
            PermissionDecision::Block
        );
        // Recording for an unknown host is a no-op.
        broker.record(
            MediaKind::Camera,
            "evil.example",
            PermissionDecision::AlwaysAllow,
        );
        assert_eq!(
            broker.decide(MediaKind::Camera, "evil.example"),
            PermissionDecision::Block
        );
    }

    #[test]
    fn default_is_ask_every_time() {
        let dir = tmp_dir("default_ask");
        let broker = PermissionBroker::load(&dir);
        assert_eq!(
            broker.decide(MediaKind::Microphone, "app.slack.com"),
            PermissionDecision::AskEveryTime
        );
        assert_eq!(
            broker.decide(MediaKind::ScreenShare, "app.slack.com"),
            PermissionDecision::AskEveryTime
        );
        assert_eq!(
            broker.decide(MediaKind::Notifications, "app.slack.com"),
            PermissionDecision::AskEveryTime
        );
    }

    #[test]
    fn always_allow_and_block_persist() {
        let dir = tmp_dir("persist");
        let broker = PermissionBroker::load(&dir);
        broker.record(
            MediaKind::Camera,
            "app.slack.com",
            PermissionDecision::AlwaysAllow,
        );
        broker.record(
            MediaKind::Microphone,
            "app.slack.com",
            PermissionDecision::Block,
        );

        let reloaded = PermissionBroker::load(&dir);
        assert_eq!(
            reloaded.decide(MediaKind::Camera, "app.slack.com"),
            PermissionDecision::AlwaysAllow
        );
        assert_eq!(
            reloaded.decide(MediaKind::Microphone, "app.slack.com"),
            PermissionDecision::Block
        );
    }

    #[test]
    fn allow_once_is_consumed_after_one_request() {
        let dir = tmp_dir("allow_once");
        let broker = PermissionBroker::load(&dir);
        broker.record(
            MediaKind::Camera,
            "app.slack.com",
            PermissionDecision::AllowOnce,
        );
        assert_eq!(
            broker.decide(MediaKind::Camera, "app.slack.com"),
            PermissionDecision::AllowOnce
        );
        // Second request: the allowance is gone and the user must be asked.
        assert_eq!(
            broker.decide(MediaKind::Camera, "app.slack.com"),
            PermissionDecision::AskEveryTime
        );
    }

    #[test]
    fn expired_allow_once_is_treated_as_ask() {
        let dir = tmp_dir("expired");
        let broker = PermissionBroker::load(&dir);
        // Fabricate an entry recorded long ago.
        let stale = StoredDecision {
            decision: PermissionDecision::AllowOnce,
            recorded_unix: 1,
        };
        broker
            .inner
            .lock()
            .unwrap()
            .by_kind
            .entry(MediaKind::Camera)
            .or_default()
            .insert("app.slack.com".into(), stale);
        assert_eq!(
            broker.decide(MediaKind::Camera, "app.slack.com"),
            PermissionDecision::AskEveryTime
        );
    }

    #[test]
    fn kinds_are_stored_separately() {
        let dir = tmp_dir("kinds");
        let broker = PermissionBroker::load(&dir);
        broker.record(
            MediaKind::Camera,
            "app.slack.com",
            PermissionDecision::AlwaysAllow,
        );
        broker.record(
            MediaKind::ScreenShare,
            "app.slack.com",
            PermissionDecision::Block,
        );
        assert_eq!(
            broker.decide(MediaKind::Camera, "app.slack.com"),
            PermissionDecision::AlwaysAllow
        );
        assert_eq!(
            broker.decide(MediaKind::ScreenShare, "app.slack.com"),
            PermissionDecision::Block
        );
        assert_eq!(
            broker.decide(MediaKind::Microphone, "app.slack.com"),
            PermissionDecision::AskEveryTime
        );
    }

    #[test]
    fn reset_all_clears_every_decision() {
        let dir = tmp_dir("reset");
        let broker = PermissionBroker::load(&dir);
        broker.record(
            MediaKind::Camera,
            "app.slack.com",
            PermissionDecision::AlwaysAllow,
        );
        broker.record(
            MediaKind::Microphone,
            "app.slack.com",
            PermissionDecision::Block,
        );
        broker.reset_all();
        let reloaded = PermissionBroker::load(&dir);
        assert_eq!(
            reloaded.decide(MediaKind::Camera, "app.slack.com"),
            PermissionDecision::AskEveryTime
        );
        assert_eq!(
            reloaded.decide(MediaKind::Microphone, "app.slack.com"),
            PermissionDecision::AskEveryTime
        );
    }

    #[test]
    fn corrupted_file_falls_back_to_defaults() {
        let dir = tmp_dir("corrupt");
        std::fs::write(dir.join("permissions.json"), "not json").unwrap();
        let broker = PermissionBroker::load(&dir);
        assert_eq!(
            broker.decide(MediaKind::Camera, "app.slack.com"),
            PermissionDecision::AskEveryTime
        );
    }

    #[test]
    fn managed_hosts_lists_only_stored_entries() {
        let dir = tmp_dir("managed");
        let broker = PermissionBroker::load(&dir);
        broker.record(
            MediaKind::Camera,
            "app.slack.com",
            PermissionDecision::AlwaysAllow,
        );
        broker.record(
            MediaKind::Camera,
            "workspace.slack.com",
            PermissionDecision::Block,
        );
        let hosts = broker.managed_hosts(MediaKind::Camera);
        assert_eq!(hosts.len(), 2);
        assert!(hosts.contains(&"app.slack.com".to_string()));
        assert!(broker.managed_hosts(MediaKind::Microphone).is_empty());
    }

    #[test]
    fn empty_host_set_is_dropped_after_ask() {
        let dir = tmp_dir("ask_drops");
        let broker = PermissionBroker::load(&dir);
        broker.record(
            MediaKind::Camera,
            "app.slack.com",
            PermissionDecision::AlwaysAllow,
        );
        broker.record(
            MediaKind::Camera,
            "app.slack.com",
            PermissionDecision::AskEveryTime,
        );
        assert!(broker.managed_hosts(MediaKind::Camera).is_empty());
    }

    #[test]
    fn set_order_is_deterministic() {
        let mut set = std::collections::BTreeSet::new();
        set.insert("b.slack.com");
        set.insert("a.slack.com");
        assert_eq!(
            set.into_iter().collect::<Vec<_>>(),
            vec!["a.slack.com", "b.slack.com"]
        );
    }
}
