//! zcode-gpui: minimal native GPUI frontend for the ZCode agent backend.
//!
//! Native chat, settings and local tool surfaces. The backend is an external process
//! (`app-server --stdio`) and driven over the V4 wire protocol, so backend
//! updates from upstream keep flowing without frontend changes.
//!
//! Source is organised as vertical slices: each folder owns one feature end
//! to end (state, effects and views). `app` composes the slices into the
//! window; `shared` holds the theme and renderers several slices reuse.

// The Android build (apps/zcode-gpui-android) enters through `open_window`, so
// desktop launch paths (CLI args, acceptance runs, single-instance) go unused.
#![cfg_attr(target_os = "android", allow(dead_code))]

mod app;
mod backend;
mod browser_bridge;
mod composer;
mod conversation;
mod files;
mod provider_settings;
pub mod remote;
mod review;
mod secrets;
mod sessions;
mod shared;
mod system_prompt;
mod terminal;
mod transcript;
mod voice;

use app::root::RootView;
use app::store::AppState;
use backend::launcher::resolve_candidates;
use backend::workspace::discover_workspaces;
use gpui::{App, AppContext, TitlebarOptions, WindowOptions};
use std::path::PathBuf;

pub use shared::app_assets::AppAssets;

/// Settings-derived state the first window needs, loaded before the platform
/// application starts (theme and font size are process globals).
pub struct Startup {
    workspace: PathBuf,
    /// The workspace was named on the command line (or is the isolated
    /// scratch root), so it joins the project list; a cwd fallback does not.
    explicit_workspace: bool,
    settings: shared::settings::AppSettings,
    settings_section: Option<app::settings::SettingsSection>,
    remote_client: bool,
}

impl Startup {
    /// Serve every workspace from the paired desktop instead of local agents
    /// (the Android build).
    pub fn remote_client(mut self) -> Self {
        self.remote_client = true;
        self
    }
}

/// Load settings, data paths, theme and font size for `workspace`.
pub fn load_startup(workspace: PathBuf) -> Startup {
    let settings = shared::settings::load_settings();
    shared::data_paths::initialize(settings.data_base_dir.as_deref());
    if let Some(font_size) = settings.ui_font_size {
        shared::theme::set_ui_font_size(font_size);
    }
    Startup {
        workspace,
        explicit_workspace: false,
        settings,
        settings_section: None,
        remote_client: false,
    }
}

#[cfg(not(target_os = "android"))]
fn chrono_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// Desktop entry: CLI arguments, panic log, single-instance guard, then the
/// platform application from `gpui_platform`.
#[cfg(not(target_os = "android"))]
pub fn desktop_main() {
    // GUI panics vanish with the window; persist them for diagnosis.
    let panic_log = std::env::temp_dir().join("zcode-gpui-panic.log");
    // Keep the default hook so panics still reach stderr under `cargo run`.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&panic_log)
        {
            let _ = writeln!(f, "=== panic at {} ===", chrono_now());
            // Panic payloads can format arbitrary state; never persist secrets.
            let _ = writeln!(f, "{}", shared::redact::scrub(&info.to_string()));
            let _ = writeln!(f, "{:?}", std::backtrace::Backtrace::force_capture());
        }
        default_hook(info);
    }));

    let mut workspace: Option<PathBuf> = None;
    let mut isolated_settings = false;
    let mut settings_section = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => workspace = args.next().map(PathBuf::from),
            "--isolated-settings" => isolated_settings = true,
            "--settings-section" => {
                settings_section = args
                    .next()
                    .and_then(|value| app::settings::SettingsSection::parse(&value));
                if settings_section.is_none() {
                    eprintln!("--settings-section requires a known Settings section");
                    std::process::exit(2);
                }
            }
            other => {
                if workspace.is_none() {
                    workspace = Some(PathBuf::from(other));
                }
            }
        }
    }
    let acceptance =
        shared::acceptance::AcceptanceRequest::read(isolated_settings).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(2);
        });
    if isolated_settings {
        let isolated = acceptance.open_isolated().unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(2);
        });
        workspace = Some(isolated.workspace());
        isolated.install();
    }
    let explicit_workspace = workspace.is_some();
    let workspace = workspace
        .and_then(|p| p.canonicalize().ok())
        .map(crate::backend::workspace::plain_path)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    // M5: Load settings, locale, theme, and font size on launch
    let mut startup = load_startup(workspace.clone());
    startup.settings_section = settings_section;
    startup.explicit_workspace = explicit_workspace;

    // M6: Single-instance guard and forward-to-first launch request. The mutex
    // name is overridable so a preview build can run beside the primary app.
    let instance_msg = shared::os::single_instance::InstanceMessage {
        action: "activate".into(),
        workspace: Some(workspace.to_string_lossy().into_owned()),
    };
    let _instance_guard = if shared::isolation::active().is_some() {
        // 通用 IPC 端口文件仍解析真实 HOME；scratch 已持有独占文件锁，不能触碰真实用户目录。
        None
    } else {
        let mutex_key = std::env::var("ZCODE_GPUI_INSTANCE_MUTEX")
            .unwrap_or_else(|_| "Local\\ZCodeGPUI_SingleInstance_Mutex".into());
        match shared::os::single_instance::try_acquire_single_instance(
            &mutex_key,
            instance_msg,
            |_msg| {
                // Primary instance received launch message from secondary instance
            },
        ) {
            shared::os::single_instance::InstanceRole::Secondary => {
                // Already forwarded to running primary instance, exit cleanly
                return;
            }
            shared::os::single_instance::InstanceRole::Primary(guard) => Some(guard),
        }
    };

    // gpui_platform::application() replaces the removed gpui::Application::new
    // (zed #restructure); AppAssets serves ely's embedded fonts + icons (plus
    // app-local icons), and
    // init() registers fonts, the theme and Tab focus bindings. Call it first.
    gpui_platform::application()
        .with_assets(AppAssets)
        .run(move |cx: &mut App| open_window(cx, startup));
}

/// Open the GCode window. Shared by the desktop and Android entries; call it
/// from the platform application's `run` callback.
pub fn open_window(cx: &mut App, startup: Startup) {
    let Startup {
        workspace,
        explicit_workspace,
        settings,
        settings_section,
        remote_client,
    } = startup;
    ely_gpui_component::init(cx);
    shared::theme::register_fonts(cx);
    shared::preferences::Preferences::install(settings, cx);
    shared::acceptance::apply_runtime(cx);
    // One agent per known workspace (desktop parity: processes are keyed by
    // workspace key; the project list comes from ~/.zcode/v2/setting.json).
    // The remote client gets its workspaces from the desktop instead.
    let (workspaces, candidates) = if remote_client {
        (Vec::new(), Vec::new())
    } else {
        (
            discover_workspaces(explicit_workspace.then_some(workspace.as_path())),
            resolve_candidates(&workspace),
        )
    };
    let window_state = shared::window_state::load_window_state();
    let initial_bounds = window_state.to_window_bounds(cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(initial_bounds),
            titlebar: Some(TitlebarOptions {
                title: Some("GCode".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let state = cx.new(|cx| {
                if remote_client {
                    AppState::new_remote(cx)
                } else {
                    AppState::new(workspaces, candidates, cx)
                }
            });
            let root = cx.new(|cx| {
                let mut root = RootView::new(state, cx);
                root.restore_open_folders(cx);
                if let Some(section) = settings_section {
                    root.open_settings_section(section, window, cx);
                }
                root
            });
            if !remote_client {
                // Voice's "only when unfocused" rule needs the window's activation.
                let state = root.read(cx).state.clone();
                root.update(cx, |_, cx| {
                    cx.observe_window_activation(window, move |_, window, cx| {
                        let active = window.is_window_active();
                        state.update(cx, |s, _| s.set_voice_window_active(active));
                    })
                    .detach();
                });
            }
            let focus = root.read(cx).state.read(cx).composer.read(cx).focus.clone();
            // 手机端 composer 获得焦点就会弹出软键盘；启动时不抢焦点，只在用户点按输入框时弹出。
            if settings_section.is_none() && !remote_client {
                window.focus(&focus, cx);
            }
            root
        },
    )
    .expect("failed to open window");
    cx.activate(true);
}
