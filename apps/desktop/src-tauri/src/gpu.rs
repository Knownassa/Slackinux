//! GPU selection.
//!
//! Detects the system's graphics hardware and, when the session allows it,
//! steers WebKit's rendering onto the best GPU available (discrete > shared).
//!
//! - Under X11, prime/offload environment variables can move rendering to a
//!   discrete GPU (`DRI_PRIME`, and for NVIDIA proprietary `__NV_PRIME_RENDER_OFFLOAD`
//!   + `__GLX_VENDOR_LIBRARY_NAME`).
//! - Under Wayland the compositor picks the GPU, so nothing app-side can
//!   switch devices; we only report which DRM device is in use.
//!
//! The preference is read before any window is created (the WebKit child
//! processes inherit the environment we set here).

use std::process::Command;

use log::{info, warn};

use crate::settings::GpuPreference;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vendor {
    Intel,
    Amd,
    Nvidia,
    Other,
}

#[derive(Debug)]
struct Gpu {
    name: String,
    discrete: bool,
    driver: Option<String>,
}

enum Target {
    /// NVIDIA proprietary driver: prime render offload.
    Nvidia,
    /// Mesa-driven discrete GPU (amdgpu / nouveau / radeon): DRI_PRIME.
    DiscreteMesa,
    Integrated,
    KeepDefault,
}

pub fn apply(pref: GpuPreference) {
    let gpus = detect_gpus();
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();

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
    info!("GPU: preference={pref}, session={session}");

    match choose(pref, &gpus, &session) {
        Target::Nvidia => {
            set_env("__NV_PRIME_RENDER_OFFLOAD", "1");
            set_env("__GLX_VENDOR_LIBRARY_NAME", "nvidia");
            set_env("DRI_PRIME", "1");
            info!("GPU: steering rendering to the NVIDIA discrete GPU");
        }
        Target::DiscreteMesa => {
            set_env("DRI_PRIME", "1");
            info!("GPU: steering rendering to the discrete GPU (DRI_PRIME=1)");
        }
        Target::Integrated => {
            set_env("DRI_PRIME", "0");
            remove_env("__NV_PRIME_RENDER_OFFLOAD");
            remove_env("__GLX_VENDOR_LIBRARY_NAME");
            info!("GPU: forcing the integrated GPU");
        }
        Target::KeepDefault => {
            info!("GPU: keeping the system default (best available)");
        }
    }
}

fn choose(pref: GpuPreference, gpus: &[Gpu], session: &str) -> Target {
    match pref {
        GpuPreference::Integrated => Target::Integrated,
        GpuPreference::Discrete => {
            if session.contains("wayland") {
                warn!(
                    "GPU: discrete GPU selection is not available on Wayland \
                     (the compositor chooses the GPU); keeping default"
                );
                Target::KeepDefault
            } else {
                match discrete_target(gpus) {
                    Some(t) => t,
                    None => {
                        warn!(
                            "GPU: discrete requested but no usable discrete GPU found; \
                             keeping default"
                        );
                        Target::KeepDefault
                    }
                }
            }
        }
        GpuPreference::Auto => {
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
        // A new (non-indented) line always begins a new PCI device, so a
        // pending GPU is only carried forward when this line is itself a GPU.
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

/// Pulls the PCI vendor id out of a `[vendor:device]` bracket group.
fn extract_vendor_id(line: &str) -> Option<u32> {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        let group = &line[i + 1..line[i + 1..].find(']').map(|p| i + 1 + p)?];
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
            // Some AMD APUs report via vendor id 0x1022 (AMD) for the GPU block.
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

/// APUs (integrated Radeon graphics) advertise themselves with these markers.
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

fn set_env(key: &str, value: &str) {
    std::env::set_var(key, value);
}

fn remove_env(key: &str) {
    std::env::remove_var(key);
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn auto_prefers_discrete_on_x11() {
        let gpus = vec![Gpu {
            name: "Intel UHD".into(),
            discrete: false,
            driver: Some("i915".into()),
        }];
        assert!(matches!(
            choose(GpuPreference::Auto, &gpus, "x11"),
            Target::KeepDefault
        ));

        let gpus = vec![
            Gpu {
                name: "Intel UHD".into(),
                discrete: false,
                driver: Some("i915".into()),
            },
            Gpu {
                name: "GeForce MX130".into(),
                discrete: true,
                driver: Some("nvidia".into()),
            },
        ];
        assert!(matches!(
            choose(GpuPreference::Auto, &gpus, "x11"),
            Target::Nvidia
        ));
    }

    #[test]
    fn wayland_keeps_default() {
        let gpus = vec![Gpu {
            name: "GeForce MX130".into(),
            discrete: true,
            driver: Some("nvidia".into()),
        }];
        assert!(matches!(
            choose(GpuPreference::Auto, &gpus, "wayland"),
            Target::KeepDefault
        ));
        assert!(matches!(
            choose(GpuPreference::Discrete, &gpus, "wayland"),
            Target::KeepDefault
        ));
        assert!(matches!(
            choose(GpuPreference::Discrete, &gpus, "x11"),
            Target::Nvidia
        ));
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
}
