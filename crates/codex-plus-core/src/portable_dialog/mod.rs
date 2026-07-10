//! Native, per-platform configuration dialog for the portable launcher.
//!
//! Each platform provides `show_portable_config_dialog(&PortableConfig) ->
//! anyhow::Result<Option<PortableConfig>>`, returning `Some(config)` when the
//! user saved and `None` when they cancelled. The Windows implementation is a
//! Win32 dialog; the macOS implementation is a Cocoa/AppKit dialog.

#[cfg(windows)]
mod win32;
#[cfg(windows)]
pub use win32::{show_portable_config_dialog, show_portable_error_dialog};

#[cfg(target_os = "macos")]
mod cocoa;
#[cfg(target_os = "macos")]
pub use cocoa::{show_portable_config_dialog, show_portable_error_dialog};
