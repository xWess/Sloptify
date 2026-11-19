//! In-app plugin marketplace + host effects for enabled packages.

use sloptify_ext::{
    marketplace_entries_from_catalog, merge_catalogs, MarketplaceEntry, MarketplaceListing,
};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PluginsState {
    pub entries: Vec<MarketplaceEntry>,
    pub status: String,
    pub enabled: Vec<String>,
    pub busy: bool,
}

impl Default for PluginsState {
    fn default() -> Self {
        Self::refresh()
    }
}

impl PluginsState {
    pub fn refresh() -> Self {
        let reg = sloptify_ext::load_registry();
        let enabled = sloptify_ext::enabled_ids(&reg);
        let entries = marketplace_entries_from_catalog(&reg, sloptify_ext::builtin_catalog());
        Self {
            entries,
            status: String::new(),
            enabled,
            busy: false,
        }
    }

    fn from_catalog(catalog: Vec<MarketplaceListing>, status: String) -> Self {
        let reg = sloptify_ext::load_registry();
        let enabled = sloptify_ext::enabled_ids(&reg);
        let entries = marketplace_entries_from_catalog(&reg, catalog);
        Self {
            entries,
            status,
            enabled,
            busy: false,
        }
    }

    /// Pull community catalog JSON (falls back to builtin).
    pub async fn refresh_remote() -> Self {
        let url = sloptify_ext::default_catalog_url();
        let remote = sloptify_ext::fetch_remote_catalog(&url).await;
        let catalog = merge_catalogs(remote, sloptify_ext::builtin_catalog());
        Self::from_catalog(catalog, String::new())
    }

    pub async fn install(id: &str) -> Self {
        let mut state = Self::refresh_remote().await;
        let Some(entry) = state.entries.iter().find(|e| e.listing.id == id).cloned() else {
            state.status = format!("Unknown plugin: {id}");
            return state;
        };

        let result = if let Some(url) = entry
            .listing
            .download_url
            .as_ref()
            .filter(|u| !u.is_empty())
        {
            sloptify_ext::install_from_url(url).await.map(|_| ())
        } else {
            sloptify_ext::install_listing(&entry.listing).map(|_| ())
        };

        match result {
            Ok(()) => {
                state = Self::refresh_remote().await;
                state.status = format!("Installed · {}", entry.listing.name);
            }
            Err(err) => state.status = format!("Install failed: {err}"),
        }
        state
    }

    pub async fn install_url(url: &str) -> Self {
        let url = url.trim();
        if url.is_empty() {
            let mut state = Self::refresh();
            state.status = "Paste a plugin zip URL first".into();
            return state;
        }
        match sloptify_ext::install_from_url(url).await {
            Ok(ext) => {
                let mut state = Self::refresh_remote().await;
                state.status = format!("Installed · {}", ext.manifest.name);
                state
            }
            Err(err) => {
                let mut state = Self::refresh_remote().await;
                state.status = format!("Install failed: {err}");
                state
            }
        }
    }

    pub fn uninstall(id: &str) -> Self {
        let mut state = Self::refresh();
        match sloptify_ext::uninstall(id) {
            Ok(()) => {
                state = Self::refresh();
                state.status = "Plugin removed".into();
            }
            Err(err) => state.status = format!("Remove failed: {err}"),
        }
        state
    }

    pub fn set_enabled(id: &str, enabled: bool) -> Self {
        let mut state = Self::refresh();
        match sloptify_ext::set_enabled(id, enabled) {
            Ok(()) => {
                state = Self::refresh();
                state.status = if enabled {
                    match id {
                        "sloptify.dense-lists" => "Dense Lists on — tighter rows".into(),
                        "sloptify.keyboard-nav" => {
                            "Keyboard Nav on — Space / j / k / arrows".into()
                        }
                        "sloptify.now-playing-notify" => {
                            "Desktop Notify on — track changes ping the shell".into()
                        }
                        "community.scrobble-hook" => {
                            "Scrobble Hook on — ready for host.scrobble wiring".into()
                        }
                        _ => "Plugin enabled".into(),
                    }
                } else {
                    "Plugin disabled".into()
                };
            }
            Err(err) => state.status = format!("Update failed: {err}"),
        }
        state
    }

    pub fn is_on(&self, id: &str) -> bool {
        self.enabled.iter().any(|e| e == id)
    }

    pub fn has_theme_dense(&self) -> bool {
        self.is_on("sloptify.dense-lists")
    }

    pub fn has_keyboard_nav(&self) -> bool {
        self.is_on("sloptify.keyboard-nav")
    }

    pub fn has_desktop_notify(&self) -> bool {
        self.is_on("sloptify.now-playing-notify")
    }

    pub fn has_scrobble_hook(&self) -> bool {
        self.is_on("community.scrobble-hook")
    }
}

/// Fire a desktop notification when the track changes (Linux `notify-send`).
pub fn notify_track_changed(title: &str, artist: &str) {
    let body = if artist.is_empty() {
        title.to_string()
    } else {
        format!("{artist} — {title}")
    };
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "Sloptify", "-u", "low", "Now playing", &body])
        .spawn();
}
