//! Plugin marketplace: discover, install, enable/disable local packages.
//!
//! A plugin is a folder with `manifest.json` (+ optional `main.js`, assets).
//! Contributors publish that shape; Sloptify installs into
//! `~/.local/share/sloptify/plugins/<id>/`.

use crate::{ExtError, ExtensionManifest, ExtensionRegistry, InstalledExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Theme,
    Action,
    Integration,
    Source,
}

impl PluginKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Action => "Action",
            Self::Integration => "Integration",
            Self::Source => "Source",
        }
    }
}

impl Default for PluginKind {
    fn default() -> Self {
        Self::Integration
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarketplaceListing {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub kind: PluginKind,
    #[serde(default)]
    pub homepage: Option<String>,
    /// Optional zip / folder URL. Empty = local/dev install only.
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Bundled sample `main.js` when installing from the curated catalog.
    #[serde(default)]
    pub sample_script: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarketplaceEntry {
    pub listing: MarketplaceListing,
    pub installed: bool,
    pub enabled: bool,
    pub local_path: Option<String>,
}

pub fn plugins_root() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share/sloptify/plugins")
}

pub fn registry_path() -> PathBuf {
    ExtensionRegistry::default_path()
}

/// Curated starter catalog — contributors mirror this shape when publishing.
pub fn builtin_catalog() -> Vec<MarketplaceListing> {
    vec![
        MarketplaceListing {
            id: "sloptify.keyboard-nav".into(),
            name: "Keyboard Nav".into(),
            version: "0.1.0".into(),
            author: "Sloptify".into(),
            description: "Arrow keys + space for transport; j/k for next/previous.".into(),
            kind: PluginKind::Action,
            homepage: Some("https://github.com/xWess/Sloptify".into()),
            download_url: None,
            tags: vec!["productivity".into(), "input".into()],
            sample_script: Some(
                "// sloptify.keyboard-nav — host binds keys when this plugin is enabled.\n\
                 export const actions = ['player.transport'];\n"
                    .into(),
            ),
        },
        MarketplaceListing {
            id: "sloptify.now-playing-notify".into(),
            name: "Desktop Notify".into(),
            version: "0.1.0".into(),
            author: "Sloptify".into(),
            description: "Show a desktop notification when the track changes.".into(),
            kind: PluginKind::Integration,
            homepage: Some("https://github.com/xWess/Sloptify".into()),
            download_url: None,
            tags: vec!["desktop".into(), "notify".into()],
            sample_script: Some(
                "// sloptify.now-playing-notify\n\
                 export const actions = ['player.trackChanged'];\n"
                    .into(),
            ),
        },
        MarketplaceListing {
            id: "sloptify.dense-lists".into(),
            name: "Dense Lists".into(),
            version: "0.1.0".into(),
            author: "Sloptify".into(),
            description: "Tighter row spacing for large playlists on small screens.".into(),
            kind: PluginKind::Theme,
            homepage: Some("https://github.com/xWess/Sloptify".into()),
            download_url: None,
            tags: vec!["ui".into(), "theme".into()],
            sample_script: Some(
                "// sloptify.dense-lists — host applies CSS vars when enabled.\n\
                 export const theme = { rowHeight: 40 };\n"
                    .into(),
            ),
        },
        MarketplaceListing {
            id: "community.scrobble-hook".into(),
            name: "Scrobble Hook".into(),
            version: "0.1.0".into(),
            author: "Community".into(),
            description: "Skeleton for Last.fm / ListenBrainz scrobbling contributors.".into(),
            kind: PluginKind::Integration,
            homepage: None,
            download_url: None,
            tags: vec!["scrobble".into(), "community".into()],
            sample_script: Some(
                "// community.scrobble-hook — implement listen() and call host.scrobble.\n\
                 export async function onTrackEnded(track) {\n\
                   // await host.scrobble(track)\n\
                 }\n"
                    .into(),
            ),
        },
    ]
}

pub fn load_registry() -> ExtensionRegistry {
    ExtensionRegistry::load(registry_path()).unwrap_or_default()
}

pub fn save_registry(reg: &ExtensionRegistry) -> Result<(), ExtError> {
    reg.save(registry_path())
}

pub fn marketplace_entries(reg: &ExtensionRegistry) -> Vec<MarketplaceEntry> {
    let catalog = builtin_catalog();
    let mut out: Vec<MarketplaceEntry> = catalog
        .into_iter()
        .map(|listing| {
            let installed = reg.extensions.iter().find(|e| e.manifest.id == listing.id);
            MarketplaceEntry {
                installed: installed.is_some(),
                enabled: installed.map(|e| e.enabled).unwrap_or(false),
                local_path: installed.map(|e| e.path.clone()),
                listing,
            }
        })
        .collect();

    // Local plugins not in the curated catalog still show up.
    for ext in &reg.extensions {
        if out.iter().any(|e| e.listing.id == ext.manifest.id) {
            continue;
        }
        let id = ext.manifest.id.as_str();
        if id.contains("lossless") || id.contains("flac") || id.starts_with("audio.") {
            continue;
        }
        out.push(MarketplaceEntry {
            listing: MarketplaceListing {
                id: ext.manifest.id.clone(),
                name: ext.manifest.name.clone(),
                version: ext.manifest.version.clone(),
                author: ext.manifest.author.clone().unwrap_or_default(),
                description: ext.manifest.description.clone().unwrap_or_default(),
                kind: PluginKind::Integration,
                homepage: None,
                download_url: None,
                tags: vec!["local".into()],
                sample_script: None,
            },
            installed: true,
            enabled: ext.enabled,
            local_path: Some(ext.path.clone()),
        });
    }
    out.sort_by(|a, b| a.listing.name.to_lowercase().cmp(&b.listing.name.to_lowercase()));
    out
}

pub fn plugin_dir(id: &str) -> PathBuf {
    plugins_root().join(sanitize_id(id))
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}


/// Default community catalog (override with SLOPTIFY_PLUGIN_CATALOG).
pub fn default_catalog_url() -> String {
    std::env::var("SLOPTIFY_PLUGIN_CATALOG").unwrap_or_else(|_| {
        "https://raw.githubusercontent.com/xWess/Sloptify/main/plugins/catalog.json".into()
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteCatalogFile {
    #[serde(default)]
    plugins: Vec<MarketplaceListing>,
}

/// Fetch a remote catalog JSON. Falls back to builtin on network/parse errors.
pub async fn fetch_remote_catalog(url: &str) -> Vec<MarketplaceListing> {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
    {
        Ok(c) => c,
        Err(_) => return builtin_catalog(),
    };
    match client.get(url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<RemoteCatalogFile>().await {
            Ok(file) if !file.plugins.is_empty() => file.plugins,
            _ => builtin_catalog(),
        },
        _ => builtin_catalog(),
    }
}

/// Merge remote listings over builtin (remote wins on same id).
pub fn merge_catalogs(remote: Vec<MarketplaceListing>, builtin: Vec<MarketplaceListing>) -> Vec<MarketplaceListing> {
    let mut out = builtin;
    for listing in remote {
        if let Some(slot) = out.iter_mut().find(|l| l.id == listing.id) {
            *slot = listing;
        } else {
            out.push(listing);
        }
    }
    out
}

pub fn marketplace_entries_from_catalog(
    reg: &ExtensionRegistry,
    catalog: Vec<MarketplaceListing>,
) -> Vec<MarketplaceEntry> {
    let mut out: Vec<MarketplaceEntry> = catalog
        .into_iter()
        .map(|listing| {
            let installed = reg.extensions.iter().find(|e| e.manifest.id == listing.id);
            MarketplaceEntry {
                installed: installed.is_some(),
                enabled: installed.map(|e| e.enabled).unwrap_or(false),
                local_path: installed.map(|e| e.path.clone()),
                listing,
            }
        })
        .collect();

    for ext in &reg.extensions {
        if out.iter().any(|e| e.listing.id == ext.manifest.id) {
            continue;
        }
        // Hide legacy FLAC / download packages from the Sloptify marketplace UI.
        let id = ext.manifest.id.as_str();
        if id.contains("lossless") || id.contains("flac") || id.starts_with("audio.") {
            continue;
        }
        out.push(MarketplaceEntry {
            listing: MarketplaceListing {
                id: ext.manifest.id.clone(),
                name: ext.manifest.name.clone(),
                version: ext.manifest.version.clone(),
                author: ext.manifest.author.clone().unwrap_or_default(),
                description: ext.manifest.description.clone().unwrap_or_default(),
                kind: PluginKind::Integration,
                homepage: None,
                download_url: None,
                tags: vec!["local".into()],
                sample_script: None,
            },
            installed: true,
            enabled: ext.enabled,
            local_path: Some(ext.path.clone()),
        });
    }
    out.sort_by(|a, b| a.listing.name.to_lowercase().cmp(&b.listing.name.to_lowercase()));
    out
}

fn register_installed(manifest: ExtensionManifest, dir: PathBuf) -> Result<InstalledExtension, ExtError> {
    let installed = InstalledExtension {
        manifest: manifest.clone(),
        enabled: true,
        path: dir.to_string_lossy().into_owned(),
    };
    let mut reg = load_registry();
    reg.extensions.retain(|e| e.manifest.id != manifest.id);
    reg.extensions.push(installed.clone());
    save_registry(&reg)?;
    Ok(installed)
}

fn find_manifest_dir(root: &Path) -> Result<PathBuf, ExtError> {
    let direct = root.join("manifest.json");
    if direct.is_file() {
        return Ok(root.to_path_buf());
    }
    // GitHub archive zips wrap once: repo-main/manifest.json
    if let Ok(rd) = std::fs::read_dir(root) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() && p.join("manifest.json").is_file() {
                return Ok(p);
            }
        }
    }
    Err(ExtError::Msg(
        "zip missing manifest.json (put it at the zip root)".into(),
    ))
}

fn extract_zip(bytes: &[u8], dest: &Path) -> Result<(), ExtError> {
    use std::io::Cursor;
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)
        .map_err(|e| ExtError::Msg(format!("invalid zip: {e}")))?;
    std::fs::create_dir_all(dest)?;
    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| ExtError::Msg(format!("zip entry: {e}")))?;
        let name = file
            .enclosed_name()
            .ok_or_else(|| ExtError::Msg("unsafe zip path".into()))?
            .to_owned();
        let out = dest.join(&name);
        if file.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut outfile = std::fs::File::create(&out)?;
        std::io::copy(&mut file, &mut outfile)?;
    }
    Ok(())
}

/// Download a plugin zip (or GitHub archive) and install it.
pub async fn install_from_url(url: &str) -> Result<InstalledExtension, ExtError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(8))
        .build()
        .map_err(|e| ExtError::Msg(e.to_string()))?;
    let bytes = client
        .get(url)
        .send()
        .await
        .map_err(|e| ExtError::Msg(format!("download failed: {e}")))?
        .error_for_status()
        .map_err(|e| ExtError::Msg(format!("download failed: {e}")))?
        .bytes()
        .await
        .map_err(|e| ExtError::Msg(format!("download body: {e}")))?;

    let staging = plugins_root().join(format!(".staging-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    extract_zip(&bytes, &staging)?;
    let src = find_manifest_dir(&staging)?;
    let (manifest, script) = crate::ExtensionSandbox::load_folder(&src)?;
    crate::ExtensionSandbox::check_script(&script)?;

    let dest = plugin_dir(&manifest.id);
    if dest.exists() {
        let _ = std::fs::remove_dir_all(&dest);
    }
    // Move extracted package into place.
    std::fs::create_dir_all(plugins_root())?;
    std::fs::rename(&src, &dest).or_else(|_| {
        // Cross-device fallback
        copy_dir(&src, &dest)?;
        let _ = std::fs::remove_dir_all(&src);
        Ok::<(), ExtError>(())
    })?;
    let _ = std::fs::remove_dir_all(&staging);
    register_installed(manifest, dest)
}

fn copy_dir(src: &Path, dest: &Path) -> Result<(), ExtError> {
    std::fs::create_dir_all(dest)?;
    for ent in std::fs::read_dir(src)? {
        let ent = ent?;
        let from = ent.path();
        let to = dest.join(ent.file_name());
        if from.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}


/// Install a curated listing (writes manifest + sample script locally).
/// If `download_url` is set, prefer `install_from_url` (async) from the host.
pub fn install_listing(listing: &MarketplaceListing) -> Result<InstalledExtension, ExtError> {
    if listing.download_url.as_ref().is_some_and(|u| !u.is_empty()) {
        return Err(ExtError::Msg(
            "remote zip listing — use install_from_url".into(),
        ));
    }
    let dir = plugin_dir(&listing.id);
    std::fs::create_dir_all(&dir)?;

    let manifest = ExtensionManifest {
        id: listing.id.clone(),
        name: listing.name.clone(),
        version: listing.version.clone(),
        author: Some(listing.author.clone()).filter(|s| !s.is_empty()),
        description: Some(listing.description.clone()).filter(|s| !s.is_empty()),
        actions: match listing.kind {
            PluginKind::Theme => vec![crate::ExtensionAction {
                name: "ui.theme".into(),
                why: Some("Adjust UI chrome".into()),
                hosts: Some(vec!["sloptify".into()]),
            }],
            PluginKind::Action => vec![crate::ExtensionAction {
                name: "player.transport".into(),
                why: Some("Keyboard / command actions".into()),
                hosts: Some(vec!["sloptify".into()]),
            }],
            PluginKind::Integration => vec![crate::ExtensionAction {
                name: "player.trackChanged".into(),
                why: Some("React to playback".into()),
                hosts: Some(vec!["sloptify".into()]),
            }],
            PluginKind::Source => vec![crate::ExtensionAction {
                name: "audio.source".into(),
                why: Some("Optional audio/source hook".into()),
                hosts: Some(vec!["sloptify".into()]),
            }],
        },
        kind: Some(format!("{:?}", listing.kind).to_lowercase()),
        homepage: listing.homepage.clone(),
        tags: listing.tags.clone(),
    };
    std::fs::write(
        dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    let script = listing
        .sample_script
        .clone()
        .unwrap_or_else(|| "// Sloptify plugin\nexport const ready = true;\n".into());
    std::fs::write(dir.join("main.js"), script)?;

    let installed = InstalledExtension {
        manifest,
        enabled: true,
        path: dir.to_string_lossy().into_owned(),
    };

    let mut reg = load_registry();
    reg.extensions.retain(|e| e.manifest.id != listing.id);
    reg.extensions.push(installed.clone());
    save_registry(&reg)?;
    Ok(installed)
}

pub fn uninstall(id: &str) -> Result<(), ExtError> {
    let mut reg = load_registry();
    if let Some(ext) = reg.extensions.iter().find(|e| e.manifest.id == id).cloned() {
        let path = PathBuf::from(&ext.path);
        if path.starts_with(plugins_root()) && path.exists() {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
    reg.extensions.retain(|e| e.manifest.id != id);
    save_registry(&reg)
}

pub fn set_enabled(id: &str, enabled: bool) -> Result<(), ExtError> {
    let mut reg = load_registry();
    let Some(ext) = reg.extensions.iter_mut().find(|e| e.manifest.id == id) else {
        return Err(ExtError::Msg(format!("plugin not installed: {id}")));
    };
    ext.enabled = enabled;
    save_registry(&reg)
}

/// Register a developer folder (does not copy files).
pub fn link_local_folder(dir: impl AsRef<Path>) -> Result<InstalledExtension, ExtError> {
    let dir = dir.as_ref();
    let (manifest, script) = crate::ExtensionSandbox::load_folder(dir)?;
    crate::ExtensionSandbox::check_script(&script)?;
    let installed = InstalledExtension {
        manifest: manifest.clone(),
        enabled: true,
        path: dir.to_string_lossy().into_owned(),
    };
    let mut reg = load_registry();
    reg.extensions.retain(|e| e.manifest.id != manifest.id);
    reg.extensions.push(installed.clone());
    save_registry(&reg)?;
    Ok(installed)
}

pub fn enabled_ids(reg: &ExtensionRegistry) -> Vec<String> {
    reg.extensions
        .iter()
        .filter(|e| e.enabled)
        .map(|e| e.manifest.id.clone())
        .collect()
}
