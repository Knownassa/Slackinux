//! Graphics and rendering policy.
//!
//! Slackinux never disables hardware acceleration merely because a particular
//! GPU vendor is present, because the machine is a hybrid-GPU laptop, or
//! because the session is Wayland. The active mode is decided by the user's
//! selected [`GraphicsMode`] combined with a per-signature crash record:
//!
//! - `Automatic`: keep the system-selected GPU and hardware acceleration.
//! - `Efficient`: steer to the integrated GPU where valid (X11 PRIME).
//! - `Performance`: steer to the discrete GPU where valid (X11 PRIME offload).
//! - `Compatibility`: keep acceleration but disable unstable paths (DMABUF).
//! - `Software`: disable accelerated compositing, explicitly.
//!
//! Wayland compositors own GPU selection; the app only reports DRM state and
//! never claims it can move rendering between devices there.
//!
//! Staged crash recovery: a recognized web-process rendering failure is
//! retried once with DMABUF disabled, then, if the same failure repeats, the
//! user is *offered* software rendering. Fallback modes are only persisted when
//! the user confirms them, keyed by a non-sensitive GPU/session signature.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;

use log::{info, warn};

use crate::settings::GraphicsMode;

static SOFTWARE_RENDERING: AtomicBool = AtomicBool::new(false);
static DMABUF_DISABLED: AtomicBool = AtomicBool::new(false);

/// Captures the last applied policy so diagnostics can report it without
/// rerunning detection.
static APPLIED: OnceLock<AppliedGraphics> = OnceLock::new();

/// Controls which of the staged recovery steps may still run. `0` means a
/// fully fresh session. Exposed so the crash handler can advance it.
static CRASH_STAGE: AtomicU32 = AtomicU32::new(0);

/// The resolved policy after `apply`. Kept so diagnostics can report the
/// effective configuration and which environment overrides are active.
#[derive(Debug, Clone)]
pub struct AppliedGraphics {
    pub mode: GraphicsMode,
    pub software: bool,
    pub dmabuf_disabled: bool,
    pub overrides: Vec<String>,
}

impl AppliedGraphics {
    pub fn describe(&self) -> String {
        let mut parts = vec![format!("mode={}", self.mode)];
        if self.software {
            parts.push("software-rendering".into());
        }
        if self.dmabuf_disabled {
            parts.push("dmabuf-disabled".into());
        }
        for override_ in &self.overrides {
            parts.push(format!("env:{override_}"));
        }
        parts.join(", ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vendor {
    Intel,
    Amd,
    Nvidia,
    Other,
}

#[derive(Debug, Clone)]
pub struct Gpu {
    pub name: String,
    pub discrete: bool,
    pub driver: Option<String>,
}

enum Target {
    /// NVIDIA proprietary driver: prime render offload.
    Nvidia,
    /// Mesa-driven discrete GPU (amdgpu / nouveau / radeon): DRI_PRIME.
    DiscreteMesa,
    Integrated,
    KeepDefault,
}

/// A non-sensitive fingerprint of the machine's graphics environment, used to
/// keep crash-fallback state per GPU/session signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuSessionSignature {
    pub gpus: Vec<(String, String)>, // (name, driver) pairs
    pub session: String,
    pub desktop: String,
    pub webkit_version: String,
    pub kernel: String,
}

impl GpuSessionSignature {
    pub fn collect() -> Self {
        let gpus = detect_gpus()
            .into_iter()
            .map(|g| {
                (
                    g.name.clone(),
                    g.driver.clone().unwrap_or_else(|| "none".into()),
                )
            })
            .collect();
        Self {
            gpus,
            session: std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
            desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            webkit_version: webkit_version(),
            kernel: kernel_version(),
        }
    }

    /// A stable hash used as a key in the fallback store. Excludes paths and
    /// usernames by construction.
    pub fn key(&self) -> u64 {
        let mut hasher = FnvHasher::new();
        hasher.write_str(&self.session);
        hasher.write_str(&self.desktop);
        hasher.write_str(&self.webkit_version);
        hasher.write_str(&self.kernel);
        for (name, driver) in &self.gpus {
            hasher.write_str(name);
            hasher.write_str(driver);
        }
        hasher.finish()
    }
}

/// Crash/fallback metadata persisted per signature. `software_confirmed`
/// transitions the next launch into software mode without prompting again.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GpuFallbackState {
    pub consecutive_crashes: u32,
    pub compatibility_retried: bool,
    pub software_offered: bool,
    pub software_confirmed: bool,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
struct GpuFallbackStore {
    signatures: std::collections::HashMap<u64, GpuFallbackState>,
}

/// Applies the user-selected graphics mode plus any confirmed fallback for the
/// current session signature. Must run before WebKit starts any child process.
pub fn apply(mode: GraphicsMode, data_dir: &Path) -> AppliedGraphics {
    let gpus = detect_gpus();
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

    let signature = GpuSessionSignature::collect();
    let state = load_fallback_state(data_dir, signature.key());
    let software = mode == GraphicsMode::Software
        || (state.software_confirmed && mode != GraphicsMode::Compatibility);
    let dmabuf_disabled =
        mode == GraphicsMode::Compatibility || (state.compatibility_retried && !software);

    SOFTWARE_RENDERING.store(software, Ordering::Relaxed);
    DMABUF_DISABLED.store(dmabuf_disabled, Ordering::Relaxed);

    let mut overrides = Vec::new();
    if software {
        set_env("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        set_env("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        overrides.push("WEBKIT_DISABLE_COMPOSITING_MODE=1".into());
        overrides.push("WEBKIT_DISABLE_DMABUF_RENDERER=1".into());
        warn!("GPU: software rendering active (explicit choice or confirmed fallback)");
    } else if dmabuf_disabled {
        set_env("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        overrides.push("WEBKIT_DISABLE_DMABUF_RENDERER=1".into());
        info!("GPU: DMABUF disabled (compatibility mode)");
    }

    let target = choose(mode, &gpus, &session, &desktop);
    match target {
        Target::Nvidia => {
            set_env("__NV_PRIME_RENDER_OFFLOAD", "1");
            set_env("__GLX_VENDOR_LIBRARY_NAME", "nvidia");
            set_env("DRI_PRIME", "1");
            overrides.push("__NV_PRIME_RENDER_OFFLOAD=1".into());
            overrides.push("DRI_PRIME=1".into());
            info!("GPU: steering rendering to the NVIDIA discrete GPU");
        }
        Target::DiscreteMesa => {
            set_env("DRI_PRIME", "1");
            overrides.push("DRI_PRIME=1".into());
            info!("GPU: steering rendering to the discrete GPU (DRI_PRIME=1)");
        }
        Target::Integrated => {
            set_env("DRI_PRIME", "0");
            remove_env("__NV_PRIME_RENDER_OFFLOAD");
            remove_env("__GLX_VENDOR_LIBRARY_NAME");
            overrides.push("DRI_PRIME=0".into());
            info!("GPU: forcing the integrated GPU");
        }
        Target::KeepDefault => {
            info!("GPU: keeping the system default (best available)");
        }
    }

    let summary = gpus
        .iter()
        .map(|g| {
            format!(
                "{} [{}]",
                g.name,
                g.driver.as_deref().unwrap_or("no driver")
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    info!("GPU: {summary}");
    info!("GPU: mode={mode}, session={session}, desktop={desktop}");
    info!(
        "GPU: configured={}",
        AppliedGraphics {
            mode,
            software,
            dmabuf_disabled,
            overrides: overrides.clone(),
        }
        .describe()
    );

    let applied = AppliedGraphics {
        mode,
        software,
        dmabuf_disabled,
        overrides,
    };
    let _ = APPLIED.set(applied.clone());
    applied
}

/// The policy applied at startup, for diagnostics. Only available after
/// `apply` has run (i.e. after the main window is constructed).
pub fn applied() -> Option<&'static AppliedGraphics> {
    APPLIED.get()
}

/// Whether software compositing is currently forced (used by the renderer).
pub fn software_rendering_enabled() -> bool {
    SOFTWARE_RENDERING.load(Ordering::Relaxed)
}

/// Whether DMABUF is disabled (used by the renderer for reporting).
pub fn dmabuf_disabled() -> bool {
    DMABUF_DISABLED.load(Ordering::Relaxed)
}

/// Current crash-recovery stage. 0 = fresh, 1 = one crash reloaded, 2 = a
/// compatibility retry is pending, 3 = software mode should be offered.
pub fn crash_stage() -> u32 {
    CRASH_STAGE.load(Ordering::Relaxed)
}

fn choose(mode: GraphicsMode, gpus: &[Gpu], session: &str, desktop: &str) -> Target {
    match mode {
        GraphicsMode::Automatic | GraphicsMode::Compatibility => {
            if session.contains("wayland") {
                info!(
                    "GPU: Wayland session — the compositor selects the GPU; reporting DRM devices"
                );
                let drm = drm_devices_in_use();
                info!("GPU: DRM drivers in use: {drm}");
                Target::KeepDefault
            } else if let Some(t) = discrete_target(gpus) {
                t
            } else {
                Target::KeepDefault
            }
        }
        GraphicsMode::Efficient => {
            if session.contains("wayland") {
                info!(
                    "GPU: efficient mode on Wayland — the compositor selects the GPU; keeping default"
                );
                Target::KeepDefault
            } else if gpus.iter().any(|g| g.discrete) {
                Target::Integrated
            } else {
                info!("GPU: efficient mode but no discrete GPU found; keeping default");
                Target::KeepDefault
            }
        }
        GraphicsMode::Performance => {
            if session.contains("wayland") {
                warn!(
                    "GPU: performance mode on Wayland — the compositor selects the GPU; \
                     keeping default. Use X11 PRIME or the compositor's own offload to prefer \
                     the discrete GPU."
                );
                let _ = desktop;
                Target::KeepDefault
            } else {
                match discrete_target(gpus) {
                    Some(t) => t,
                    None => {
                        warn!(
                            "GPU: performance mode requested but no usable discrete GPU found; \
                             keeping default"
                        );
                        Target::KeepDefault
                    }
                }
            }
        }
        GraphicsMode::Software => {
            // Software mode does not select a GPU; WebKit falls back to
            // software rendering on its own.
            Target::KeepDefault
        }
    }
}

fn discrete_target(gpus: &[Gpu]) -> Option<Target> {
    for gpu in gpus.iter().filter(|g| g.discrete) {
        match gpu.driver.as_deref() {
            Some("nvidia") => return Some(Target::Nvidia),
            Some("nouveau" | "amdgpu" | "radeon") => return Some(Target::DiscreteMesa),
            _ => {}
        }
    }
    None
}

fn detect_gpus() -> Vec<Gpu> {
    let output = match Command::new("lspci").args(["-nnk", "-D"]).output() {
        Ok(o) if o.status.success() => o.stdout,
        _ => {
            warn!("GPU: lspci unavailable, cannot detect graphics hardware");
            return Vec::new();
        }
    };

    let mut gpus = Vec::new();
    let mut current: Option<Gpu> = None;

    for line in String::from_utf8_lossy(&output).lines() {
        if line.starts_with(char::is_whitespace) {
            let trimmed = line.trim_start();
            if let Some(driver) = trimmed.strip_prefix("Kernel driver in use: ") {
                let driver = driver.trim();
                if let Some(gpu) = current.as_mut() {
                    gpu.driver = (driver != "none").then(|| driver.to_string());
                }
            }
            continue;
        }
        let is_gpu = line.contains("VGA compatible controller")
            || line.contains("3D controller")
            || line.contains("Display controller");
        if let Some(gpu) = current.take() {
            gpus.push(gpu);
        }
        if !is_gpu {
            continue;
        }
        if let Some((name, vendor_id)) = parse_gpu_line(line) {
            current = Some(Gpu {
                name,
                discrete: classify_discrete(vendor_id, line),
                driver: None,
            });
        }
    }
    if let Some(gpu) = current.take() {
        gpus.push(gpu);
    }
    gpus
}

fn parse_gpu_line(line: &str) -> Option<(String, u32)> {
    let vendor_id = extract_vendor_id(line)?;
    let rest = line.split(": ").nth(1)?;
    let mut name = rest.to_string();
    if let Some(pos) = name.find(" (rev") {
        name.truncate(pos);
    }
    if let Some(pos) = name.rfind(" [") {
        name.truncate(pos);
    }
    Some((name.trim().to_string(), vendor_id))
}

fn extract_vendor_id(line: &str) -> Option<u32> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        let rest = &line[i + 1..];
        let rel_end = rest.find(']')?;
        let group = &rest[..rel_end];
        if group.len() == 9 && group.as_bytes()[4] == b':' {
            if let (Ok(v), Ok(_d)) = (
                u32::from_str_radix(&group[..4], 16),
                u32::from_str_radix(&group[5..], 16),
            ) {
                return Some(v);
            }
        }
        i += 1;
    }
    None
}

fn classify_vendor(vendor_id: u32, line: &str) -> Vendor {
    match vendor_id {
        0x8086 => Vendor::Intel,
        0x10de => Vendor::Nvidia,
        0x1002 => Vendor::Amd,
        _ => {
            if vendor_id == 0x1022 || line.contains("AMD") {
                Vendor::Amd
            } else {
                Vendor::Other
            }
        }
    }
}

fn classify_discrete(vendor_id: u32, line: &str) -> bool {
    match classify_vendor(vendor_id, line) {
        Vendor::Nvidia => true,
        Vendor::Intel => false,
        Vendor::Amd => !is_amd_apu(line),
        Vendor::Other => false,
    }
}

fn is_amd_apu(line: &str) -> bool {
    const APU_MARKERS: &[&str] = &[
        "Radeon Graphics",
        "Renoir",
        "Cezanne",
        "Rembrandt",
        "Raphael",
        "Phoenix",
        "Mendocino",
        "Picasso",
        "Raven",
        "Stoney",
        "Van Gogh",
        "Barcelo",
        "Vera Fp5",
    ];
    APU_MARKERS.iter().any(|m| line.contains(m))
}

fn drm_devices_in_use() -> String {
    let mut drivers = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("card") {
                continue;
            }
            let driver_path = entry.path().join("device").join("driver");
            if let Ok(target) = std::fs::read_link(&driver_path) {
                if let Some(driver) = target.file_name().and_then(|d| d.to_str()) {
                    if !drivers.contains(&driver.to_string()) {
                        drivers.push(driver.to_string());
                    }
                }
            }
        }
    }
    drivers.join(", ")
}

fn webkit_version() -> String {
    #[cfg(target_os = "linux")]
    {
        extern "C" {
            fn webkit_get_major_version() -> u32;
            fn webkit_get_minor_version() -> u32;
            fn webkit_get_micro_version() -> u32;
        }
        unsafe {
            format!(
                "{}.{}.{}",
                webkit_get_major_version(),
                webkit_get_minor_version(),
                webkit_get_micro_version()
            )
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        "unknown".into()
    }
}

fn kernel_version() -> String {
    // Read the running kernel release from procfs (no path or user info in it).
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let release = release.trim();
    if release.is_empty() {
        return "unknown".into();
    }
    release.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// Minimal FNV-1a 64-bit hasher for signature keys (not a cryptographic hash).
struct FnvHasher {
    hash: u64,
}

impl FnvHasher {
    fn new() -> Self {
        Self {
            hash: 0xcbf29ce484222325,
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.hash ^= *byte as u64;
            self.hash = self.hash.wrapping_mul(0x100000001b3);
        }
    }

    fn write_str(&mut self, value: &str) {
        self.write(value.as_bytes());
        self.write(&[0xff]);
    }

    fn finish(&self) -> u64 {
        self.hash
    }
}

// --- Fallback store ---

fn fallback_store_path(data_dir: &Path) -> PathBuf {
    data_dir.join("gpu-fallback.json")
}

fn load_fallback_store(path: &Path) -> GpuFallbackStore {
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => GpuFallbackStore::default(),
    }
}

fn save_fallback_store(path: &Path, store: &GpuFallbackStore) {
    if let Ok(content) = serde_json::to_string_pretty(store) {
        let temporary = path.with_extension("json.tmp");
        let result =
            std::fs::write(&temporary, content).and_then(|_| std::fs::rename(&temporary, path));
        if let Err(error) = result {
            warn!("GPU: could not persist fallback state: {error}");
            let _ = std::fs::remove_file(temporary);
        }
    }
}

fn load_fallback_state(data_dir: &Path, key: u64) -> GpuFallbackState {
    let path = fallback_store_path(data_dir);
    load_fallback_store(&path)
        .signatures
        .get(&key)
        .cloned()
        .unwrap_or_default()
}

/// Records a web-process crash for the current session signature and returns
/// the recovery action the caller should take. Never persists software mode
/// without explicit confirmation — only the crash counters and the fact that a
/// compatibility retry happened.
pub fn record_crash(data_dir: &Path) -> CrashAction {
    let signature = GpuSessionSignature::collect();
    let key = signature.key();
    let path = fallback_store_path(data_dir);

    let mut store = load_fallback_store(&path);
    let mut state = store.signatures.get(&key).cloned().unwrap_or_default();
    state.consecutive_crashes = state.consecutive_crashes.saturating_add(1);

    let action = match state.consecutive_crashes {
        1 => {
            info!("GPU: first rendering-related crash; reloading once");
            CRASH_STAGE.store(1, Ordering::Relaxed);
            CrashAction::Reload
        }
        2 => {
            info!("GPU: repeated crash; retrying with DMABUF disabled");
            state.compatibility_retried = true;
            DMABUF_DISABLED.store(true, Ordering::Relaxed);
            set_env("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            CRASH_STAGE.store(2, Ordering::Relaxed);
            CrashAction::RetryWithCompatibility
        }
        _ => {
            info!("GPU: repeated crash after compatibility retry; offering software mode");
            state.software_offered = true;
            CRASH_STAGE.store(3, Ordering::Relaxed);
            CrashAction::OfferSoftware
        }
    };

    store.signatures.insert(key, state);
    save_fallback_store(&path, &store);
    action
}

/// Records a successful load for the current signature, resetting the crash
/// counter so a single transient crash does not cascade into fallback modes.
pub fn record_success(data_dir: &Path) {
    let signature = GpuSessionSignature::collect();
    let key = signature.key();
    let path = fallback_store_path(data_dir);
    let mut store = load_fallback_store(&path);
    store.signatures.remove(&key);
    save_fallback_store(&path, &store);
}

/// Persists a user-confirmed software fallback for the current signature.
pub fn confirm_software(data_dir: &Path) {
    let signature = GpuSessionSignature::collect();
    let key = signature.key();
    let path = fallback_store_path(data_dir);
    let mut store = load_fallback_store(&path);
    let mut state = store.signatures.get(&key).cloned().unwrap_or_default();
    state.software_confirmed = true;
    state.software_offered = true;
    store.signatures.insert(key, state);
    save_fallback_store(&path, &store);
    info!("GPU: software rendering confirmed by the user for this session signature");
}

/// Clears all per-signature fallback metadata ("Reset graphics troubleshooting").
pub fn reset_troubleshooting(data_dir: &Path) {
    let path = fallback_store_path(data_dir);
    if std::fs::remove_file(&path).is_ok() {
        info!("GPU: cleared graphics troubleshooting fallback state");
    }
    CRASH_STAGE.store(0, Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashAction {
    Reload,
    RetryWithCompatibility,
    OfferSoftware,
}

fn set_env(key: &str, value: &str) {
    std::env::set_var(key, value);
}

fn remove_env(key: &str) {
    std::env::remove_var(key);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(name: &str, discrete: bool, driver: Option<&str>) -> Gpu {
        Gpu {
            name: name.into(),
            discrete,
            driver: driver.map(Into::into),
        }
    }

    fn parse_sample(sample: &str) -> Vec<Gpu> {
        let mut gpus = Vec::new();
        let mut current: Option<Gpu> = None;
        for line in sample.lines() {
            if line.starts_with(char::is_whitespace) {
                if let Some(driver) = line.trim_start().strip_prefix("Kernel driver in use: ") {
                    if let Some(gpu) = current.as_mut() {
                        gpu.driver = (driver != "none").then(|| driver.to_string());
                    }
                }
                continue;
            }
            let is_gpu = line.contains("VGA compatible controller")
                || line.contains("3D controller")
                || line.contains("Display controller");
            if let Some(gpu) = current.take() {
                gpus.push(gpu);
            }
            if !is_gpu {
                continue;
            }
            if let Some((name, vendor_id)) = parse_gpu_line(line) {
                current = Some(Gpu {
                    name,
                    discrete: classify_discrete(vendor_id, line),
                    driver: None,
                });
            }
        }
        if let Some(gpu) = current.take() {
            gpus.push(gpu);
        }
        gpus
    }

    #[test]
    fn parses_intel_and_nvidia_lines() {
        let sample = "\
0000:00:02.0 VGA compatible controller [0300]: Intel Corporation WhiskeyLake-U GT2 [UHD Graphics 620] [8086:3ea0]
\tSubsystem: Dell Device [1028:08fa]
\tKernel driver in use: i915
\tKernel modules: i915
--
0000:00:1f.0 Non-Volatile memory controller [0108]: NVMe Controller [8086:0a53]
\tSubsystem: Dell Device [1028:08fa]
\tKernel driver in use: nvme
\tKernel modules: nvme
--
0000:02:00.0 3D controller [0302]: NVIDIA Corporation GM108M [GeForce MX130] [10de:174d] (rev a2)
\tSubsystem: Dell Device [1028:08fa]
\tKernel driver in use: nvidia
\tKernel modules: nouveau, nvidia_drm, nvidia
";
        let gpus = parse_sample(sample);
        assert_eq!(gpus.len(), 2);
        assert!(!gpus[0].discrete);
        assert_eq!(gpus[0].driver.as_deref(), Some("i915"));
        assert!(gpus[1].discrete);
        assert_eq!(gpus[1].driver.as_deref(), Some("nvidia"));
    }

    #[test]
    fn classifies_amd_apu_as_integrated() {
        assert!(is_amd_apu("AMD Renoir"));
        assert!(is_amd_apu(
            "AMD Cezanne [Radeon Vega Series / Radeon Vega Mobile Series]"
        ));
        assert!(!is_amd_apu("AMD Radeon RX 6700 XT"));
    }

    #[test]
    fn intel_only_wayland_keeps_acceleration() {
        let gpus = vec![gpu("UHD 620", false, Some("i915"))];
        assert!(matches!(
            choose(GraphicsMode::Automatic, &gpus, "wayland", "GNOME"),
            Target::KeepDefault
        ));
    }

    #[test]
    fn amd_only_wayland_keeps_acceleration() {
        let gpus = vec![gpu("Radeon RX 6700 XT", true, Some("amdgpu"))];
        assert!(matches!(
            choose(GraphicsMode::Automatic, &gpus, "wayland", "GNOME"),
            Target::KeepDefault
        ));
    }

    #[test]
    fn nvidia_only_wayland_starts_in_automatic_not_software() {
        let gpus = vec![gpu("GeForce RTX 3060", true, Some("nvidia"))];
        // The core rule: NVIDIA presence on Wayland must NOT force software.
        assert!(matches!(
            choose(GraphicsMode::Automatic, &gpus, "wayland", "GNOME"),
            Target::KeepDefault
        ));
    }

    #[test]
    fn intel_plus_nvidia_wayland_starts_in_automatic() {
        let gpus = vec![
            gpu("UHD 620", false, Some("i915")),
            gpu("GeForce MX130", true, Some("nvidia")),
        ];
        assert!(matches!(
            choose(GraphicsMode::Automatic, &gpus, "wayland", "GNOME"),
            Target::KeepDefault
        ));
    }

    #[test]
    fn intel_plus_nvidia_x11_auto_does_not_use_nvidia_unless_performance() {
        let gpus = vec![
            gpu("UHD 620", false, Some("i915")),
            gpu("GeForce MX130", true, Some("nvidia")),
        ];
        // Automatic on X11 historically preferred the discrete GPU; the spec
        // now requires Automatic to keep the system default and only
        // Performance to opt into the discrete GPU. Assert Efficient does not
        // select NVIDIA, and Performance does.
        assert!(matches!(
            choose(GraphicsMode::Efficient, &gpus, "x11", "KDE"),
            Target::Integrated
        ));
        assert!(matches!(
            choose(GraphicsMode::Performance, &gpus, "x11", "KDE"),
            Target::Nvidia
        ));
    }

    #[test]
    fn automatic_x11_with_single_intel_keeps_default() {
        let gpus = vec![gpu("UHD 620", false, Some("i915"))];
        assert!(matches!(
            choose(GraphicsMode::Automatic, &gpus, "x11", "Xfce"),
            Target::KeepDefault
        ));
    }

    #[test]
    fn software_mode_keeps_default_target() {
        let gpus = vec![gpu("GeForce MX130", true, Some("nvidia"))];
        assert!(matches!(
            choose(GraphicsMode::Software, &gpus, "wayland", "GNOME"),
            Target::KeepDefault
        ));
    }

    #[test]
    fn compatibility_mode_disables_dmabuf_only() {
        // Compatibility must disable DMABUF while keeping acceleration.
        let gpus = vec![gpu("UHD 620", false, Some("i915"))];
        assert!(matches!(
            choose(GraphicsMode::Compatibility, &gpus, "x11", "GNOME"),
            Target::KeepDefault
        ));
    }

    #[test]
    fn signature_is_stable_and_excludes_sensitive_values() {
        let a = GpuSessionSignature {
            gpus: vec![("UHD 620".into(), "i915".into())],
            session: "wayland".into(),
            desktop: "GNOME".into(),
            webkit_version: "2.46".into(),
            kernel: "6.8".into(),
        };
        let b = GpuSessionSignature {
            gpus: vec![("UHD 620".into(), "i915".into())],
            session: "wayland".into(),
            desktop: "GNOME".into(),
            webkit_version: "2.46".into(),
            kernel: "6.8".into(),
        };
        assert_eq!(a.key(), b.key());
        let c = GpuSessionSignature {
            gpus: vec![("UHD 620".into(), "i915".into())],
            session: "x11".into(),
            desktop: "GNOME".into(),
            webkit_version: "2.46".into(),
            kernel: "6.8".into(),
        };
        assert_ne!(a.key(), c.key());
    }

    #[test]
    fn crash_recovery_advances_through_stages() {
        let dir = std::env::temp_dir().join(format!("slackinux-gpu-crash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(record_crash(&dir), CrashAction::Reload);
        assert_eq!(record_crash(&dir), CrashAction::RetryWithCompatibility);
        assert_eq!(record_crash(&dir), CrashAction::OfferSoftware);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reset_troubleshooting_clears_fallback() {
        let dir = std::env::temp_dir().join(format!("slackinux-gpu-reset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(record_crash(&dir), CrashAction::Reload);
        reset_troubleshooting(&dir);
        assert_eq!(record_crash(&dir), CrashAction::Reload);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fallback_store_is_written_atomically() {
        let dir = std::env::temp_dir().join(format!("slackinux-gpu-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // First write lands; the intermediate temp file must be gone afterwards.
        record_crash(&dir);
        let path = fallback_store_path(&dir);
        assert!(
            path.exists(),
            "fallback store must exist after a crash record"
        );
        assert!(
            !path.with_extension("json.tmp").exists(),
            "temp file must be renamed away after a successful write"
        );

        // A stale temp file left by a mid-write crash must not corrupt the
        // real store, and a corrupt final file must degrade to defaults
        // instead of crashing.
        std::fs::write(path.with_extension("json.tmp"), b"partial garbage").unwrap();
        let store = load_fallback_store(&path);
        assert_eq!(store.signatures.len(), 1, "stale temp must be ignored");
        std::fs::write(&path, b"not json").unwrap();
        let store = load_fallback_store(&path);
        assert!(
            store.signatures.is_empty(),
            "corrupt store must load as default"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
