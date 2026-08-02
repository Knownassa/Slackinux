//! Linux runtime compatibility.
//!
//! Tauri AppImages bundle WebKitGTK and GTK. Those libraries can be older than
//! the host's security-patched runtime or incompatible with a newer graphics
//! stack. When a compatible host WebKitGTK 4.1 is installed, re-execute without
//! the bundled library environment. `APPDIR` disappearing is the loop guard;
//! unlike an inherited marker, it also remains correct after an in-app update.

#[cfg(target_os = "linux")]
pub fn prefer_host_webkit_for_appimage() {
    use std::os::unix::process::CommandExt;

    if std::env::var_os("APPIMAGE").is_none() || std::env::var_os("APPDIR").is_none() {
        return;
    }

    if !host_webkit_available() {
        eprintln!("Slackinux: host WebKitGTK 4.1 not found; using AppImage runtime");
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
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        command.env("GDK_BACKEND", "wayland");
    } else {
        command.env("GDK_BACKEND", "x11");
    }

    eprintln!("Slackinux: using the host WebKitGTK 4.1 runtime");
    let error = command.exec();
    eprintln!("Slackinux could not start with the host WebKitGTK runtime: {error}");
}

#[cfg(not(target_os = "linux"))]
pub fn prefer_host_webkit_for_appimage() {}

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

#[cfg(target_os = "linux")]
fn host_webkit_available() -> bool {
    const LIBRARY: &str = "libwebkit2gtk-4.1.so.0";
    const LIBRARY_DIRS: &[&str] = &[
        "/usr/lib",
        "/usr/lib64",
        "/usr/lib/x86_64-linux-gnu",
        "/lib/x86_64-linux-gnu",
    ];

    LIBRARY_DIRS
        .iter()
        .any(|directory| std::path::Path::new(directory).join(LIBRARY).is_file())
        || std::process::Command::new("ldconfig")
            .arg("-p")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .is_some_and(|output| String::from_utf8_lossy(&output.stdout).contains(LIBRARY))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_runtime_variables_include_appdir_loop_guard() {
        assert!(bundled_runtime_variables().contains(&"APPDIR"));
    }
}
