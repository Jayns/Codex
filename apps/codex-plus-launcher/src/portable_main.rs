#![cfg_attr(windows, windows_subsystem = "windows")]

//! Entry point for the portable launcher build: a single self-contained
//! folder (exe + `codex_app\` + `config.ini`) that can be copied to another
//! machine and run without an installer. Unlike the installed launcher
//! (`main.rs`), configuration lives in `config.ini` next to the executable
//! instead of `%USERPROFILE%`, and is edited through a small native dialog
//! rather than the separate manager app.
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
    let force_config = std::env::args().skip(1).any(|arg| {
        let arg = arg.trim();
        arg == "--config" || arg == "--settings"
    });

    let config_path = codex_plus_core::portable::default_portable_config_path();
    let mut existing = PortableConfig::load(&config_path);

    // Pre-fill the Codex App path with the bundled `codex_app` folder next to
    // the executable when the user hasn't set one, so the dialog shows a sane
    // default instead of an empty field.
    if existing.codex_app_dir.trim().is_empty() {
        existing.codex_app_dir = codex_plus_core::portable::default_portable_app_dir()
            .to_string_lossy()
            .into_owned();
    }

    // Silent fast path: already configured and not explicitly asked to edit.
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
        edited
    };

    let app_dir = if config.codex_app_dir.trim().is_empty() {
        codex_plus_core::portable::default_portable_app_dir()
    } else {
        std::path::PathBuf::from(&config.codex_app_dir)
    };

    let settings = config.to_backend_settings();
    let hooks = LauncherHooks::default();
    hooks.apply_active_relay_profile(&settings).await?;

    let options = LaunchOptions {
        app_dir: Some(app_dir),
        debug_port: config.debug_port,
        ..LaunchOptions::default()
    };
    let handle = launch_and_inject_with_hooks(options, &hooks).await?;

    // Apply this exe's embedded icon to the Codex window's taskbar entry. The
    // core only does this for packaged (MSIX) launches; the portable loose-folder
    // Codex.exe otherwise shows a blank/default taskbar icon.
    #[cfg(windows)]
    apply_window_icon_to_codex();

    // Keep this process (and with it the helper + CDP bridge that back the
    // injected enhancements) alive until Codex exits. The core wait detects the
    // loose-folder Codex.exe used by the portable build, so no special handling
    // is needed here.
    handle.wait_for_codex_exit().await?;
    Ok(())
}

/// Polls for the Codex window and applies this launcher exe's embedded icon to
/// it (taskbar + window), retrying for ~15s while Codex finishes starting.
#[cfg(windows)]
fn apply_window_icon_to_codex() {
    let icon_path = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("codex.exe"));
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
