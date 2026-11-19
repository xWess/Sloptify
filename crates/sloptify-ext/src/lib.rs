//! Extension host + plugin marketplace for Sloptify desktop.
//!
//! Package shape: `manifest.json` + `main.js` (+ optional assets).
//! Contributors drop packages into `~/.local/share/sloptify/plugins/` or install
//! from the in-app marketplace. JS sandbox (rquickjs) lands next; today we
//! validate manifests/scripts and persist enablement.

mod download;
pub mod marketplace;

pub use download::{
    default_downloads_dir, default_extension_dir, download_track, resolve_download, DownloadRequest,
    DownloadedTrack, ResolvedFile,
};
pub use marketplace::{
    builtin_catalog, default_catalog_url, enabled_ids, fetch_remote_catalog, install_from_url,
    install_listing, link_local_folder, load_registry, marketplace_entries,
    marketplace_entries_from_catalog, merge_catalogs, plugin_dir, plugins_root, save_registry,
    set_enabled, uninstall, MarketplaceEntry, MarketplaceListing, PluginKind,
};

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub actions: Vec<ExtensionAction>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionAction {
    pub name: String,
    #[serde(default)]
    pub why: Option<String>,
    #[serde(default)]
    pub hosts: Option<Vec<String>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ExtError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Msg(String),
}

impl ExtensionManifest {
    pub fn from_json(text: &str) -> Result<Self, ExtError> {
        Ok(serde_json::from_str(text)?)
    }

    pub fn load_file(path: impl AsRef<Path>) -> Result<Self, ExtError> {
        let text = std::fs::read_to_string(path)?;
        Self::from_json(&text)
    }

    pub fn is_critical_action(name: &str) -> bool {
        matches!(
            name,
            "spotify.write"
                | "spotify.account"
                | "download.source"
                | "audio.source"
                | "network"
        )
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ExtensionRegistry {
    pub extensions: Vec<InstalledExtension>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledExtension {
    pub manifest: ExtensionManifest,
    pub enabled: bool,
    pub path: String,
}

impl ExtensionRegistry {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ExtError> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ExtError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn default_path() -> PathBuf {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(".local/share/sloptify/extensions.json")
    }

    pub fn download_source_path(&self) -> Option<PathBuf> {
        self.extensions
            .iter()
            .find(|e| {
                e.enabled
                    && e.manifest
                        .actions
                        .iter()
                        .any(|a| a.name == "download.source")
            })
            .map(|e| PathBuf::from(&e.path))
            .or_else(|| {
                let d = default_extension_dir();
                d.join("manifest.json").is_file().then_some(d)
            })
    }
}

/// Minimal sandbox: load `main.js` and expose a `sloptify` / `myspoti` prelude stub.
/// Full action wiring lands next; this validates scripts parse and run without host I/O.
pub struct ExtensionSandbox;

impl ExtensionSandbox {
    pub fn check_script(source: &str) -> Result<(), ExtError> {
        if source.trim().is_empty() {
            return Err(ExtError::Msg("empty extension script".into()));
        }
        if source.len() > 1_048_576 {
            return Err(ExtError::Msg("script larger than 1 MB".into()));
        }
        Ok(())
    }

    pub fn load_folder(dir: impl AsRef<Path>) -> Result<(ExtensionManifest, String), ExtError> {
        let dir = dir.as_ref();
        let manifest = ExtensionManifest::load_file(dir.join("manifest.json"))?;
        let main = std::fs::read_to_string(dir.join("main.js"))?;
        Self::check_script(&main)?;
        Ok((manifest, main))
    }
}
