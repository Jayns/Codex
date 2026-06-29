#![cfg_attr(windows, windows_subsystem = "windows")]

use anyhow::{Context, Result};
use codex_plus_core::launcher::{LaunchHooks, LaunchOptions, launch_and_inject_with_hooks};
use codex_plus_launcher::LauncherHooks;
use serde_json::json;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let helper_only = args.iter().any(|arg| arg == "--helper-only");
    let options = parse_launch_options(args.iter());
    if helper_only {
        let hooks = LauncherHooks::default();
        hooks.start_helper(options.helper_port).await?;
        std::future::pending::<()>().await;
        hooks.shutdown_helper(options.helper_port).await;
        return Ok(());
    }
    let Some(_guard) = acquire_single_instance_guard(options.debug_port)? else {
        activate_existing_codex_app(&options).await?;
        return Ok(());
    };
    tokio::spawn(async {
        let _ = notify_manager_when_update_available().await;
    });
    let hooks = LauncherHooks::default();
    let handle = launch_and_inject_with_hooks(options, &hooks).await?;
    handle.wait_for_codex_exit().await?;
    Ok(())
}

fn acquire_single_instance_guard(
    debug_port: u16,
) -> anyhow::Result<Option<codex_plus_core::ports::LoopbackPortGuard>> {
    acquire_single_instance_guard_with_retry(debug_port, true)
}

fn acquire_single_instance_guard_with_retry(
    debug_port: u16,
    allow_stale_recovery: bool,
) -> anyhow::Result<Option<codex_plus_core::ports::LoopbackPortGuard>> {
    match try_acquire_single_instance_guard() {
        Ok(guard) => {
            if let Some(fallback_lock_path) = guard.fallback_path() {
                log_launcher_guard_fallback(fallback_lock_path);
            }
            Ok(Some(guard))
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            log_launcher_already_running(debug_port);
            Ok(None)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            log_launcher_already_running(debug_port);
            if allow_stale_recovery && should_recover_stale_launcher(debug_port) {
                codex_plus_core::watcher::stop_launcher_processes();
                std::thread::sleep(std::time::Duration::from_millis(250));
                return acquire_single_instance_guard_with_retry(debug_port, false);
            }
            Ok(None)
        }
        Err(error) => Err(error)
            .with_context(|| {
                format!(
                    "failed to acquire launcher guard port {}",
                    codex_plus_core::ports::launcher_guard_port()
                )
            })
            .map(Some),
    }
}

fn try_acquire_single_instance_guard() -> std::io::Result<codex_plus_core::ports::LoopbackPortGuard>
{
    codex_plus_core::ports::acquire_resilient_loopback_port_guard(
        codex_plus_core::ports::launcher_guard_port(),
    )
}

fn log_launcher_guard_fallback(fallback_lock_path: &Path) {
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "launcher.guard_fallback",
        json!({
            "requested_guard_port": codex_plus_core::ports::launcher_guard_port(),
            "fallback_lock_path": fallback_lock_path
        }),
    );
}

fn should_recover_stale_launcher(debug_port: u16) -> bool {
    let has_codex_process = !codex_plus_core::watcher::find_codex_processes().is_empty();
    let cdp_listening = codex_plus_core::watcher::cdp_listening(debug_port);
    let recover =
        codex_plus_core::watcher::should_recover_stale_launcher(has_codex_process, cdp_listening);
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "launcher.stale_recovery_check",
        json!({
            "debug_port": debug_port,
            "has_codex_process": has_codex_process,
            "cdp_listening": cdp_listening,
            "recover": recover
        }),
    );
    recover
}

async fn activate_existing_codex_app(options: &LaunchOptions) -> anyhow::Result<()> {
    let hooks = LauncherHooks::default();
    let settings = hooks.load_settings().await?;
    let app_dir = hooks.resolve_app_dir(options.app_dir.as_deref(), &settings)?;
    let launch_result = hooks
        .launch_codex(
            &app_dir,
            options.debug_port,
            &settings,
            &settings.codex_extra_args,
        )
        .await;
    if settings.enhancements_enabled {
        hooks.start_helper(options.helper_port).await?;
    }
    let process_ids = codex_plus_core::watcher::find_codex_processes();
    let mut activated = false;
    #[cfg(windows)]
    {
        for process_id in &process_ids {
            if codex_plus_core::windows_activate_process_window(*process_id) {
                activated = true;
                break;
            }
        }
    }
    let injection_ready = if settings.enhancements_enabled {
        hooks
            .ensure_injection(options.debug_port, options.helper_port, &app_dir)
            .await
    } else {
        false
    };
    if injection_ready {
        hooks
            .start_bridge_watchdog(options.debug_port, options.helper_port)
            .await?;
        hooks.write_status("running").await;
    } else if settings.enhancements_enabled {
        hooks.write_status("running_degraded").await;
    }
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "launcher.activate_existing_codex",
        json!({
            "app_dir": app_dir.to_string_lossy(),
            "debug_port": options.debug_port,
            "helper_port": options.helper_port,
            "process_ids": process_ids,
            "activated": activated,
            "injection_ready": injection_ready,
            "launch_ok": launch_result.is_ok(),
            "launch_error": launch_result.as_ref().err().map(|error| error.to_string())
        }),
    );
    launch_result.map(|_| ())
}

fn log_launcher_already_running(debug_port: u16) {
    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
        "launcher.already_running",
        json!({
            "guard_port": codex_plus_core::ports::launcher_guard_port(),
            "debug_port": debug_port
        }),
    );
}

async fn notify_manager_when_update_available() -> anyhow::Result<bool> {
    let update =
        codex_plus_core::update::check_for_update(codex_plus_core::version::VERSION).await?;
    if !update.update_available {
        return Ok(false);
    }
    open_manager_with_update_prompt()?;
    Ok(true)
}

fn open_manager_with_update_prompt() -> anyhow::Result<()> {
    let manager_path = codex_plus_launcher::manager_exe_path();
    let mut command = std::process::Command::new(&manager_path);
    command.arg("--show-update");
    #[cfg(windows)]
    {
        command.creation_flags(codex_plus_core::windows_create_no_window());
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("启动管理工具失败：{error}"))
}

fn parse_launch_options<I, S>(args: I) -> LaunchOptions
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut options = LaunchOptions::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_ref() {
            "--app-path" => {
                if let Some(value) = iter.next() {
                    let value = value.as_ref().trim();
                    if !value.is_empty() {
                        options.app_dir = Some(PathBuf::from(value));
                    }
                }
            }
            "--debug-port" => {
                if let Some(value) = iter.next() {
                    if let Ok(port) = value.as_ref().parse::<u16>() {
                        options.debug_port = port;
                    }
                }
            }
            "--helper-port" => {
                if let Some(value) = iter.next() {
                    if let Ok(port) = value.as_ref().parse::<u16>() {
                        options.helper_port = port;
                    }
                }
            }
            _ => {}
        }
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_launch_options_accepts_manager_forwarded_ports_and_app_path() {
        let options = parse_launch_options([
            "--app-path",
            "C:/Codex/App",
            "--debug-port",
            "9333",
            "--helper-port",
            "57322",
        ]);

        assert_eq!(options.app_dir, Some(PathBuf::from("C:/Codex/App")));
        assert_eq!(options.debug_port, 9333);
        assert_eq!(options.helper_port, 57322);
    }

    #[test]
    fn parse_launch_options_ignores_invalid_ports() {
        let options = parse_launch_options(["--debug-port", "nope", "--helper-port", "70000"]);

        assert_eq!(options.debug_port, LaunchOptions::default().debug_port);
        assert_eq!(options.helper_port, LaunchOptions::default().helper_port);
    }

    #[test]
    fn launcher_uses_single_instance_guard_before_launching() {
        let source = include_str!("main.rs");

        assert!(source.contains("acquire_single_instance_guard(options.debug_port)?"));
        assert!(source.contains("launcher_guard_port"));
        assert!(source.contains("launcher.already_running"));
    }

    #[test]
    fn launcher_hooks_forward_computer_use_guard_methods() {
        let source = include_str!("lib.rs");

        assert!(source.contains("async fn ensure_computer_use_config"));
        assert!(source.contains("self.core.ensure_computer_use_config(settings).await"));
        assert!(source.contains("async fn ensure_plugin_marketplace_config"));
        assert!(source.contains("self.core.ensure_plugin_marketplace_config(settings).await"));
        assert!(source.contains("async fn start_computer_use_guard_watchdog"));
        assert!(source.contains("self.core"));
        assert!(source.contains(".start_computer_use_guard_watchdog(settings)"));
    }

    #[test]
    fn manager_update_prompt_uses_sidecar_manager_binary_name() {
        let path = codex_plus_launcher::manager_exe_path();

        assert!(
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains(codex_plus_core::install::MANAGER_BINARY))
        );
    }
}
