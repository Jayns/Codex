#![cfg_attr(windows, windows_subsystem = "windows")]

//! Entry point for the portable launcher build: a single self-contained
//! folder (exe + `config.ini`, plus a bundled `codex_app\` on Windows) that
//! can be copied to another machine and run without an installer. Unlike the
//! installed launcher (`main.rs`), configuration lives in `config.ini` next
//! to the executable instead of `%USERPROFILE%`, and is edited through a
//! small native dialog rather than the separate manager app.
//!
//! On macOS there is no bundled Codex App copy: the Codex App path defaults
//! to whatever `Codex.app` is already installed under `/Applications` /
//! `~/Applications` (see `platform_default_app_dir` below).
//!
//! The underlying launch/inject mechanism (CDP bridge, relay config) is
//! unchanged and reused as-is from `codex_plus_launcher::LauncherHooks`.
//!
//! Dialog behaviour: the config window only appears on first run (or when
//! `config.ini` is missing required fields), so that once configured the exe
//! launches Codex silently. Pass `--config` to force the dialog open for
//! editing the relay settings later.

use anyhow::Result;
use codex_plus_core::launcher::{LaunchHooks, LaunchOptions, launch_and_inject_with_hooks};
use codex_plus_core::portable::PortableConfig;
use codex_plus_launcher::LauncherHooks;

#[tokio::main]
async fn main() -> Result<()> {
    // The portable exe has no console (windows_subsystem = "windows" /
    // LSUIElement .app), so a failure that only reaches stderr looks like
    // "nothing happened". Surface it in a native error dialog instead.
    if let Err(error) = run().await {
        codex_plus_core::portable_dialog::show_portable_error_dialog(&format!("{error:#}"));
        return Err(error);
    }
    Ok(())
}

async fn run() -> Result<()> {
    let force_config = std::env::args().skip(1).any(|arg| {
        let arg = arg.trim();
        arg == "--config" || arg == "--settings"
    });

    let config_path = codex_plus_core::portable::default_portable_config_path();
    let mut existing = PortableConfig::load(&config_path);

    // Pre-fill the Codex App path with a sane platform default when the user
    // hasn't set one, so the dialog shows something useful instead of an
    // empty field.
    if existing.codex_app_dir.trim().is_empty() {
        if let Some(app_dir) = platform_default_app_dir() {
            existing.codex_app_dir = app_dir.to_string_lossy().into_owned();
        }
    }

    // Silent fast path: already configured and not explicitly asked to edit.
    let mut configured_via_dialog = false;
    let config = if existing.is_complete() && !force_config {
        existing
    } else {
        let Some(edited) =
            codex_plus_core::portable_dialog::show_portable_config_dialog(&existing)?
        else {
            // User closed/cancelled the dialog without saving: do not launch.
            return Ok(());
        };
        edited.save(&config_path)?;
        if !edited.is_complete() {
            return Ok(());
        }
        configured_via_dialog = true;
        edited
    };

    let app_dir = if config.codex_app_dir.trim().is_empty() {
        platform_default_app_dir().ok_or_else(|| {
            anyhow::anyhow!(
                "未找到已安装的 ChatGPT App，请运行 `chatgpt-launcher --config` 手动选择安装路径"
            )
        })?
    } else {
        std::path::PathBuf::from(&config.codex_app_dir)
    };

    let settings = config.to_backend_settings();
    let hooks = LauncherHooks::portable();
    hooks.apply_active_relay_profile(&settings).await?;

    let options = LaunchOptions {
        app_dir: Some(app_dir.clone()),
        debug_port: config.debug_port,
        ..LaunchOptions::default()
    };
    let handle = launch_and_inject_with_hooks(options, &hooks).await?;

    // Restore Codex's own taskbar icon. The core only sets a window icon for
    // packaged (MSIX) launches; the portable loose-folder Codex.exe otherwise
    // shows a blank/default taskbar icon.
    #[cfg(windows)]
    apply_window_icon_to_codex();

    // When the user just configured and launched, drop a desktop shortcut so
    // they can relaunch without opening the portable folder. Created only on
    // the configure path and only if one doesn't already exist, so a user who
    // deletes it isn't fought.
    #[cfg(windows)]
    if configured_via_dialog {
        let _ = create_desktop_shortcut(&app_dir);
    }
    #[cfg(not(windows))]
    let _ = configured_via_dialog;

    // Keep this process (and with it the helper + CDP bridge that back the
    // injected enhancements) alive until Codex exits. The core wait detects the
    // loose-folder Codex.exe used by the portable build, so no special handling
    // is needed here.
    handle.wait_for_codex_exit().await?;
    Ok(())
}

/// Platform-appropriate default Codex App location: the bundled `codex_app`
/// folder next to the executable on Windows (the portable package ships its
/// own copy there), or the already-installed `Codex.app` under
/// `/Applications` / `~/Applications` on macOS (the portable package does not
/// bundle its own copy on that platform).
fn platform_default_app_dir() -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        // Prefer an already-installed official ChatGPT/Codex app so the
        // portable launcher reuses it instead of requiring its own bundled
        // copy; fall back to the bundled `codex_app` folder next to the exe
        // (if it actually exists), and only leave this empty when neither is
        // found, so the dialog prompts the user to pick a path.
        if let Some(app_dir) = codex_plus_core::app_paths::resolve_codex_app_dir(None) {
            return Some(app_dir);
        }
        let bundled = codex_plus_core::portable::default_portable_app_dir();
        if codex_plus_core::app_paths::build_codex_executable(&bundled).exists() {
            return Some(bundled);
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        codex_plus_core::app_paths::find_macos_codex_app_default()
    }
}

/// Creates a "ChatGPT Launcher" desktop shortcut to this launcher (with the
/// original Codex App icon), unless one already exists. Best-effort: failures
/// are ignored.
#[cfg(windows)]
fn create_desktop_shortcut(app_dir: &std::path::Path) -> anyhow::Result<()> {
    let Some(desktop) = codex_plus_core::windows_desktop_dir() else {
        return Ok(());
    };
    let shortcut_path = desktop.join("ChatGPT Launcher.lnk");
    if shortcut_path.exists() {
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    let working_directory = exe.parent().map(|parent| parent.to_path_buf());
    let icon = [
        app_dir.join("app").join("resources").join("icon.ico"),
        app_dir.join("resources").join("icon.ico"),
    ]
    .into_iter()
    .find(|candidate| candidate.exists());

    codex_plus_core::windows_create_shortcut(&codex_plus_core::ShortcutSpec {
        path: shortcut_path,
        target: exe.clone(),
        arguments: String::new(),
        working_directory,
        description: "ChatGPT Launcher".to_string(),
        icon,
        show_minimized: false,
    })
}

/// The original Codex App icon, bundled into the launcher so we don't depend on
/// extracting it from the running Codex.exe (which is timing- and
/// resource-layout-sensitive). Sourced from `codex_app/app/resources/icon.ico`.
#[cfg(windows)]
const CODEX_APP_ICON: &[u8] = include_bytes!("../assets/codex-app-icon.ico");

/// Polls for the Codex window and applies the bundled original Codex App icon
/// to it (taskbar + window), retrying for ~15s while Codex finishes starting.
#[cfg(windows)]
fn apply_window_icon_to_codex() {
    let Some(icon_path) = materialize_bundled_icon() else {
        return;
    };
    tokio::spawn(async move {
        for _ in 0..30 {
            let mut applied = false;
            for pid in codex_plus_core::watcher::find_codex_processes() {
                if codex_plus_core::windows_apply_codexplusplus_icon_to_process_window(
                    pid,
                    icon_path.clone(),
                ) {
                    applied = true;
                }
            }
            if applied {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });
}

/// Writes the bundled icon to a stable temp path once so the existing
/// file-based icon loader can use it. Returns the path, or `None` on failure.
#[cfg(windows)]
fn materialize_bundled_icon() -> Option<std::path::PathBuf> {
    let path = std::env::temp_dir().join("codex-portable-app-icon.ico");
    let needs_write = match std::fs::metadata(&path) {
        Ok(meta) => meta.len() != CODEX_APP_ICON.len() as u64,
        Err(_) => true,
    };
    if needs_write && std::fs::write(&path, CODEX_APP_ICON).is_err() {
        return None;
    }
    Some(path)
}
