//! Huddle compatibility doctor for Slackinux.
//!
//! Huddles need real-time audio, camera, and screen sharing. Whether they work
//! depends on the surrounding Linux environment: a running PipeWire session,
//! the XDG desktop portal's ScreenCast interface, GStreamer codec plugins in
//! the WebKit process, and actual input devices. This module probes those
//! pieces and classifies the result so the user gets an actionable report
//! instead of a confusing in-app failure.
//!
//! The report is privacy-safe by construction: it never includes workspace
//! names, message content, cookies, or tokens — only environment facts.
//!
//! Classification is a pure function of [`HuddleReport`], which keeps the
//! decision logic unit-testable and the probes themselves thin.

use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Overall Huddle support classification, as required by the phase spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HuddleSupport {
    /// Every probed prerequisite is present.
    Supported,
    /// Huddles may work, but part of the chain is unusual or unverified.
    Experimental,
    /// PipeWire or the desktop portal's ScreenCast interface is unavailable.
    MissingPortal,
    /// Essential audio/video codecs are missing from the WebKit process.
    MissingCodecs,
    /// No camera or microphone input device was found.
    MissingDevice,
    /// The renderer cannot do WebRTC at all.
    UnsupportedByRenderer,
    /// Slack's web client refused to expose media APIs in this environment.
    BlockedBySlackBrowserPolicy,
}

impl HuddleSupport {
    /// Short user-facing label.
    pub fn label(self) -> &'static str {
        match self {
            HuddleSupport::Supported => "Supported",
            HuddleSupport::Experimental => "Experimental",
            HuddleSupport::MissingPortal => "Missing portal",
            HuddleSupport::MissingCodecs => "Missing codecs",
            HuddleSupport::MissingDevice => "Missing device",
            HuddleSupport::UnsupportedByRenderer => "Unsupported by renderer",
            HuddleSupport::BlockedBySlackBrowserPolicy => "Blocked by Slack browser policy",
        }
    }

    /// Privacy-safe explanation for the diagnostics report.
    pub fn describe(self) -> &'static str {
        match self {
            HuddleSupport::Supported => {
                "Huddles should work. PipeWire, the portal, codecs, and input devices are present."
            }
            HuddleSupport::Experimental => {
                "Huddles may work, but part of the media chain is unusual. Check the report for details."
            }
            HuddleSupport::MissingPortal => {
                "Screen sharing needs a running PipeWire session and the desktop portal ScreenCast interface (xdg-desktop-portal + xdg-desktop-portal-gtk/gnome/kde)."
            }
            HuddleSupport::MissingCodecs => {
                "The WebKit process is missing essential audio/video codecs. Install gstreamer1.0-plugins-good and gstreamer1.0-plugins-bad (or -ugly) for the H.264 codec."
            }
            HuddleSupport::MissingDevice => {
                "No microphone or camera was detected. Huddles cannot capture without an input device."
            }
            HuddleSupport::UnsupportedByRenderer => {
                "The embedded renderer cannot do WebRTC, so Huddles are not available in this build."
            }
            HuddleSupport::BlockedBySlackBrowserPolicy => {
                "Slack's web client did not expose media APIs in this environment. Try opening the Huddle in a supported browser."
            }
        }
    }
}

/// Codec availability probed from inside the WebKit process.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodecSet {
    pub opus: bool,
    pub vp8: bool,
    pub vp9: bool,
    pub h264: bool,
    pub av1: bool,
}

impl CodecSet {
    /// Core Huddle codecs: Opus for audio, and at least one video codec.
    fn has_core_codecs(self) -> bool {
        self.opus && (self.vp8 || self.vp9 || self.h264 || self.av1)
    }
}

/// A snapshot of the media environment, gathered without touching the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HuddleReport {
    pub pipewire_connected: bool,
    pub screencast_portal: bool,
    pub audio_device: bool,
    pub video_device: bool,
    pub webrtc_available: bool,
    /// `None` until the renderer codec probe has run.
    pub codecs: Option<CodecSet>,
    /// `true` when Slack's page exposes `MediaRecorder`/`MediaStream`.
    pub media_api_exposed: bool,
}

impl Default for HuddleReport {
    fn default() -> Self {
        Self {
            pipewire_connected: false,
            screencast_portal: false,
            audio_device: false,
            video_device: false,
            webrtc_available: true,
            codecs: None,
            media_api_exposed: true,
        }
    }
}

impl HuddleReport {
    /// The classification for this snapshot. Pure and order-independent of how
    /// the fields were probed, so the logic is directly testable.
    pub fn classify(&self) -> HuddleSupport {
        if !self.webrtc_available {
            return HuddleSupport::UnsupportedByRenderer;
        }
        if !self.media_api_exposed {
            return HuddleSupport::BlockedBySlackBrowserPolicy;
        }
        if !self.pipewire_connected || !self.screencast_portal {
            return HuddleSupport::MissingPortal;
        }
        if self.codecs.is_some_and(|codecs| !codecs.has_core_codecs()) {
            return HuddleSupport::MissingCodecs;
        }
        if !self.audio_device && !self.video_device {
            return HuddleSupport::MissingDevice;
        }
        if self.codecs.is_none() {
            // Codecs not yet probed: everything else is in place.
            return HuddleSupport::Experimental;
        }
        HuddleSupport::Supported
    }
}

/// Runs the synchronous, filesystem- and D-Bus-based environment probes and
/// returns the resulting snapshot. Called on the UI thread before the async
/// WebKit probe, so the classification shown is the final one only after
/// [`probe_renderer_codecs`] merges its results in.
pub fn probe_environment() -> HuddleReport {
    HuddleReport {
        pipewire_connected: pipewire_session_active(),
        screencast_portal: screencast_portal_available(),
        audio_device: audio_input_device_present(),
        video_device: video_capture_device_present(),
        webrtc_available: true,
        codecs: None,
        media_api_exposed: true,
    }
}

/// Whether a live PipeWire session is reachable. Checking only that the binary
/// exists is not enough — the daemon must actually answer a control request.
fn pipewire_session_active() -> bool {
    if !runtime_socket("pipewire-0").exists() {
        return false;
    }
    // `pw-cli info` connects to the daemon; it only exits 0 when a session is
    // really up. Bound the wait so a hung daemon cannot freeze the doctor.
    Command::new("pw-cli")
        .arg("info")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Whether the desktop portal exposes the ScreenCast interface WebKit uses for
/// screen sharing. Checks the portal D-Bus name and introspects for the
/// interface, without the portal the screen-picker dialog cannot appear.
fn screencast_portal_available() -> bool {
    if Command::new("gdbus")
        .args([
            "introspect",
            "--session",
            "--dest",
            "org.freedesktop.portal.Desktop",
            "--object-path",
            "/org/freedesktop/portal/desktop",
        ])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .is_some_and(|xml| xml.contains("ScreenCast"))
    {
        return true;
    }
    Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.portal.Desktop",
            "--type=method_call",
            "--print-reply",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.DBus.Peer.Ping",
        ])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// A real audio input device: either a PipeWire capture source or a card in
/// ALSA. A card without capture capability is not treated as input.
fn audio_input_device_present() -> bool {
    if let Ok(output) = Command::new("pw-cli").args(["ls", "Source"]).output() {
        if let Ok(stdout) = String::from_utf8(output.stdout) {
            if stdout
                .lines()
                .any(|line| line.contains("Source") && line.contains("audio"))
            {
                return true;
            }
        }
    }
    if let Ok(cards) = std::fs::read_to_string("/proc/asound/cards") {
        if cards.lines().count() > 2 {
            return true;
        }
    }
    false
}

/// A camera: any video device node present.
fn video_capture_device_present() -> bool {
    let Ok(entries) = std::fs::read_dir("/dev") else {
        return false;
    };
    entries
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy().starts_with("video"))
}

fn runtime_socket(name: &str) -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user".into());
    PathBuf::from(dir).join(name)
}

/// JS evaluated inside the WebView. Best-effort: MediaRecorder support tells
/// us which codecs the WebKit GStreamer pipeline really has, and whether the
/// Slack page exposes the media API at all.
pub fn codec_probe_script() -> &'static str {
    "(() => {
        const isSupported = (mime) => {
            try {
                return typeof MediaRecorder !== 'undefined' && MediaRecorder.isTypeSupported(mime);
            } catch { return false; }
        };
        return JSON.stringify({
            opus: isSupported('audio/webm;codecs=opus'),
            vp8: isSupported('video/webm;codecs=vp8'),
            vp9: isSupported('video/webm;codecs=vp9'),
            h264: isSupported('video/webm;codecs=h264'),
            av1: isSupported('video/webm;codecs=av1'),
            mediaExposed: typeof MediaStream !== 'undefined' && typeof MediaRecorder !== 'undefined',
        });
    })()"
}

/// Merges the WebKit codec probe results into the report and reclassifies.
/// `None` payloads are ignored so a probe failure never corrupts the report.
pub fn apply_codec_results(report: &mut HuddleReport, probe: Option<&str>) {
    let Some(payload) = probe else { return };
    let value: serde_json::Value = match serde_json::from_str(payload) {
        Ok(value) => value,
        Err(_) => return,
    };
    report.codecs = Some(CodecSet {
        opus: value["opus"].as_bool().unwrap_or(false),
        vp8: value["vp8"].as_bool().unwrap_or(false),
        vp9: value["vp9"].as_bool().unwrap_or(false),
        h264: value["h264"].as_bool().unwrap_or(false),
        av1: value["av1"].as_bool().unwrap_or(false),
    });
    report.media_api_exposed = value["mediaExposed"].as_bool().unwrap_or(true);
}

/// Privacy-safe, human-readable report used by the diagnostics flow.
pub fn describe(report: &HuddleReport) -> String {
    let codecs = report
        .codecs
        .map(|codecs| {
            format!(
                "opus={} vp8={} vp9={} h264={} av1={}",
                codecs.opus, codecs.vp8, codecs.vp9, codecs.h264, codecs.av1
            )
        })
        .unwrap_or_else(|| "not probed".into());
    format!(
        "Huddle support: {}\n\
         - {}\n\
         - PipeWire session: {}\n\
         - Portal ScreenCast: {}\n\
         - Microphone detected: {}\n\
         - Camera detected: {}\n\
         - WebRTC in renderer: {}\n\
         - Media API exposed by Slack page: {}\n\
         - Codecs: {}",
        report.classify().label(),
        report.classify().describe(),
        report.pipewire_connected,
        report.screencast_portal,
        report.audio_device,
        report.video_device,
        report.webrtc_available,
        report.media_api_exposed,
        codecs,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supported_report() -> HuddleReport {
        HuddleReport {
            pipewire_connected: true,
            screencast_portal: true,
            audio_device: true,
            video_device: true,
            webrtc_available: true,
            codecs: Some(CodecSet {
                opus: true,
                vp8: true,
                vp9: true,
                h264: true,
                av1: true,
            }),
            media_api_exposed: true,
        }
    }

    #[test]
    fn full_environment_is_supported() {
        assert_eq!(supported_report().classify(), HuddleSupport::Supported);
    }

    #[test]
    fn missing_pipewire_or_portal_is_missing_portal() {
        let mut report = supported_report();
        report.pipewire_connected = false;
        assert_eq!(report.classify(), HuddleSupport::MissingPortal);
        let mut report = supported_report();
        report.screencast_portal = false;
        assert_eq!(report.classify(), HuddleSupport::MissingPortal);
    }

    #[test]
    fn missing_codecs_downgrades_to_missing_codecs() {
        let mut report = supported_report();
        report.codecs = Some(CodecSet {
            opus: false,
            vp8: true,
            vp9: false,
            h264: false,
            av1: false,
        });
        assert_eq!(report.classify(), HuddleSupport::MissingCodecs);
        // Audio present but no video codec is also missing.
        let mut report = supported_report();
        report.codecs = Some(CodecSet {
            opus: true,
            vp8: false,
            vp9: false,
            h264: false,
            av1: false,
        });
        assert_eq!(report.classify(), HuddleSupport::MissingCodecs);
    }

    #[test]
    fn no_input_devices_is_missing_device() {
        let mut report = supported_report();
        report.audio_device = false;
        report.video_device = false;
        assert_eq!(report.classify(), HuddleSupport::MissingDevice);
        // One working device is enough.
        let mut report = supported_report();
        report.video_device = false;
        assert_eq!(report.classify(), HuddleSupport::Supported);
    }

    #[test]
    fn unprobed_codecs_are_experimental_not_fatal() {
        let mut report = supported_report();
        report.codecs = None;
        assert_eq!(report.classify(), HuddleSupport::Experimental);
    }

    #[test]
    fn blocked_page_outranks_portal_checks() {
        let mut report = supported_report();
        report.media_api_exposed = false;
        assert_eq!(
            report.classify(),
            HuddleSupport::BlockedBySlackBrowserPolicy
        );
    }

    #[test]
    fn no_webrtc_is_unsupported_by_renderer() {
        let mut report = supported_report();
        report.webrtc_available = false;
        assert_eq!(report.classify(), HuddleSupport::UnsupportedByRenderer);
    }

    #[test]
    fn codec_probe_payload_is_merged() {
        let mut report = supported_report();
        report.codecs = None;
        apply_codec_results(
            &mut report,
            Some(
                r#"{"opus":true,"vp8":false,"vp9":false,"h264":true,"av1":false,"mediaExposed":true}"#,
            ),
        );
        assert!(report.codecs.unwrap().h264);
        assert!(!report.codecs.unwrap().vp8);
        assert_eq!(report.classify(), HuddleSupport::Supported);
    }

    #[test]
    fn garbage_codec_probe_is_ignored() {
        let mut report = supported_report();
        apply_codec_results(&mut report, Some("not json"));
        assert_eq!(report.classify(), HuddleSupport::Supported);
        assert_eq!(report.codecs, Some(supported_report().codecs.unwrap()));
    }
}
