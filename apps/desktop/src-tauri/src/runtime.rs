//! Linux runtime compatibility.
//!
//! Tauri's Ubuntu-built AppImage bundles WebKitGTK and GTK. On rolling
//! Arch-family systems those libraries can be incompatible with the host's
//! much newer Mesa/NVIDIA EGL stack, causing every WebKit child process to
//! abort before it paints. Re-executing the same AppImage binary without the
//! bundled library environment lets it use the host WebKitGTK while retaining
//! the AppImage path and updater behavior.

#[cfg(target_os = "linux")]
pub fn prefer_host_webkit_for_rolling_appimage() {
    use std::os::unix::process::CommandExt;

    const REEXEC_MARKER: &str = "SLACKINUX_HOST_WEBKIT";
    if std::env::var_os(REEXEC_MARKER).is_some()
        || std::env::var_os("APPIMAGE").is_none()
        || std::env::var_os("APPDIR").is_none()
    {
        return;
    }

    let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    if !is_arch_family(&os_release) {
        return;
    }

    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let mut command = std::process::Command::new(executable);
    command.args(std::env::args_os().skip(1));
    for name in bundled_runtime_variables() {
        command.env_remove(name);
    }
    command.env(REEXEC_MARKER, "1");
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        command.env("GDK_BACKEND", "wayland");
    } else {
        command.env("GDK_BACKEND", "x11");
    }

    eprintln!("Slackinux: using host WebKitGTK for Arch-family AppImage compatibility");
    let error = command.exec();
    eprintln!("Slackinux could not start with the host WebKitGTK runtime: {error}");
}

#[cfg(not(target_os = "linux"))]
pub fn prefer_host_webkit_for_rolling_appimage() {}

#[cfg(target_os = "linux")]
fn bundled_runtime_variables() -> &'static [&'static str] {
    &[
        "APPDIR",
        "LD_LIBRARY_PATH",
        "GTK_DATA_PREFIX",
        "GTK_PATH",
        "GTK_THEME",
        "GTK_IM_MODULE_FILE",
        "GDK_PIXBUF_MODULE_FILE",
        "GIO_EXTRA_MODULES",
        "GSETTINGS_SCHEMA_DIR",
        "XDG_DATA_DIRS",
    ]
}

fn is_arch_family(os_release: &str) -> bool {
    os_release.lines().any(|line| {
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        if key != "ID" && key != "ID_LIKE" {
            return false;
        }
        value
            .trim_matches('"')
            .split_whitespace()
            .any(|id| matches!(id, "arch" | "cachyos" | "manjaro" | "endeavouros"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_arch_family_distributions() {
        assert!(is_arch_family("ID=cachyos\nID_LIKE=arch\n"));
        assert!(is_arch_family("ID=manjaro\nID_LIKE=\"arch\"\n"));
        assert!(!is_arch_family("ID=ubuntu\nID_LIKE=debian\n"));
    }
}
