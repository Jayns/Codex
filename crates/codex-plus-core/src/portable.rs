//! Minimal portable-mode configuration: a single `config.ini` living next to the
//! launcher executable, instead of the full `BackendSettings` JSON stored under
//! the user's home directory. Intended for self-contained "copy the folder and
//! run" distributions that bundle their own Codex App copy.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::settings::{BackendSettings, RelayMode, RelayProfile, RelayProtocol};

const SECTION: &str = "codex";
const PORTABLE_RELAY_ID: &str = "portable";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortableConfig {
    pub api_base_url: String,
    pub api_key: String,
    pub model: String,
    pub provider_name: String,
    pub codex_app_dir: String,
    pub debug_port: u16,
    pub last_synced_hash: String,
}

impl Default for PortableConfig {
    fn default() -> Self {
        Self {
            api_base_url: "https://sub2api.vx123.xin/v1".to_string(),
            api_key: String::new(),
            model: "gpt-5.5".to_string(),
            provider_name: "custom".to_string(),
            codex_app_dir: String::new(),
            debug_port: 9229,
            last_synced_hash: String::new(),
        }
    }
}

impl PortableConfig {
    /// True when the required fields for a usable relay connection are present.
    pub fn is_complete(&self) -> bool {
        !self.api_base_url.trim().is_empty() && !self.api_key.trim().is_empty()
    }

    pub fn load(path: &Path) -> PortableConfig {
        let Ok(contents) = fs::read_to_string(path) else {
            return PortableConfig::default();
        };
        let values = parse_ini_section(contents.trim_start_matches('\u{feff}'), SECTION);
        let mut config = PortableConfig::default();
        if let Some(value) = values.get("api_base_url") {
            config.api_base_url = value.clone();
        }
        if let Some(value) = values.get("api_key") {
            config.api_key = value.clone();
        }
        if let Some(value) = values.get("model") {
            config.model = value.clone();
        }
        if let Some(value) = values.get("provider_name") {
            config.provider_name = value.clone();
        }
        if let Some(value) = values.get("codex_app_dir") {
            config.codex_app_dir = value.clone();
        }
        if let Some(value) = values.get("debug_port") {
            if let Ok(port) = value.parse::<u16>() {
                config.debug_port = port;
            }
        }
        if let Some(value) = values.get("last_synced_hash") {
            config.last_synced_hash = value.clone();
        }
        config
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let contents = format!(
            "[{section}]\n\
             api_base_url = {api_base_url}\n\
             api_key = {api_key}\n\
             codex_app_dir = {codex_app_dir}\n\
             model = {model}\n\
             provider_name = {provider_name}\n\
             debug_port = {debug_port}\n\
             last_synced_hash = {last_synced_hash}\n",
            section = SECTION,
            api_base_url = self.api_base_url,
            api_key = self.api_key,
            codex_app_dir = self.codex_app_dir,
            model = self.model,
            provider_name = self.provider_name,
            debug_port = self.debug_port,
            last_synced_hash = self.last_synced_hash,
        );
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
        Ok(())
    }

    /// Builds a `BackendSettings` value that the existing launch/inject pipeline
    /// (`apply_active_relay_profile`, `launch_and_inject_with_hooks`, ...) can
    /// consume unmodified.
    ///
    /// Uses `PureApi` relay mode: the API key is written into `auth.json` as
    /// `OPENAI_API_KEY` so Codex authenticates with the relay directly and the
    /// login screen is skipped entirely. (`MixedApi`/`Official` instead expect
    /// an existing ChatGPT login, which a portable, key-only setup does not
    /// have.) Note the launcher applies this profile directly without going
    /// through `normalize_relay_profile_for_storage`, so we populate
    /// `auth_contents` here rather than relying on that step to fill it in.
    pub fn to_backend_settings(&self) -> BackendSettings {
        let auth_contents = if self.api_key.trim().is_empty() {
            String::new()
        } else {
            serde_json::to_string_pretty(&serde_json::json!({
                "OPENAI_API_KEY": self.api_key.trim()
            }))
            .unwrap_or_default()
        };

        let mut settings = BackendSettings::default();
        settings.codex_app_path = self.codex_app_dir.clone();
        settings.relay_profiles_enabled = true;
        settings.active_relay_id = PORTABLE_RELAY_ID.to_string();
        // Only set the fields the portable flow cares about; inherit the rest
        // from RelayProfile::default() so upstream field additions don't break
        // this construction.
        settings.relay_profiles = vec![RelayProfile {
            id: PORTABLE_RELAY_ID.to_string(),
            name: self.provider_name.clone(),
            model: self.model.clone(),
            base_url: self.api_base_url.clone(),
            upstream_base_url: self.api_base_url.clone(),
            api_key: self.api_key.clone(),
            protocol: RelayProtocol::Responses,
            relay_mode: RelayMode::PureApi,
            official_mix_api_key: false,
            auth_contents,
            ..RelayProfile::default()
        }];
        settings
    }
}

/// Hand-rolled INI reader: just enough for flat `key = value` pairs under a
/// single `[section]` header. No external dependency needed for this format.
fn parse_ini_section(contents: &str, section: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let mut in_section = false;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line[1..line.len() - 1].trim() == section;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            values.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    values
}

/// Default `config.ini` path: next to the running executable (portable layout).
///
/// On macOS the launcher is packaged as `Codex++ Portable.app` so double-
/// clicking it doesn't spawn a Terminal window (see
/// `scripts/installer/macos/package-portable.sh`), which means the running
/// executable actually lives at `Codex++ Portable.app/Contents/MacOS/codex`.
/// Anchoring `config.ini` there would bury it inside the bundle, so this
/// resolves to the directory *containing* the `.app` instead, keeping the
/// portable folder a flat `Codex++ Portable.app` + `config.ini` layout.
pub fn default_portable_config_path() -> PathBuf {
    portable_root_dir().join("config.ini")
}

/// Default bundled Codex App directory: `codex_app` next to the executable.
pub fn default_portable_app_dir() -> PathBuf {
    portable_root_dir().join("codex_app")
}

fn portable_root_dir() -> PathBuf {
    let Some(exe) = std::env::current_exe().ok() else {
        return PathBuf::from(".");
    };
    #[cfg(target_os = "macos")]
    if let Some(root) = macos_app_bundle_root(&exe) {
        return root;
    }
    exe.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// If `exe` is `Foo.app/Contents/MacOS/exe`, returns the directory containing
/// `Foo.app`. Returns `None` for a loose (non-bundled) executable, e.g. when
/// running `cargo run` / the unpackaged binary directly during development.
#[cfg(target_os = "macos")]
fn macos_app_bundle_root(exe: &Path) -> Option<PathBuf> {
    let macos_dir = exe.parent()?;
    if macos_dir.file_name()? != "MacOS" {
        return None;
    }
    let contents_dir = macos_dir.parent()?;
    if contents_dir.file_name()? != "Contents" {
        return None;
    }
    let app_dir = contents_dir.parent()?;
    if app_dir.extension()? != "app" {
        return None;
    }
    app_dir.parent().map(Path::to_path_buf)
}

#[cfg(target_os = "macos")]
#[cfg(test)]
mod macos_bundle_tests {
    use super::*;

    #[test]
    fn resolves_root_for_bundled_executable() {
        let exe = Path::new("/Applications/Codex++ Portable.app/Contents/MacOS/codex");
        assert_eq!(
            macos_app_bundle_root(exe),
            Some(PathBuf::from("/Applications"))
        );
    }

    #[test]
    fn returns_none_for_loose_executable() {
        let exe = Path::new("/Users/me/dev/target/release/codex");
        assert_eq!(macos_app_bundle_root(exe), None);
    }

    #[test]
    fn returns_none_when_app_extension_is_missing() {
        let exe = Path::new("/Applications/NotAnApp/Contents/MacOS/codex");
        assert_eq!(macos_app_bundle_root(exe), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flat_key_value_pairs_under_section() {
        let ini = "[codex]\napi_base_url = https://example.com/v1\napi_key = sk-abc\nmodel = gpt-5.5\ndebug_port = 9333\n";
        let config = {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.ini");
            fs::write(&path, ini).unwrap();
            PortableConfig::load(&path)
        };

        assert_eq!(config.api_base_url, "https://example.com/v1");
        assert_eq!(config.api_key, "sk-abc");
        assert_eq!(config.model, "gpt-5.5");
        assert_eq!(config.debug_port, 9333);
        assert!(config.is_complete());
    }

    #[test]
    fn parses_file_with_leading_utf8_bom() {
        let ini = "\u{feff}[codex]\napi_base_url = https://example.com/v1\napi_key = sk-abc\n";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.ini");
        fs::write(&path, ini).unwrap();

        let config = PortableConfig::load(&path);

        assert_eq!(config.api_base_url, "https://example.com/v1");
        assert_eq!(config.api_key, "sk-abc");
    }

    #[test]
    fn missing_file_returns_defaults() {
        let config = PortableConfig::load(Path::new("does/not/exist.ini"));
        assert_eq!(config, PortableConfig::default());
        assert!(!config.is_complete());
    }

    #[test]
    fn round_trips_through_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.ini");
        let config = PortableConfig {
            api_base_url: "https://sub2api.example/v1".to_string(),
            api_key: "sk-xyz".to_string(),
            model: "gpt-5.5".to_string(),
            provider_name: "custom".to_string(),
            codex_app_dir: "D:/Portable/codex_app".to_string(),
            debug_port: 9229,
            last_synced_hash: "abc123".to_string(),
        };
        config.save(&path).unwrap();
        let loaded = PortableConfig::load(&path);
        assert_eq!(loaded, config);
    }

    #[test]
    fn maps_into_backend_settings_relay_profile() {
        let config = PortableConfig {
            api_base_url: "https://sub2api.example/v1".to_string(),
            api_key: "sk-xyz".to_string(),
            model: "gpt-5.5".to_string(),
            provider_name: "custom".to_string(),
            codex_app_dir: "D:/Portable/codex_app".to_string(),
            debug_port: 9229,
            last_synced_hash: String::new(),
        };
        let settings = config.to_backend_settings();
        let profile = settings.active_relay_profile();

        assert_eq!(settings.codex_app_path, "D:/Portable/codex_app");
        assert!(settings.relay_profiles_enabled);
        assert_eq!(profile.base_url, "https://sub2api.example/v1");
        assert_eq!(profile.api_key, "sk-xyz");
        assert_eq!(profile.model, "gpt-5.5");
        // PureApi so the key is written into auth.json and the login screen is
        // skipped (MixedApi/Official would still demand a ChatGPT login).
        assert_eq!(profile.relay_mode, RelayMode::PureApi);
        assert!(!profile.official_mix_api_key);
        // auth.json carries the API key as OPENAI_API_KEY.
        let auth: serde_json::Value =
            serde_json::from_str(&profile.auth_contents).expect("auth_contents is valid JSON");
        assert_eq!(auth["OPENAI_API_KEY"], "sk-xyz");
    }

    #[test]
    fn empty_api_key_produces_empty_auth_contents() {
        let config = PortableConfig {
            api_base_url: "https://sub2api.example/v1".to_string(),
            api_key: String::new(),
            ..PortableConfig::default()
        };
        let profile = config.to_backend_settings().relay_profiles[0].clone();
        assert!(profile.auth_contents.is_empty());
    }
}
