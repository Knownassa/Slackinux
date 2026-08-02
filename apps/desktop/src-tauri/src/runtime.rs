//! Linux runtime compatibility.
//!
//! Tauri AppImages bundle WebKitGTK and GTK. Those libraries can be older than
//! the host's security-patched runtime or incompatible with a newer graphics
//! stack. When a compatible host WebKitGTK 4.1 is installed, re-execute through
//! the host dynamic loader while inhibiting the packaged binary's RUNPATH.
//! `APPDIR` disappearing is the loop guard; unlike an inherited marker, it also
//! remains correct after an in-app update.

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

    let Some(loader) = host_dynamic_loader() else {
        eprintln!("Slackinux: host dynamic loader not found; using AppImage runtime");
        return;
    };

    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    // linuxdeploy adds `$ORIGIN/../lib` to the executable's RUNPATH. Clearing
    // LD_LIBRARY_PATH alone therefore still loads the bundled WebKitGTK. The
    // glibc loader's empty inhibit list disables every RPATH/RUNPATH entry for
    // this execution and lets the normal host library search resolve WebKit.
    let mut command = std::process::Command::new(loader);
    command.arg("--inhibit-rpath").arg("").arg(executable);
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

#[cfg(target_os = "linux")]
fn host_dynamic_loader() -> Option<&'static str> {
    const LOADERS: &[&str] = &[
        "/lib64/ld-linux-x86-64.so.2",
        "/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
        "/usr/lib64/ld-linux-x86-64.so.2",
        "/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
    ];

    LOADERS
        .iter()
        .copied()
        .find(|loader| std::path::Path::new(loader).is_file())
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

    #[test]
    fn host_loader_is_available_on_supported_linux_build_hosts() {
        assert!(host_dynamic_loader().is_some());
    }
}
