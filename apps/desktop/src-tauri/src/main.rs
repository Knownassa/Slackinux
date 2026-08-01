#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod deep_links;
mod diagnostics;
mod error;
#[cfg(target_os = "linux")]
mod frame;
#[cfg(target_os = "linux")]
mod gpu;
mod navigation;
mod notifications;
mod renderer;
mod runtime;
mod settings;
mod updates;

use std::env;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU16, Ordering};
use std::sync::Arc;

use error::{AppError, AppResult};
use log::{error, info, warn};
use notifications::NotificationManager;
use renderer::{webkit::WebKitRenderer, SlackRenderer};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WebviewUrl, WebviewWindowBuilder,
};
use url::Url;

struct AppState {
    renderer: Arc<dyn SlackRenderer>,
    notif_mgr: Arc<NotificationManager>,
    data_dir: std::path::PathBuf,
    zoom_level: Arc<AtomicU16>,
    gpu_preference: Arc<std::sync::Mutex<settings::GpuPreference>>,
    theme_preference: Arc<std::sync::Mutex<settings::ThemePreference>>,
    auto_check_updates: Arc<AtomicBool>,
    last_update_check_unix: Arc<AtomicI64>,
}

impl AppState {
    /// Rebuilds the persisted settings snapshot from the live state.
    fn settings(&self) -> settings::Settings {
        settings::Settings {
            zoom_level: self.zoom_level.load(Ordering::Relaxed),
            dnd: self.notif_mgr.is_dnd(),
            gpu_preference: *self.gpu_preference.lock().unwrap(),
            theme_preference: *self.theme_preference.lock().unwrap(),
            auto_check_updates: self.auto_check_updates.load(Ordering::Relaxed),
            last_update_check_unix: self.last_update_check_unix.load(Ordering::Relaxed),
        }
    }

    fn set_last_update_check(&self, unix: i64) {
        self.last_update_check_unix.store(unix, Ordering::Relaxed);
    }
}

fn main() -> AppResult<()> {
    runtime::prefer_host_webkit_for_rolling_appimage();
    diagnostics::init_logging();

    let version = env!("CARGO_PKG_VERSION");
    info!("Slackinux v{version} starting");

    let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
    let desktop = env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into());
    info!("session: {session_type}, desktop: {desktop}");

    #[cfg(target_os = "linux")]
    log_webkit_version();

    let zoom_level = Arc::new(AtomicU16::new(10));
    let zoom_level_menu = zoom_level.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            handle_slack_launch_args(app, &argv);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            #[cfg(target_os = "linux")]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                // AppImages do not have a traditional installer. Registering
                // here makes browser sign-in callbacks work even when the
                // user launches the AppImage directly or moves it.
                if let Err(err) = app.deep_link().register_all() {
                    // Missing desktop-database tools must never prevent Slack
                    // itself from opening; packaged and shell installs still
                    // carry the static MIME association.
                    warn!("could not register Slack browser callback handler: {err}");
                }
            }

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir).ok();
            migrate_legacy_data_dir(&data_dir);
            info!(
                "profile path: {}",
                redact_path(&data_dir.display().to_string())
            );

            let user_settings = settings::Settings::load(&data_dir);
            info!(
                "loaded settings: zoom={}x, dnd={}, theme={}",
                f64::from(user_settings.zoom_level) / 10.0,
                user_settings.dnd,
                user_settings.theme_preference
            );

            #[cfg(target_os = "linux")]
            {
                gpu::apply(user_settings.gpu_preference);
            }

            #[cfg(target_os = "linux")]
            {
                log_media_capabilities();
            }

            let slack_url = "https://app.slack.com/client"
                .parse::<Url>()
                .map_err(|e| AppError::InvalidUrl(e.to_string()))?;

            let window = WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::App("bootstrap/index.html".into()),
            )
            .title("Slackinux")
            .inner_size(1280.0, 800.0)
            .decorations(false)
            .transparent(true)
            .theme(match user_settings.theme_preference {
                settings::ThemePreference::System => None,
                settings::ThemePreference::Light => Some(tauri::Theme::Light),
                settings::ThemePreference::Dark => Some(tauri::Theme::Dark),
            })
            .build()
            .map_err(AppError::Tauri)?;

            let theme_preference = Arc::new(std::sync::Mutex::new(user_settings.theme_preference));

            #[cfg(target_os = "linux")]
            frame::apply_custom_frame(app.handle(), &window, theme_preference.clone());

            let download_dir = data_dir.join("downloads");
            std::fs::create_dir_all(&download_dir).ok();

            let notif_mgr = Arc::new(NotificationManager::new());
            notif_mgr.set_dnd(user_settings.dnd);
            zoom_level.store(user_settings.zoom_level, Ordering::Relaxed);

            // --- Tray (needed before renderer setup for title callback) ---
            let tray_show = MenuItemBuilder::with_id("tray_show", "Show Slackinux").build(app)?;
            let tray_quit =
                MenuItemBuilder::with_id("tray_quit", "Quit").accelerator("CmdOrCtrl+Q").build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .item(&tray_show)
                .item(&tray_quit)
                .build()?;

            let tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Slackinux")
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // --- Renderer ---
            let renderer = Arc::new(WebKitRenderer::new(window.clone(), download_dir));

            #[cfg(target_os = "linux")]
            {
                let tt = tray.clone();
                let handle = app.handle().clone();
                let win_title = window.clone();
                renderer.setup_linux(
                    move |title| {
                        let unread = parse_unread_count(title);
                        if unread > 0 {
                            let _: Result<(), _> = tt.set_tooltip(Some(format!("Slackinux ({unread})")));
                        } else {
                            let _: Result<(), _> = tt.set_tooltip(Some("Slackinux"));
                        }
                        let _ = win_title.set_title(title);
                    },
                    notif_mgr.clone(),
                    handle,
                );
            }

            let handle = app.handle().clone();
            let gpu_pref = Arc::new(std::sync::Mutex::new(user_settings.gpu_preference));
            handle.manage(AppState {
                renderer: renderer.clone(),
                notif_mgr: notif_mgr.clone(),
                data_dir: data_dir.clone(),
                zoom_level: zoom_level.clone(),
                gpu_preference: gpu_pref,
                theme_preference: theme_preference.clone(),
                auto_check_updates: Arc::new(AtomicBool::new(user_settings.auto_check_updates)),
                last_update_check_unix: Arc::new(AtomicI64::new(
                    user_settings.last_update_check_unix,
                )),
            });

            // --- App menu: Slack-desktop-style (File/Edit/View/History/Window/
            // Help) with the Slackinux extras (Account, Graphics) kept too. ---
            let quit_item = MenuItemBuilder::with_id("quit", "Quit")
                .accelerator("CmdOrCtrl+Q")
                .build(app)?;
            let edit_undo = MenuItemBuilder::with_id("edit_undo", "Undo")
                .accelerator("CmdOrCtrl+Z")
                .build(app)?;
            let edit_redo = MenuItemBuilder::with_id("edit_redo", "Redo")
                .accelerator("CmdOrCtrl+Shift+Z")
                .build(app)?;
            let edit_cut = MenuItemBuilder::with_id("edit_cut", "Cut")
                .accelerator("CmdOrCtrl+X")
                .build(app)?;
            let edit_copy = MenuItemBuilder::with_id("edit_copy", "Copy")
                .accelerator("CmdOrCtrl+C")
                .build(app)?;
            let edit_paste = MenuItemBuilder::with_id("edit_paste", "Paste")
                .accelerator("CmdOrCtrl+V")
                .build(app)?;
            let edit_select_all = MenuItemBuilder::with_id("edit_select_all", "Select All")
                .accelerator("CmdOrCtrl+A")
                .build(app)?;
            let reload = MenuItemBuilder::with_id("reload", "Reload")
                .accelerator("CmdOrCtrl+R")
                .build(app)?;
            let zoom_in = MenuItemBuilder::with_id("zoom_in", "Zoom In")
                .accelerator("CmdOrCtrl+Plus")
                .build(app)?;
            let zoom_out = MenuItemBuilder::with_id("zoom_out", "Zoom Out")
                .accelerator("CmdOrCtrl+-")
                .build(app)?;
            let zoom_reset = MenuItemBuilder::with_id("zoom_reset", "Actual Size")
                .accelerator("CmdOrCtrl+0")
                .build(app)?;
            let fullscreen =
                MenuItemBuilder::with_id("fullscreen", "Toggle Fullscreen")
                    .accelerator("F11")
                    .build(app)?;
            let history_back = MenuItemBuilder::with_id("history_back", "Back")
                .accelerator("Alt+Left")
                .build(app)?;
            let history_forward = MenuItemBuilder::with_id("history_forward", "Forward")
                .accelerator("Alt+Right")
                .build(app)?;
            let login_in_app =
                MenuItemBuilder::with_id("login_in_app", "Sign In to Slack").build(app)?;
            let login_browser = MenuItemBuilder::with_id(
                "login_browser",
                "Open Sign-In in Browser",
            )
            .build(app)?;
            let dnd_toggle = MenuItemBuilder::with_id("dnd_toggle", "Do Not Disturb")
                .accelerator("CmdOrCtrl+D")
                .build(app)?;
            let clear_cache = MenuItemBuilder::with_id("clear_cache", "Clear Cache & Restart")
                .build(app)?;
            let check_updates =
                MenuItemBuilder::with_id("check_updates", "Check for Updates…").build(app)?;
            let release_notes =
                MenuItemBuilder::with_id("release_notes", "Release Notes").build(app)?;
            let open_logs =
                MenuItemBuilder::with_id("open_logs", "Open Log Folder").build(app)?;
            let copy_diagnostics =
                MenuItemBuilder::with_id("copy_diagnostics", "Copy Diagnostic Info").build(app)?;
            let report_issue =
                MenuItemBuilder::with_id("report_issue", "Report an Issue…").build(app)?;
            let diagnostics_menu = SubmenuBuilder::new(app, "Diagnostics")
                .item(&open_logs)
                .item(&copy_diagnostics)
                .separator()
                .item(&report_issue)
                .build()?;
            let about = MenuItemBuilder::with_id("about", "About Slackinux").build(app)?;
            let win_minimize = MenuItemBuilder::with_id("win_minimize", "Minimize")
                .accelerator("CmdOrCtrl+M")
                .build(app)?;
            let win_maximize =
                MenuItemBuilder::with_id("win_maximize", "Maximize / Restore").build(app)?;

            let theme_menu = {
                use tauri::menu::CheckMenuItemBuilder;
                let theme_system = CheckMenuItemBuilder::with_id("theme_system", "System")
                    .checked(user_settings.theme_preference == settings::ThemePreference::System)
                    .build(app)?;
                let theme_light = CheckMenuItemBuilder::with_id("theme_light", "Light")
                    .checked(user_settings.theme_preference == settings::ThemePreference::Light)
                    .build(app)?;
                let theme_dark = CheckMenuItemBuilder::with_id("theme_dark", "Dark")
                    .checked(user_settings.theme_preference == settings::ThemePreference::Dark)
                    .build(app)?;
                SubmenuBuilder::with_id(app, "theme", "Theme")
                    .item(&theme_system)
                    .item(&theme_light)
                    .item(&theme_dark)
                    .build()?
            };

            #[cfg(target_os = "linux")]
            let graphics_menu = {
                use tauri::menu::CheckMenuItemBuilder;
                let gpu_auto = CheckMenuItemBuilder::with_id("gpu_auto", "Auto (recommended)")
                    .checked(user_settings.gpu_preference == settings::GpuPreference::Auto)
                    .build(app)?;
                let gpu_integrated =
                    CheckMenuItemBuilder::with_id("gpu_integrated", "Integrated GPU")
                        .checked(user_settings.gpu_preference == settings::GpuPreference::Integrated)
                        .build(app)?;
                let gpu_discrete =
                    CheckMenuItemBuilder::with_id("gpu_discrete", "Discrete GPU")
                        .checked(user_settings.gpu_preference == settings::GpuPreference::Discrete)
                        .build(app)?;
                let gpu_restart = MenuItemBuilder::with_id(
                    "gpu_restart",
                    "Restart to Apply Graphics Changes",
                )
                .build(app)?;
                SubmenuBuilder::with_id(app, "graphics", "Graphics")
                    .item(&gpu_auto)
                    .item(&gpu_integrated)
                    .item(&gpu_discrete)
                    .separator()
                    .item(&gpu_restart)
                    .build()?
            };

            let file_menu = SubmenuBuilder::new(app, "File")
                .item(&quit_item)
                .build()?;

            let edit_menu = SubmenuBuilder::new(app, "Edit")
                .item(&edit_undo)
                .item(&edit_redo)
                .separator()
                .item(&edit_cut)
                .item(&edit_copy)
                .item(&edit_paste)
                .separator()
                .item(&edit_select_all)
                .build()?;

            let view_menu = SubmenuBuilder::new(app, "View")
                .item(&zoom_in)
                .item(&zoom_out)
                .item(&zoom_reset)
                .separator()
                .item(&reload)
                .item(&fullscreen)
                .build()?;

            let history_menu = SubmenuBuilder::new(app, "History")
                .item(&history_back)
                .item(&history_forward)
                .build()?;

            let window_menu = SubmenuBuilder::new(app, "Window")
                .item(&win_minimize)
                .item(&win_maximize)
                .build()?;

            let help_menu = SubmenuBuilder::new(app, "Help")
                .item(&check_updates)
                .item(&release_notes)
                .separator()
                .item(&diagnostics_menu)
                .item(&about)
                .build()?;

            let account_menu = SubmenuBuilder::new(app, "Account")
                .item(&login_in_app)
                .item(&login_browser)
                .separator()
                .item(&dnd_toggle)
                .item(&clear_cache)
                .build()?;

            let menu = MenuBuilder::new(app)
                .item(&file_menu)
                .item(&edit_menu)
                .item(&view_menu)
                .item(&history_menu)
                .item(&window_menu)
                .item(&theme_menu)
                .item(&help_menu)
                .build()?;
            app.set_menu(menu)?;

            // Slackinux extras go after Help so the primary Slack menu keeps its
            // standard shape.
            if let Some(menu) = app.menu() {
                #[cfg(target_os = "linux")]
                menu.append(&graphics_menu)?;
                menu.append(&account_menu)?;
            }

            // --- Close to tray ---
            let window_clone = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window_clone.hide();
                }
            });

            // --- Apply saved zoom ---
            let saved_zoom = f64::from(user_settings.zoom_level) / 10.0;
            let _ = renderer.set_zoom_level(saved_zoom);
            info!("zoom: {saved_zoom:.1}x (saved)");

            // --- Navigation ---
            let initial_url = deep_links::slack_url_from_args(&env::args().collect::<Vec<_>>())
                .unwrap_or(slack_url);
            info!("navigating to Slack URL");
            renderer.navigate(initial_url.as_str())?;

            updates::schedule_startup_check(app.handle().clone());

            info!("webview created successfully");
            Ok(())
        })
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            match id {
                "edit_undo" | "edit_redo" | "edit_cut" | "edit_copy" | "edit_paste"
                | "edit_select_all" => {
                    let state = app.state::<AppState>();
                    let cmd = match id {
                        "edit_undo" => "undo",
                        "edit_redo" => "redo",
                        "edit_cut" => "cut",
                        "edit_copy" => "copy",
                        "edit_paste" => "paste",
                        _ => "selectAll",
                    };
                    let _ = state.renderer.eval(&format!(
                        "document.execCommand('{cmd}')"
                    ));
                }
                "history_back" => {
                    let state = app.state::<AppState>();
                    let _ = state.renderer.eval("history.back()");
                }
                "history_forward" => {
                    let state = app.state::<AppState>();
                    let _ = state.renderer.eval("history.forward()");
                }
                "fullscreen" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.set_fullscreen(!window.is_fullscreen().unwrap_or(false));
                    }
                }
                "quit" => {
                    app.exit(0);
                }
                "reload" => {
                    let state = app.state::<AppState>();
                    let _ = state.renderer.reload();
                }
                "zoom_in" | "zoom_out" | "zoom_reset" => {
                    let current = zoom_level_menu.load(Ordering::Relaxed);
                    let new_level = match id {
                        "zoom_in" => current.saturating_add(1).min(30),
                        "zoom_out" => current.saturating_sub(1).max(3),
                        "zoom_reset" => 10,
                        _ => current,
                    };
                    zoom_level_menu.store(new_level, Ordering::Relaxed);
                    let level = f64::from(new_level) / 10.0;
                    let state = app.state::<AppState>();
                    let _ = state.renderer.set_zoom_level(level);
                    state.settings().save(&state.data_dir);
                    info!("zoom: {level:.1}x");
                }
                "login_in_app" => {
                    let state = app.state::<AppState>();
                    info!("opening Slack sign-in in the app");
                    let _ = state.renderer.navigate("https://app.slack.com/signin");
                }
                "login_browser" => {
                    info!("opening Slack sign-in in the system browser");
                    if let Err(err) = open::that_detached("https://slack.com/signin") {
                        error!("failed to open Slack sign-in in browser: {err}");
                    }
                }
                "check_updates" => {
                    updates::check_for_updates(app.clone(), updates::UpdateCheckReason::Manual);
                }
                "release_notes" => {
                    if let Err(err) =
                        open::that_detached("https://github.com/Knownassa/Slackinux/releases")
                    {
                        error!("failed to open release notes: {err}");
                    }
                }
                "open_logs" => diagnostics::open_log_folder(app),
                "copy_diagnostics" => diagnostics::copy_support_report(app),
                "report_issue" => diagnostics::report_issue(app),
                "dnd_toggle" => {
                    let state = app.state::<AppState>();
                    let dnd = !state.notif_mgr.is_dnd();
                    state.notif_mgr.set_dnd(dnd);
                    state.settings().save(&state.data_dir);
                }
                "clear_cache" => {
                    let state = app.state::<AppState>();
                    let _ = state.renderer.clear_cache();
                    if let Ok(exe) = std::env::current_exe() {
                        let _ = std::process::Command::new(exe).spawn();
                    }
                    let app_clone = app.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(800));
                        app_clone.exit(0);
                    });
                }
                "gpu_auto" | "gpu_integrated" | "gpu_discrete" => {
                    use tauri_plugin_dialog::DialogExt;
                    let state = app.state::<AppState>();
                    let pref = match id {
                        "gpu_auto" => settings::GpuPreference::Auto,
                        "gpu_integrated" => settings::GpuPreference::Integrated,
                        _ => settings::GpuPreference::Discrete,
                    };
                    *state.gpu_preference.lock().unwrap() = pref;
                    state.settings().save(&state.data_dir);
                    set_graphics_checks(app, pref);
                    info!("graphics preference: {pref} (applies after restart)");
                    app.dialog()
                        .message(format!(
                            "Graphics preference set to {pref}.\n\nIt takes effect on the next \
                             launch of Slackinux."
                        ))
                        .title("Slackinux — Graphics")
                        .kind(tauri_plugin_dialog::MessageDialogKind::Info)
                        .show(|_| {});
                }
                "theme_system" | "theme_light" | "theme_dark" => {
                    let preference = match id {
                        "theme_light" => settings::ThemePreference::Light,
                        "theme_dark" => settings::ThemePreference::Dark,
                        _ => settings::ThemePreference::System,
                    };
                    let state = app.state::<AppState>();
                    *state.theme_preference.lock().unwrap() = preference;
                    state.settings().save(&state.data_dir);
                    set_theme_checks(app, preference);
                    if let Some(window) = app.get_webview_window("main") {
                        #[cfg(target_os = "linux")]
                        frame::set_theme(&window, preference);
                    }
                    info!("theme preference: {preference}");
                }
                "gpu_restart" => {
                    if let Ok(exe) = std::env::current_exe() {
                        let _ = std::process::Command::new(exe).spawn();
                    }
                    let app_clone = app.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(800));
                        app_clone.exit(0);
                    });
                }
                "tray_show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "tray_quit" => {
                    app.exit(0);
                }
                "about" => {
                    use tauri_plugin_dialog::DialogExt;
                    app.dialog()
                        .message(format!(
                            "Slackinux v{version}\n\nAn unofficial, resource-conscious \
                             Linux desktop shell for Slack Web.\n\nBuilt with Tauri 2 + WebKitGTK.\n\n\
                             Published by Knownassa.\n\nNot affiliated with or endorsed by Slack Technologies."
                        ))
                        .title("About Slackinux")
                        .kind(tauri_plugin_dialog::MessageDialogKind::Info)
                        .show(|_| {});
                }
                "win_minimize" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.minimize();
                    }
                }
                "win_maximize" => {
                    if let Some(window) = app.get_webview_window("main") {
                        if window.is_maximized().unwrap_or(false) {
                            let _ = window.unmaximize();
                        } else {
                            let _ = window.maximize();
                        }
                    }
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            error!("application error: {e}");
            std::process::exit(1);
        });

    Ok(())
}

#[cfg(target_os = "linux")]
extern "C" {
    fn webkit_get_major_version() -> u32;
    fn webkit_get_minor_version() -> u32;
    fn webkit_get_micro_version() -> u32;
}

#[cfg(target_os = "linux")]
fn log_webkit_version() {
    unsafe {
        let major = webkit_get_major_version();
        let minor = webkit_get_minor_version();
        let micro = webkit_get_micro_version();
        info!("WebKitGTK: {major}.{minor}.{micro}");
        info!("WebRTC: available via WebKitGTK settings");
    }
}

#[cfg(target_os = "linux")]
fn log_media_capabilities() {
    info!("PipeWire: {}", detect_pipewire());
    info!("Portal: {}", detect_portal());
}

#[cfg(target_os = "linux")]
fn detect_pipewire() -> &'static str {
    let pw_check = std::process::Command::new("pw-cli")
        .arg("info")
        .output()
        .is_ok();
    if pw_check {
        return "available";
    }
    let pa_check = std::process::Command::new("pactl")
        .arg("info")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.contains("PipeWire"))
        .unwrap_or(false);
    if pa_check {
        "available"
    } else {
        "not detected"
    }
}

#[cfg(target_os = "linux")]
fn detect_portal() -> &'static str {
    let portal = env::var("XDG_DESKTOP_PORTAL").unwrap_or_default();
    if !portal.is_empty() {
        return "available";
    }
    let has_dbus = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.portal.Desktop",
            "--type=method_call",
            "--print-reply",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.DBus.Peer.Ping",
        ])
        .output();
    if has_dbus.is_ok() {
        "available"
    } else {
        "not detected"
    }
}

fn parse_unread_count(title: &str) -> u32 {
    let title = title.trim();
    if title.starts_with('(') {
        if let Some(end) = title.find(')') {
            return title[1..end].parse().unwrap_or(0);
        }
    }
    0
}

fn handle_slack_launch_args(app: &tauri::AppHandle, args: &[String]) {
    let Some(url) = deep_links::slack_url_from_args(args) else {
        return;
    };
    let Some(state) = app.try_state::<AppState>() else {
        warn!("Slack callback arrived before the renderer was ready");
        return;
    };
    info!("opening Slack browser callback in Slackinux");
    if let Err(err) = state.renderer.navigate(url.as_str()) {
        error!("failed to open Slack browser callback: {err}");
    }
}

/// Keeps the Graphics menu check items consistent with the chosen preference.
fn set_graphics_checks(app: &tauri::AppHandle, pref: settings::GpuPreference) {
    use tauri::menu::MenuItemKind;
    let Some(menu) = app.menu() else {
        return;
    };
    let Some(MenuItemKind::Submenu(graphics)) = menu.get("graphics") else {
        return;
    };
    let states = [
        ("gpu_auto", pref == settings::GpuPreference::Auto),
        (
            "gpu_integrated",
            pref == settings::GpuPreference::Integrated,
        ),
        ("gpu_discrete", pref == settings::GpuPreference::Discrete),
    ];
    for (id, checked) in states {
        if let Some(MenuItemKind::Check(item)) = graphics.get(id) {
            let _ = item.set_checked(checked);
        }
    }
}

/// Keeps the Theme menu check items mutually exclusive.
fn set_theme_checks(app: &tauri::AppHandle, pref: settings::ThemePreference) {
    use tauri::menu::MenuItemKind;
    let Some(menu) = app.menu() else {
        return;
    };
    let Some(MenuItemKind::Submenu(theme)) = menu.get("theme") else {
        return;
    };
    let states = [
        ("theme_system", pref == settings::ThemePreference::System),
        ("theme_light", pref == settings::ThemePreference::Light),
        ("theme_dark", pref == settings::ThemePreference::Dark),
    ];
    for (id, checked) in states {
        if let Some(MenuItemKind::Check(item)) = theme.get(id) {
            let _ = item.set_checked(checked);
        }
    }
}

fn redact_path(path: &str) -> String {
    let home = env::var("HOME").unwrap_or_else(|_| "/home".into());
    path.replace(&home, "~")
}

fn migrate_legacy_data_dir(data_dir: &std::path::Path) {
    // Historical identifiers that may have left data behind.
    let legacy_ids = ["com.swiftwire.app", "com.swiftwire.desktop"];
    let Some(parent) = data_dir.parent() else {
        return;
    };
    for id in legacy_ids {
        let legacy = parent.join(id);
        if legacy == data_dir || !legacy.is_dir() {
            continue;
        }

        let new_settings = data_dir.join("settings.json");
        let old_settings = legacy.join("settings.json");
        if !new_settings.exists() && old_settings.exists() {
            if let Ok(content) = std::fs::read_to_string(&old_settings) {
                let _ = std::fs::write(&new_settings, content);
                info!("migrated settings from legacy data dir {id}");
            }
        }

        let new_downloads = data_dir.join("downloads");
        let old_downloads = legacy.join("downloads");
        if !new_downloads.exists()
            && old_downloads.exists()
            && std::fs::rename(&old_downloads, &new_downloads).is_ok()
        {
            info!("migrated downloads from legacy data dir {id}");
        }
    }
}
