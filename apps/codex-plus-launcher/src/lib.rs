//! Shared launcher plumbing reused by both the installed launcher binary
//! (`codex-plus-plus`) and the portable launcher binary (`chatgpt-launcher`).
//! Keeping this in one place avoids maintaining two copies of the CDP
//! bridge/data-service wiring.

use codex_plus_core::launcher::{BridgeReinjector, DefaultLaunchHooks, LaunchHooks};
use codex_plus_core::models::{DeleteResult, ExportResult, SessionRef};
use codex_plus_core::routes::{BridgeContext, BridgeDataService, BridgeRuntimeService};
use codex_plus_core::user_scripts::UserScriptManager;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct LauncherHooks {
    pub core: Arc<DefaultLaunchHooks>,
    pub data: Arc<LauncherDataService>,
    pub runtime: Arc<LauncherRuntimeService>,
    bridge_context: Arc<Mutex<Option<BridgeContext>>>,
}

impl Default for LauncherHooks {
    fn default() -> Self {
        Self {
            core: Arc::new(DefaultLaunchHooks::default()),
            data: Arc::new(LauncherDataService::default()),
            runtime: Arc::new(LauncherRuntimeService::new(
                9229,
                default_user_script_manager(),
                Vec::new(),
            )),
            bridge_context: Arc::new(Mutex::new(None)),
        }
    }
}

impl LauncherHooks {
    /// Same wiring as [`Default`], except "打开管理工具" launches the manager
    /// restricted to the Dream Skin screen (`--skin-only`). The portable
    /// distribution has no relay/plugin/session settings of its own (those
    /// live in `config.ini`, edited through the small native config dialog),
    /// so the full manager UI would be confusing there — this is the only
    /// part of it that's still useful in a portable install.
    pub fn portable() -> Self {
        Self {
            core: Arc::new(DefaultLaunchHooks::default()),
            data: Arc::new(LauncherDataService::default()),
            runtime: Arc::new(LauncherRuntimeService::new(
                9229,
                default_user_script_manager(),
                vec!["--skin-only".to_string()],
            )),
            bridge_context: Arc::new(Mutex::new(None)),
        }
    }

    fn watchdog_bridge_context(&self) -> anyhow::Result<BridgeContext> {
        self.bridge_context
            .lock()
            .map_err(|_| anyhow::anyhow!("bridge context lock poisoned"))?
            .clone()
            .ok_or_else(|| anyhow::anyhow!("bridge context is not initialized"))
    }
}

#[async_trait::async_trait(?Send)]
impl LaunchHooks for LauncherHooks {
    fn resolve_app_dir(
        &self,
        app_dir: Option<&std::path::Path>,
        settings: &codex_plus_core::settings::BackendSettings,
    ) -> anyhow::Result<std::path::PathBuf> {
        self.core.resolve_app_dir(app_dir, settings)
    }

    fn select_debug_port(&self, requested: u16) -> u16 {
        self.core.select_debug_port(requested)
    }

    fn select_helper_port(&self, requested: u16) -> u16 {
        self.core.select_helper_port(requested)
    }

    async fn load_settings(&self) -> anyhow::Result<codex_plus_core::settings::BackendSettings> {
        self.core.load_settings().await
    }

    fn cleanup_unsupported_config(&self) -> anyhow::Result<()> {
        self.core.cleanup_unsupported_config()
    }

    async fn run_provider_sync(&self) -> anyhow::Result<()> {
        let _ = tokio::task::spawn_blocking(|| codex_plus_data::run_provider_sync(None))
            .await
            .map_err(|error| anyhow::anyhow!("provider sync task failed: {error}"))?;
        Ok(())
    }

    fn has_pending_remote_control_session_recoveries(&self) -> bool {
        codex_plus_core::paths::default_pending_remote_control_recovery_path().exists()
    }

    fn remote_control_session_recovery_is_safe_to_run(&self) -> bool {
        codex_plus_core::watcher::find_session_index_cleanup_blocking_processes().is_empty()
    }

    async fn run_remote_control_session_recovery(&self) -> anyhow::Result<()> {
        let outcomes = tokio::task::spawn_blocking(|| {
            let requests = codex_plus_core::remote_control_recovery::load_pending_remote_control_recoveries(None)?;
            let settings = codex_plus_core::settings::SettingsStore::default()
                .load()?;
            let mut outcomes = Vec::with_capacity(requests.len());
            for request in requests {
                let current_profile = settings
                    .relay_profiles
                    .iter()
                    .find(|profile| profile.id == request.profile_id);
                if remote_control_recovery_is_superseded_by_openai(&settings, &request) {
                    let completion_error =
                        codex_plus_core::remote_control_recovery::complete_pending_remote_control_recovery(
                            None,
                            &request.thread_id,
                        )
                        .err()
                        .map(|error| error.to_string());
                    let completed = completion_error.is_none();
                    outcomes.push((
                        request,
                        codex_plus_data::ProviderSyncResult {
                            status: if completed {
                                codex_plus_data::ProviderSyncStatus::Synced
                            } else {
                                codex_plus_data::ProviderSyncStatus::Skipped
                            },
                            message: if completed {
                                "Remote Control session finalization discarded after switching to OpenAI session identity".to_string()
                            } else {
                                "Remote Control session finalization could not discard the superseded recovery request".to_string()
                            },
                            target_provider: "openai".to_string(),
                            backup_dir: None,
                            changed_session_files: 0,
                            sqlite_rows_updated: 0,
                            sqlite_provider_rows_updated: 0,
                            sqlite_user_event_rows_updated: 0,
                            sqlite_cwd_rows_updated: 0,
                            sqlite_catalog_rows_inserted: 0,
                            sqlite_catalog_rows_removed: 0,
                            updated_workspace_roots: 0,
                            skipped_locked_rollout_files: Vec::new(),
                            encrypted_content_warning: None,
                            repair_audit: codex_plus_data::ProviderSyncAudit::default(),
                        },
                        completion_error,
                    ));
                    continue;
                }
                let request_is_current = settings.active_relay_id == request.profile_id
                    && current_profile.is_some_and(|profile| {
                    codex_plus_core::remote_control_recovery::config_generation(
                        profile,
                        &request.target_provider,
                    ) == request.config_generation
                });
                if !request_is_current {
                    outcomes.push((
                        request,
                        codex_plus_data::ProviderSyncResult {
                            status: codex_plus_data::ProviderSyncStatus::Skipped,
                            message: "Remote Control session finalization deferred after relay profile changed".to_string(),
                            target_provider: String::new(),
                            backup_dir: None,
                            changed_session_files: 0,
                            sqlite_rows_updated: 0,
                            sqlite_provider_rows_updated: 0,
                            sqlite_user_event_rows_updated: 0,
                            sqlite_cwd_rows_updated: 0,
                            sqlite_catalog_rows_inserted: 0,
                            sqlite_catalog_rows_removed: 0,
                            updated_workspace_roots: 0,
                            skipped_locked_rollout_files: Vec::new(),
                            encrypted_content_warning: None,
                            repair_audit: codex_plus_data::ProviderSyncAudit::default(),
                        },
                        None,
                    ));
                    continue;
                }
                let result = codex_plus_data::run_remote_control_session_finalization_for_thread_with_target(
                    None,
                    &request.thread_id,
                    &request.target_provider,
                );
                let completed = result.status == codex_plus_data::ProviderSyncStatus::Synced;
                let completion_error = if completed {
                    codex_plus_core::remote_control_recovery::complete_pending_remote_control_recovery(
                        None,
                        &request.thread_id,
                    )
                    .err()
                    .map(|error| error.to_string())
                } else {
                    None
                };
                outcomes.push((request, result, completion_error));
            }
            Ok::<_, anyhow::Error>(outcomes)
        })
        .await
        .map_err(|error| anyhow::anyhow!("Remote Control session recovery task failed: {error}"))?;
        match outcomes {
            Ok(outcomes) => {
                for (request, result, completion_error) in outcomes {
                    let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
                        "launcher.remote_control_session_finalization",
                        json!({
                            "thread_id": request.thread_id,
                            "profile_id": request.profile_id,
                            "target_provider": request.target_provider,
                            "config_generation": request.config_generation,
                            "status": result.status,
                            "message": result.message,
                            "completion_error": completion_error
                        }),
                    );
                }
            }
            Err(error) => {
                let _ = codex_plus_core::diagnostic_log::append_diagnostic_log(
                    "launcher.remote_control_session_finalization_failed_nonfatal",
                    json!({"message": error.to_string()}),
                );
            }
        }
        Ok(())
    }

    async fn apply_active_relay_profile(
        &self,
        settings: &codex_plus_core::settings::BackendSettings,
    ) -> anyhow::Result<()> {
        self.core.apply_active_relay_profile(settings).await
    }

    async fn ensure_active_protocol_proxy_config(
        &self,
        settings: &codex_plus_core::settings::BackendSettings,
    ) -> anyhow::Result<()> {
        self.core
            .ensure_active_protocol_proxy_config(settings)
            .await
    }

    async fn ensure_plugin_marketplace_config(
        &self,
        settings: &codex_plus_core::settings::BackendSettings,
    ) -> anyhow::Result<()> {
        self.core.ensure_plugin_marketplace_config(settings).await
    }

    async fn start_helper(&self, helper_port: u16) -> anyhow::Result<()> {
        self.core.start_helper(helper_port).await
    }

    async fn launch_codex(
        &self,
        app_dir: &Path,
        debug_port: u16,
        settings: &codex_plus_core::settings::BackendSettings,
        extra_args: &[String],
    ) -> anyhow::Result<codex_plus_core::launcher::CodexLaunch> {
        self.core
            .launch_codex(app_dir, debug_port, settings, extra_args)
            .await
    }

    async fn bridge_context(
        &self,
        debug_port: u16,
        app_dir: &Path,
    ) -> anyhow::Result<Option<BridgeContext>> {
        self.runtime.set_debug_port(debug_port);
        let ctx = BridgeContext::core_with_data_and_app_dir(
            self.runtime.clone(),
            self.data.clone(),
            app_dir.to_path_buf(),
        );
        *self
            .bridge_context
            .lock()
            .map_err(|_| anyhow::anyhow!("bridge context lock poisoned"))? = Some(ctx.clone());
        Ok(Some(ctx))
    }

    async fn inject_bridge(
        &self,
        debug_port: u16,
        helper_port: u16,
        ctx: BridgeContext,
    ) -> anyhow::Result<()> {
        inject_with_context(debug_port, helper_port, ctx, self.runtime.clone()).await
    }

    async fn inject(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
        self.core.inject(debug_port, helper_port).await
    }

    async fn start_bridge_watchdog(&self, debug_port: u16, helper_port: u16) -> anyhow::Result<()> {
        let ctx = self.watchdog_bridge_context()?;
        let runtime = self.runtime.clone();
        let reinjector: BridgeReinjector = Arc::new(move || {
            let ctx = ctx.clone();
            let runtime = runtime.clone();
            Box::pin(
                async move { inject_with_context(debug_port, helper_port, ctx, runtime).await },
            )
        });
        self.core.set_bridge_reinjector(reinjector).await;
        self.core
            .start_bridge_watchdog(debug_port, helper_port)
            .await
    }

    async fn write_status(&self, status: &str) {
        self.core.write_status(status).await;
    }

    async fn wait_for_codex_exit(
        &self,
        launch: &codex_plus_core::launcher::CodexLaunch,
        debug_port: u16,
    ) -> anyhow::Result<()> {
        self.core.wait_for_codex_exit(launch, debug_port).await
    }

    async fn shutdown_helper(&self, helper_port: u16) {
        self.core.shutdown_helper(helper_port).await;
    }

    async fn terminate_codex(&self, launch: &codex_plus_core::launcher::CodexLaunch) {
        self.core.terminate_codex(launch).await;
    }
}

#[derive(Debug, Clone)]
pub struct LauncherDataService {
    db_path: PathBuf,
    backup_dir: PathBuf,
}

impl Default for LauncherDataService {
    fn default() -> Self {
        Self {
            db_path: default_codex_db_path(),
            backup_dir: codex_plus_core::paths::default_app_state_dir().join("backups"),
        }
    }
}

#[async_trait::async_trait]
impl BridgeDataService for LauncherDataService {
    async fn delete(&self, session: SessionRef) -> anyhow::Result<DeleteResult> {
        let db_paths = self.candidate_db_paths();
        let backup_store = codex_plus_data::BackupStore::new(self.backup_dir.clone());
        tokio::task::spawn_blocking(move || {
            codex_plus_data::delete_local_from_paths(
                db_paths,
                backup_store,
                &session,
                Some(&codex_plus_core::codex_sqlite::default_codex_home_dir()),
            )
        })
        .await
        .map_err(|error| anyhow::anyhow!("delete task failed: {error}"))
    }

    async fn undo(&self, undo_token: String) -> anyhow::Result<DeleteResult> {
        let adapter = self.storage_adapter();
        tokio::task::spawn_blocking(move || adapter.undo(&undo_token))
            .await
            .map_err(|error| anyhow::anyhow!("undo task failed: {error}"))
    }

    async fn export_markdown(&self, session: SessionRef) -> anyhow::Result<ExportResult> {
        let db_paths = self.candidate_db_paths();
        tokio::task::spawn_blocking(move || {
            codex_plus_data::export_markdown_from_paths(db_paths, &session)
        })
        .await
        .map_err(|error| anyhow::anyhow!("export markdown task failed: {error}"))
    }

    async fn thread_usage_history(&self, session: SessionRef) -> anyhow::Result<Value> {
        let adapter = self.storage_adapter();
        tokio::task::spawn_blocking(move || adapter.codex_thread_usage_history(&session))
            .await
            .map_err(|error| anyhow::anyhow!("thread usage history task failed: {error}"))
    }

    async fn find_archived_thread_by_title(
        &self,
        title: String,
    ) -> anyhow::Result<Option<SessionRef>> {
        let adapter = self.storage_adapter();
        tokio::task::spawn_blocking(move || adapter.find_archived_thread_by_title(&title))
            .await
            .map_err(|error| anyhow::anyhow!("archived lookup task failed: {error}"))
    }

    async fn recover_remote_control_session(&self, thread_id: String) -> anyhow::Result<Value> {
        let settings = codex_plus_core::settings::SettingsStore::default()
            .load()
            .unwrap_or_default();
        let profile = settings.active_relay_profile();
        if !settings.relay_profiles_enabled
            || profile.relay_mode != codex_plus_core::settings::RelayMode::Official
            || !profile.official_mix_api_key
        {
            return Ok(json!({
                "status": "skipped",
                "message": "Remote Control session recovery is disabled for the active profile"
            }));
        }
        let home = codex_plus_core::codex_sqlite::default_codex_home_dir();
        let target_provider =
            codex_plus_core::model_catalog::codex_model_provider_for_relay_profile(&home, &profile);
        if target_provider.trim().is_empty() || target_provider == "openai" {
            return Ok(json!({
                "status": "skipped",
                "message": "Remote Control session recovery requires a non-openai target provider"
            }));
        }
        let candidate_thread_id = thread_id.clone();
        let candidate = tokio::task::spawn_blocking(move || {
            codex_plus_data::remote_control_session_recovery_candidate_exists(
                None,
                &candidate_thread_id,
            )
        })
        .await
        .map_err(|error| anyhow::anyhow!("Remote Control candidate check failed: {error}"))??;
        if !candidate {
            return Ok(json!({
                "status": "skipped",
                "message": "Remote Control session recovery is waiting for a recent openai thread"
            }));
        }
        let request = codex_plus_core::remote_control_recovery::PendingRemoteControlRecovery {
            thread_id: thread_id.clone(),
            profile_id: profile.id.clone(),
            target_provider: target_provider.clone(),
            config_generation: codex_plus_core::remote_control_recovery::config_generation(
                &profile,
                &target_provider,
            ),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        };
        codex_plus_core::remote_control_recovery::enqueue_pending_remote_control_recovery(
            None, request,
        )?;
        tokio::task::spawn_blocking(move || {
            serde_json::to_value(
                codex_plus_data::run_remote_control_session_catalog_recovery_for_thread_with_target(
                    None,
                    &thread_id,
                    &target_provider,
                ),
            )
            .map_err(anyhow::Error::from)
        })
        .await
        .map_err(|error| anyhow::anyhow!("Remote Control session recovery task failed: {error}"))?
    }

    async fn export_session_file(&self, session: SessionRef) -> anyhow::Result<Value> {
        LauncherDataService::export_session_file(self, session).await
    }

    async fn import_session_file(&self, payload: Value) -> anyhow::Result<Value> {
        LauncherDataService::import_session_file(self, payload).await
    }
}

impl LauncherDataService {
    fn candidate_db_paths(&self) -> Vec<PathBuf> {
        let mut paths = vec![self.db_path.clone()];
        for path in codex_plus_core::codex_sqlite::codex_session_db_paths_from_home(
            &codex_plus_core::codex_sqlite::default_codex_home_dir(),
        ) {
            if !paths.iter().any(|candidate| candidate == &path) {
                paths.push(path);
            }
        }
        paths
    }

    fn storage_adapter(&self) -> codex_plus_data::SQLiteStorageAdapter {
        let allowed_db_paths = self.candidate_db_paths();
        codex_plus_data::SQLiteStorageAdapter::new(
            self.db_path.clone(),
            codex_plus_data::BackupStore::new(self.backup_dir.clone()),
        )
        .with_allowed_db_paths(allowed_db_paths)
        .with_codex_home(codex_plus_core::codex_sqlite::default_codex_home_dir())
    }

    async fn export_session_file(&self, session: SessionRef) -> anyhow::Result<Value> {
        let home = codex_plus_core::codex_sqlite::default_codex_home_dir();
        tokio::task::spawn_blocking(move || {
            codex_plus_core::session_share::export_rollout(&home, &session.session_id)
        })
        .await
        .map_err(|error| anyhow::anyhow!("session export task failed: {error}"))?
    }

    async fn import_session_file(&self, payload: Value) -> anyhow::Result<Value> {
        let home = codex_plus_core::codex_sqlite::default_codex_home_dir();
        tokio::task::spawn_blocking(move || {
            codex_plus_core::session_share::import_rollout(&home, &payload)
        })
        .await
        .map_err(|error| anyhow::anyhow!("session import task failed: {error}"))?
    }
}

pub struct LauncherRuntimeService {
    debug_port: Mutex<u16>,
    websocket_url: Mutex<Option<String>>,
    user_scripts: UserScriptManager,
    manager_launch_args: Vec<String>,
}

impl LauncherRuntimeService {
    pub fn new(
        debug_port: u16,
        user_scripts: UserScriptManager,
        manager_launch_args: Vec<String>,
    ) -> Self {
        Self {
            debug_port: Mutex::new(debug_port),
            websocket_url: Mutex::new(None),
            user_scripts,
            manager_launch_args,
        }
    }

    fn set_debug_port(&self, debug_port: u16) {
        *self.debug_port.lock().unwrap() = debug_port;
    }

    fn set_websocket_url(&self, websocket_url: &str) {
        *self.websocket_url.lock().unwrap() = Some(websocket_url.to_string());
    }
}

#[async_trait::async_trait]
impl BridgeRuntimeService for LauncherRuntimeService {
    async fn user_script_inventory(&self) -> anyhow::Result<Value> {
        self.user_scripts.inventory()
    }

    async fn user_script_inventory_with_runtime_status(
        &self,
        payload: Value,
    ) -> anyhow::Result<Value> {
        self.user_scripts
            .inventory_with_runtime_status(payload.get("runtime_status"))
    }

    async fn set_user_scripts_enabled(&self, enabled: bool) -> anyhow::Result<Value> {
        self.user_scripts.set_global_enabled(enabled)?;
        self.user_scripts.inventory()
    }

    async fn set_user_script_enabled(&self, key: String, enabled: bool) -> anyhow::Result<Value> {
        self.user_scripts.set_script_enabled(&key, enabled)?;
        self.user_scripts.inventory()
    }

    async fn delete_user_script(&self, key: String) -> anyhow::Result<Value> {
        self.user_scripts.delete_user_script(&key)?;
        self.user_scripts.inventory()
    }

    async fn reload_user_scripts(&self) -> anyhow::Result<Value> {
        let bundle = self.user_scripts.build_enabled_bundle()?;
        let websocket_url = self.websocket_url.lock().unwrap().clone();
        if let Some(websocket_url) = websocket_url.filter(|_| !bundle.trim().is_empty()) {
            codex_plus_core::bridge::evaluate_script(&websocket_url, &bundle).await?;
        }
        self.user_scripts.inventory()
    }

    async fn open_devtools(&self) -> anyhow::Result<Value> {
        let debug_port = *self.debug_port.lock().unwrap();
        let targets = codex_plus_core::cdp::list_targets(debug_port).await?;
        let target = codex_plus_core::cdp::pick_page_target(&targets)?;
        let url = codex_plus_core::routes::devtools_url(debug_port, &target.id);
        open_url(&url)?;
        Ok(json!({
            "status": "ok",
            "target_id": target.id,
            "url": url
        }))
    }

    async fn open_manager(&self, payload: Value) -> anyhow::Result<Value> {
        let navigation =
            codex_plus_core::manager_navigation::save_pending_manager_navigation_from_payload(
                &payload,
            )?;
        let target = if self.manager_launch_args.is_empty() {
            codex_plus_core::install::open_or_activate_manager()
        } else {
            codex_plus_core::install::spawn_companion(
                codex_plus_core::install::MANAGER_BINARY,
                self.manager_launch_args.iter().map(String::as_str),
            )
        }
            .map_err(|error| anyhow::anyhow!("启动管理工具失败：{error}"))
            .map_err(|error| {
                codex_plus_core::manager_navigation::rollback_pending_manager_navigation_after_launch_failure(
                    navigation.as_ref(),
                    error,
                )
            })?;
        Ok(json!({
            "status": "ok",
            "path": target,
            "navigation": navigation
        }))
    }

    async fn open_transient_manager(&self, payload: Value) -> anyhow::Result<Value> {
        let navigation =
            codex_plus_core::manager_navigation::save_pending_manager_navigation_from_payload(
                &payload,
            )?;
        let target = codex_plus_core::install::spawn_companion(
            codex_plus_core::install::MANAGER_BINARY,
            self.manager_launch_args
                .iter()
                .map(String::as_str)
                .chain(["--transient"]),
        )
        .map_err(|error| anyhow::anyhow!("启动管理工具失败：{error}"))
        .map_err(|error| {
            codex_plus_core::manager_navigation::rollback_pending_manager_navigation_after_launch_failure(
                navigation.as_ref(),
                error,
            )
        })?;
        Ok(json!({
            "status": "ok",
            "path": target,
            "navigation": navigation
        }))
    }

    async fn backend_status(&self) -> anyhow::Result<Value> {
        Ok(
            json!({"status": "ok", "message": "后端已连接", "version": codex_plus_core::version::VERSION}),
        )
    }

    async fn codex_model_catalog(&self) -> anyhow::Result<Value> {
        Ok(codex_plus_core::model_catalog::read_codex_model_catalog().await)
    }

    async fn ads(&self) -> anyhow::Result<Value> {
        codex_plus_core::ads::fetch_ad_list().await
    }

    async fn zed_remote_status(&self) -> anyhow::Result<Value> {
        Ok(codex_plus_core::zed_remote::zed_remote_status())
    }

    async fn resolve_zed_remote_host(&self, payload: Value) -> anyhow::Result<Value> {
        Ok(codex_plus_core::zed_remote::resolve_ssh_target_response(
            &payload,
        ))
    }

    async fn fallback_zed_remote_request(&self, payload: Value) -> anyhow::Result<Value> {
        Ok(codex_plus_core::zed_remote::fallback_open_request_response(
            &payload,
        ))
    }

    async fn open_zed_remote(&self, payload: Value) -> anyhow::Result<Value> {
        Ok(codex_plus_core::zed_remote::open_zed_remote(&payload))
    }

    async fn list_zed_remote_projects(&self, payload: Value) -> anyhow::Result<Value> {
        Ok(codex_plus_core::zed_remote::list_zed_remote_projects_response(&payload))
    }

    async fn remember_zed_remote_project(&self, payload: Value) -> anyhow::Result<Value> {
        Ok(codex_plus_core::zed_remote::remember_zed_remote_project_response(&payload))
    }

    async fn forget_zed_remote_project(&self, payload: Value) -> anyhow::Result<Value> {
        Ok(codex_plus_core::zed_remote::forget_zed_remote_project_response(&payload))
    }

    async fn upstream_worktree_status(&self) -> anyhow::Result<Value> {
        Ok(codex_plus_core::upstream_worktree::status_response())
    }

    async fn upstream_worktree_defaults(&self, payload: Value) -> anyhow::Result<Value> {
        Ok(codex_plus_core::upstream_worktree::defaults_response(
            &payload,
        ))
    }

    async fn upstream_worktree_prepare(&self, payload: Value) -> anyhow::Result<Value> {
        Ok(codex_plus_core::upstream_worktree::prepare_response(
            &payload,
        ))
    }

    async fn upstream_worktree_create(&self, payload: Value) -> anyhow::Result<Value> {
        Ok(codex_plus_core::upstream_worktree::create_response(
            &payload,
        ))
    }
}

async fn inject_with_context(
    debug_port: u16,
    helper_port: u16,
    ctx: BridgeContext,
    runtime: Arc<LauncherRuntimeService>,
) -> anyhow::Result<()> {
    let mut last_error = None;
    for _ in 0..20 {
        match try_inject_with_context(debug_port, helper_port, ctx.clone(), runtime.clone()).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Codex injection failed")))
}

fn remote_control_recovery_is_superseded_by_openai(
    settings: &codex_plus_core::settings::BackendSettings,
    request: &codex_plus_core::remote_control_recovery::PendingRemoteControlRecovery,
) -> bool {
    settings.active_relay_id == request.profile_id
        && settings.active_relay_session_provider()
            == codex_plus_core::settings::RelaySessionProvider::Openai
}

async fn try_inject_with_context(
    debug_port: u16,
    helper_port: u16,
    ctx: BridgeContext,
    runtime: Arc<LauncherRuntimeService>,
) -> anyhow::Result<()> {
    let targets = codex_plus_core::cdp::list_targets(debug_port).await?;
    let target = codex_plus_core::cdp::pick_injectable_codex_page_target(&targets)?;
    let websocket_url = target
        .web_socket_debugger_url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("selected CDP target has no websocket URL"))?;
    runtime.set_websocket_url(websocket_url);
    let settings = codex_plus_core::settings::SettingsStore::default()
        .load()
        .unwrap_or_default();
    let script = codex_plus_core::assets::injection_script_with_settings(helper_port, &settings);
    let user_bundle = runtime
        .user_scripts
        .build_enabled_bundle()
        .unwrap_or_default();
    let new_document_scripts = if user_bundle.is_empty() {
        vec![script]
    } else {
        vec![script, user_bundle]
    };
    codex_plus_core::bridge::install_bridge(
        websocket_url,
        codex_plus_core::bridge::BRIDGE_BINDING_NAME,
        Arc::new(move |path, payload| {
            let ctx = ctx.clone();
            Box::pin(async move {
                Ok(codex_plus_core::routes::handle_bridge_request(ctx, &path, payload).await)
            })
        }),
        &new_document_scripts,
    )
    .await
}

pub fn default_codex_db_path() -> PathBuf {
    codex_plus_core::codex_sqlite::codex_session_db_path()
}

pub fn open_url(url: &str) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        codex_plus_core::windows_open_url(url)
            .map_err(|error| anyhow::anyhow!("failed to open DevTools URL: {error}"))
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!("failed to open DevTools URL: {error}"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|error| anyhow::anyhow!("failed to open DevTools URL: {error}"))
    }

    #[cfg(not(any(windows, target_os = "macos", unix)))]
    {
        let _ = url;
        anyhow::bail!("opening DevTools URL is not supported on this platform")
    }
}

pub fn default_user_script_manager() -> UserScriptManager {
    let config_dir = default_user_scripts_config_dir();
    UserScriptManager::new(
        builtin_user_scripts_dir(),
        config_dir.join("user_scripts"),
        config_dir.join("user_scripts.json"),
    )
}

pub fn default_user_scripts_config_dir() -> PathBuf {
    if cfg!(windows) {
        if let Some(roaming) = std::env::var_os("APPDATA") {
            return PathBuf::from(roaming).join("Codex++");
        }
        if let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
            return home.join("AppData").join("Roaming").join("Codex++");
        }
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("Codex++")
}

pub fn builtin_user_scripts_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .map(|path| path.join("user_scripts"))
        .unwrap_or_else(|| PathBuf::from("user_scripts"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_session_identity_supersedes_only_its_active_pending_recovery() {
        let request = codex_plus_core::remote_control_recovery::PendingRemoteControlRecovery {
            thread_id: "mobile".to_string(),
            profile_id: "relay".to_string(),
            target_provider: "custom".to_string(),
            config_generation: "old-generation".to_string(),
            created_at: 1,
        };
        let mut settings = codex_plus_core::settings::BackendSettings {
            active_relay_id: "relay".to_string(),
            relay_profiles: vec![codex_plus_core::settings::RelayProfile {
                id: "relay".to_string(),
                config_contents: "model_provider = \"openai\"\n".to_string(),
                ..codex_plus_core::settings::RelayProfile::default()
            }],
            ..codex_plus_core::settings::BackendSettings::default()
        };

        assert!(remote_control_recovery_is_superseded_by_openai(
            &settings, &request
        ));

        settings.relay_profiles[0].config_contents = "model_provider = \"custom\"\n".to_string();
        assert!(!remote_control_recovery_is_superseded_by_openai(
            &settings, &request
        ));

        settings.relay_profiles[0].config_contents = "model_provider = \"openai\"\n".to_string();
        settings.active_relay_id = "other".to_string();
        assert!(!remote_control_recovery_is_superseded_by_openai(
            &settings, &request
        ));
    }

    #[tokio::test]
    async fn watchdog_reuses_bridge_context_with_data_service() {
        let test_dir = std::env::temp_dir().join(format!(
            "codex-plus-launcher-watchdog-test-{}",
            std::process::id()
        ));
        let hooks = LauncherHooks {
            core: Arc::new(DefaultLaunchHooks::default()),
            data: Arc::new(LauncherDataService {
                db_path: test_dir.join("state.sqlite"),
                backup_dir: test_dir.join("backups"),
            }),
            runtime: Arc::new(LauncherRuntimeService::new(
                9229,
                UserScriptManager::new(
                    test_dir.join("builtin"),
                    test_dir.join("user"),
                    test_dir.join("settings.json"),
                ),
                Vec::new(),
            )),
            bridge_context: Arc::new(Mutex::new(None)),
        };

        hooks.bridge_context(9229, &test_dir).await.unwrap();
        let ctx = hooks.watchdog_bridge_context().unwrap();
        let result = codex_plus_core::routes::handle_bridge_request(
            ctx,
            "/move-thread-workspace",
            json!({"session_id": "missing", "title": "Missing", "target_cwd": "/new"}),
        )
        .await;

        assert_ne!(
            result["message"],
            "Move workspace service is not wired in core launcher hooks"
        );
    }
}
