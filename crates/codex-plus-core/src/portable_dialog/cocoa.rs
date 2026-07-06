//! macOS (Cocoa/AppKit) configuration dialog for the portable launcher.
//!
//! TODO(macos): implement the native dialog. It must mirror the Windows
//! (`win32.rs`) behaviour:
//!
//! - A modal window with five text fields — API base URL, API key (secure),
//!   default model, provider name, Codex App path — plus a "Browse…" button
//!   next to the path field that opens an `NSOpenPanel` folder picker.
//! - Two buttons: "退出" (cancel → return `Ok(None)`) and "保存并启动 Codex"
//!   (save → return `Ok(Some(config))` built from the field values).
//! - Prefill the fields from `initial`.
//!
//! Suggested implementation crates (add under `[target.'cfg(target_os =
//! "macos")'.dependencies]` in Cargo.toml): `objc2`, `objc2-foundation`,
//! `objc2-app-kit`. Build/run on a Mac:
//!
//!     cargo build --release -p codex-plus-launcher --bin codex
//!
//! Until implemented, this returns an error so the failure is obvious rather
//! than silently launching with a half-filled config.

use crate::portable::PortableConfig;

pub fn show_portable_config_dialog(
    _initial: &PortableConfig,
) -> anyhow::Result<Option<PortableConfig>> {
    anyhow::bail!(
        "macOS 配置弹窗尚未实现，请在 crates/codex-plus-core/src/portable_dialog/cocoa.rs 中实现"
    )
}
