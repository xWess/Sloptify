//! Catalog reads through the page's GraphQL loader (Sloptify `gql.query`).

use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::bridge::Bridge;

/// Persisted query hashes from Sloptify `gql-ops.json` (Web Player).
const HOME_HASH: &str = "5fb7da7a03ee2776c0670856609cb85a189a3cecc36cc160b91cde79063b9cf1";
const SEARCH_HASH: &str = "eef7cc54888d91bdd6802623477873caa3948ae173a0c34fd86827b267e94c03";
const LIBRARY_V3_HASH: &str = "390c78e5b951029bad359785e69b07b536a509c581cbcd0aded5e5067f187455";
const LIKED_TRACKS_HASH: &str = "087278b20b743578a6262c2b0b4bcd20d879c503cc359a2285baf083ef944240";
const PLAYLIST_HASH: &str = "21ea11a6ac75a5f3e01fd365d29c18e0a4a8b7dc09f89bbb5d73910190c6acaf";
const ALBUM_HASH: &str = "11c9cad9cb24c4f9e25ef9456f68621cfd6006a90f679e729c1d2f2fd70dfe16";
const ARTIST_OVERVIEW_HASH: &str =
    "9f8134ef565e78621f1e1793555bd6633c5ac144ae0f89604ed3ae3f80b3c8e6";
const PROFILE_HASH: &str = "08ffb4730af3746e04a8301396f20875dbbce10c75243803091a9274eacc8ac0";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeFeed {
    pub greeting: String,
    pub chips: Vec<HomeChip>,
    pub shelves: Vec<HomeShelf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeChip {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeShelf {
    pub title: String,
    pub items: Vec<HomeItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeItem {
    pub uri: String,
    pub name: String,
    pub subtitle: String,
    pub image: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub uri: String,
    pub name: String,
    pub username: String,
    pub image: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContextKind {
    Playlist,
    Album,
    Artist,
    Liked,
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextTrack {
    pub uri: String,
    pub name: String,
    pub artists: String,
    pub album: String,
    pub image: Option<String>,
    pub duration_ms: i64,
    pub uid: Option<String>,
    pub album_uri: Option<String>,
    pub artist_uri: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPage {
    pub uri: String,
    pub kind: ContextKind,
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub image: Option<String>,
    /// Popular / track list (artist top tracks, playlist songs, …).
    pub tracks: Vec<ContextTrack>,
    /// Legacy single related row (kept for callers); prefer [shelves].
    pub related: Vec<HomeItem>,
    /// Artist discography / appears-on / related (and similar context shelves).
    #[serde(default)]
    pub shelves: Vec<HomeShelf>,
    pub total: i64,
    pub next_offset: Option<i64>,
    pub loading: bool,
    pub error: String,
}

impl Default for ContextKind {
    fn default() -> Self {
        Self::Other
    }
}

impl ContextKind {
    pub fn from_uri(uri: &str) -> Self {
        if uri.contains(":playlist:") {
            Self::Playlist
        } else if uri.contains(":album:") {
            Self::Album
        } else if uri.contains(":artist:") {
            Self::Artist
        } else if uri.contains(":collection:tracks") || uri.ends_with(":collection:tracks") {
            Self::Liked
        } else {
            Self::Other
        }
    }
}

/// True when opening should show a track list instead of immediate play.
pub fn is_browsable_uri(uri: &str) -> bool {
    matches!(
        ContextKind::from_uri(uri),
        ContextKind::Playlist | ContextKind::Album | ContextKind::Artist | ContextKind::Liked
    ) || uri.starts_with("spotify:user:") && uri.contains(":playlist:")
}

pub struct SpotifyCatalog {
    bridge: Arc<Bridge>,
}

impl SpotifyCatalog {
    pub fn new(bridge: Arc<Bridge>) -> Self {
        Self { bridge }
    }

    pub async fn home(&self, facet: &str) -> Result<HomeFeed> {
        let tz = iana_timezone();
        let raw = self
            .bridge
            .call(
                "gql.query",
                json!({
                    "name": "home",
                    "operation": "query",
                    "hash": HOME_HASH,
                    "variables": {
                        "timeZone": tz,
                        "sp_t": "",
                        "facet": facet,
                        "sectionItemsLimit": 10,
                        "homeEndUserIntegration": "INTEGRATION_WEB_PLAYER",
                        "includeEpisodeContentRatingsV2": true,
                        "includeHomeChapterVideoCards": false
                    }
                }),
            )
            .await
            .map_err(|e| anyhow!("{e}"))?;

        Ok(parse_home(&raw))
    }

    pub async fn search(&self, q: &str) -> Result<Vec<HomeItem>> {
        let raw = self
            .bridge
            .call(
                "gql.query",
                json!({
                    "name": "searchDesktop",
                    "operation": "query",
                    "hash": SEARCH_HASH,
                    "variables": {
                        "searchTerm": q,
                        "offset": 0,
                        "limit": 20,
                        "numberOfTopResults": 5,
                        "includeAudiobooks": true,
                        "includeArtistHasConcertsField": false,
                        "includePreReleases": true,
                        "includeAuthors": false
                    }
                }),
            )
            .await;
        match raw {
            Ok(v) => Ok(parse_search(&v)),
            Err(_) => Ok(Vec::new()),
        }
    }

    pub async fn play_uri(&self, context_uri: &str, track_uri: Option<&str>) -> Result<()> {
        self.play_skip(context_uri, track_uri, None, None).await
    }

    /// Play a context, optionally skipping to a track by uri/uid/index (Web Player skipTo).
    pub async fn play_skip(
        &self,
        context_uri: &str,
        track_uri: Option<&str>,
        track_uid: Option<&str>,
        index: Option<i64>,
    ) -> Result<()> {
        // Home cards can be track / album / playlist / artist / collection.
        let (context, track) = match track_uri {
            Some(t) => (context_uri.to_string(), Some(t.to_string())),
            None if context_uri.starts_with("spotify:track:") => {
                // Playing a lone track: use the track as context (Web Player accepts it).
                (context_uri.to_string(), Some(context_uri.to_string()))
            }
            None => (context_uri.to_string(), None),
        };
        let mut args = json!({ "contextUri": context });
        if let Some(t) = track {
            args["trackUri"] = json!(t);
        }
        if let Some(uid) = track_uid.filter(|s| !s.is_empty()) {
            args["trackUid"] = json!(uid);
        }
        if let Some(i) = index {
            args["index"] = json!(i);
        }
        tracing::info!(%context_uri, "player.play");
        // Cap wait: Sloptify recovers via reload rather than sitting on a 20s hang.
        match self
            .bridge
            .call_timeout("player.play", args.clone(), std::time::Duration::from_secs(12))
            .await
        {
            Ok(_) => Ok(()),
            Err(crate::bridge::BridgeError::Timeout) | Err(crate::bridge::BridgeError::NotReady) => {
                tracing::info!("play stalled — reloading Web Player then retrying");
                self.bridge.reload_page().await.map_err(|e| anyhow!("{e}"))?;
                self.bridge
                    .call_timeout("player.play", args, std::time::Duration::from_secs(12))
                    .await
                    .map_err(|e| anyhow!("{e}"))?;
                Ok(())
            }
            Err(err) => Err(anyhow!("{err}")),
        }
    }

    /// Fallback when the user hits play with nothing loaded.
    pub async fn play_liked_or_resume(&self) -> Result<()> {
        self.play_uri("spotify:collection:tracks", None).await
    }

    /// Sloptify `SpotifyLibrary.items` — libraryV3 (playlists, liked, etc.).
    pub async fn library(&self, filter: &str) -> Result<Vec<HomeItem>> {
        let filters: Vec<String> = if filter.is_empty() {
            vec![]
        } else {
            vec![filter.to_string()]
        };
        let raw = self
            .bridge
            .call(
                "gql.query",
                json!({
                    "name": "libraryV3",
                    "operation": "query",
                    "hash": LIBRARY_V3_HASH,
                    "variables": {
                        "filters": filters,
                        "order": "Recents",
                        "textFilter": "",
                        "features": ["LIKED_SONGS", "YOUR_EPISODES_V2", "PRERELEASES", "PRERELEASES_V2", "CLIPS", "EVENTS"],
                        "limit": 50,
                        "offset": 0,
                        "flatten": false,
                        "expandedFolders": [],
                        "folderUri": null,
                        "includeFoldersWhenFlattening": true
                    }
                }),
            )
            .await
            .map_err(|e| anyhow!("{e}"))?;
        Ok(parse_library(&raw))
    }

    /// Liked Songs page (Sloptify `likedTracks`).
    pub async fn liked_tracks(&self, limit: i64) -> Result<Vec<HomeItem>> {
        let raw = self
            .bridge
            .call(
                "gql.query",
                json!({
                    "name": "fetchLibraryTracks",
                    "operation": "query",
                    "hash": LIKED_TRACKS_HASH,
                    "variables": { "offset": 0, "limit": limit }
                }),
            )
            .await
            .map_err(|e| anyhow!("{e}"))?;
        Ok(parse_liked(&raw))
    }

    /// Sloptify `users.me` via `profileAttributes`.
    pub async fn me(&self) -> Result<UserProfile> {
        let raw = self
            .bridge
            .call(
                "gql.query",
                json!({
                    "name": "profileAttributes",
                    "operation": "query",
                    "hash": PROFILE_HASH,
                    "variables": {}
                }),
            )
            .await
            .map_err(|e| anyhow!("{e}"))?;
        Ok(parse_profile(&raw))
    }

    /// Open a playlist / album / artist / Liked Songs page (Sloptify context screens).
    pub async fn context_page(&self, uri: &str, offset: i64, limit: i64) -> Result<ContextPage> {
        let kind = ContextKind::from_uri(uri);
        match kind {
            ContextKind::Playlist => self.playlist_page(uri, offset, limit).await,
            ContextKind::Album => self.album_page(uri, offset, limit).await,
            ContextKind::Artist => self.artist_page(uri).await,
            ContextKind::Liked => self.liked_page(offset, limit).await,
            ContextKind::Other => Err(anyhow!("not a browsable context: {uri}")),
        }
    }

    async fn playlist_page(&self, uri: &str, offset: i64, limit: i64) -> Result<ContextPage> {
        let raw = self
            .bridge
            .call(
                "gql.query",
                json!({
                    "name": "fetchPlaylistContents",
                    "operation": "query",
                    "hash": PLAYLIST_HASH,
                    "variables": {
                        "uri": uri,
                        "offset": offset,
                        "limit": limit,
                        "includeEpisodeContentRatingsV2": true
                    }
                }),
            )
            .await
            .map_err(|e| anyhow!("{e}"))?;
        Ok(parse_playlist_page(&raw, uri, offset, limit))
    }

    async fn album_page(&self, uri: &str, offset: i64, limit: i64) -> Result<ContextPage> {
        let raw = self
            .bridge
            .call(
                "gql.query",
                json!({
                    "name": "getAlbum",
                    "operation": "query",
                    "hash": ALBUM_HASH,
                    "variables": {
                        "uri": uri,
                        "locale": "",
                        "offset": offset,
                        "limit": limit
                    }
                }),
            )
            .await
            .map_err(|e| anyhow!("{e}"))?;
        Ok(parse_album_page(&raw, uri, offset, limit))
    }

    async fn artist_page(&self, uri: &str) -> Result<ContextPage> {
        let raw = self
            .bridge
            .call(
                "gql.query",
                json!({
                    "name": "queryArtistOverview",
                    "operation": "query",
                    "hash": ARTIST_OVERVIEW_HASH,
                    "variables": {
                        "uri": uri,
                        "locale": "",
                        "preReleaseV2": false
                    }
                }),
            )
            .await
            .map_err(|e| anyhow!("{e}"))?;
        Ok(parse_artist_page(&raw, uri))
    }

    async fn liked_page(&self, offset: i64, limit: i64) -> Result<ContextPage> {
        let raw = self
            .bridge
            .call(
                "gql.query",
                json!({
                    "name": "fetchLibraryTracks",
                    "operation": "query",
                    "hash": LIKED_TRACKS_HASH,
                    "variables": { "offset": offset, "limit": limit }
                }),
            )
            .await
            .map_err(|e| anyhow!("{e}"))?;
        let tracks = parse_liked_tracks(&raw);
        let total = raw
            .pointer("/data/me/library/tracks/totalCount")
            .and_then(|v| v.as_i64())
            .unwrap_or(tracks.len() as i64);
        let next_offset = {
            let n = offset + limit;
            if n < total {
                Some(n)
            } else {
                None
            }
        };
        let error = if tracks.is_empty() && total > 0 && offset == 0 {
            "Couldn’t parse Liked Songs — try again.".into()
        } else {
            String::new()
        };
        Ok(ContextPage {
            uri: "spotify:collection:tracks".into(),
            kind: ContextKind::Liked,
            title: "Liked Songs".into(),
            subtitle: format!("{total} songs"),
            description: String::new(),
            image: None,
            tracks,
            related: Vec::new(),
            shelves: Vec::new(),
            total,
            next_offset,
            loading: false,
            error,
        })
    }
}

fn parse_playlist_page(raw: &Value, uri: &str, offset: i64, limit: i64) -> ContextPage {
    let p = raw
        .pointer("/data/playlistV2")
        .or_else(|| raw.pointer("/playlistV2"))
        .cloned()
        .unwrap_or(Value::Null);
    let mut title = str_path(&p, &["name"])
        .or_else(|| str_path(&p, &["attributes", "name"]))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Playlist".into());
    let subtitle = str_path(&p, &["ownerV2", "data", "name"])
        .or_else(|| str_path(&p, &["owner", "displayName"]))
        .unwrap_or_default();
    let description = str_path(&p, &["description"])
        .or_else(|| str_path(&p, &["attributes", "description"]))
        .unwrap_or_default();
    let mut image = first_image(&p)
        .or_else(|| {
            p.pointer("/images/items/0/sources/0/url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .or_else(|| {
            p.pointer("/attributes/picture")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });
    let content = p
        .pointer("/content")
        .cloned()
        .or_else(|| p.pointer("/items").cloned())
        .unwrap_or(Value::Null);
    let total = content
        .get("totalCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let tracks = content
        .pointer("/items")
        .or_else(|| content.as_array().map(|_| &content))
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let uid = str_path(row, &["uid"]);
            let data = row
                .pointer("/itemV2/data")
                .or_else(|| row.pointer("/item/data"))
                .or_else(|| row.pointer("/itemV2"))
                .or_else(|| row.pointer("/track"))
                .unwrap_or(row);
            parse_context_track(data, uid)
        })
        .collect::<Vec<_>>();
    if image.is_none() {
        image = tracks.iter().find_map(|t| t.image.clone());
    }
    let next_offset = {
        let n = offset + limit;
        if n < total {
            Some(n)
        } else {
            None
        }
    };
    if title.is_empty() {
        title = "Playlist".into();
    }
    ContextPage {
        uri: uri.to_string(),
        kind: ContextKind::Playlist,
        title,
        subtitle,
        description,
        image,
        tracks,
        related: Vec::new(),
        shelves: Vec::new(),
        total,
        next_offset,
        loading: false,
        error: String::new(),
    }
}

fn parse_album_page(raw: &Value, uri: &str, offset: i64, limit: i64) -> ContextPage {
    let a = raw
        .pointer("/data/albumUnion")
        .or_else(|| raw.pointer("/albumUnion"))
        .cloned()
        .unwrap_or(Value::Null);
    let title = str_path(&a, &["name"]).unwrap_or_else(|| "Album".into());
    let artists = artist_names(&a);
    let image = first_image(&a);
    let tracks_node = a.pointer("/tracksV2").cloned().unwrap_or(Value::Null);
    let total = tracks_node
        .get("totalCount")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let album_name = title.clone();
    let tracks = tracks_node
        .pointer("/items")
        .and_then(|arr| arr.as_array())
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let uid = str_path(row, &["uid"]);
            let data = row.pointer("/track").unwrap_or(row);
            let mut t = parse_context_track(data, uid)?;
            if t.album.is_empty() {
                t.album = album_name.clone();
            }
            if t.image.is_none() {
                t.image = image.clone();
            }
            Some(t)
        })
        .collect::<Vec<_>>();
    let next_offset = {
        let n = offset + limit;
        if n < total {
            Some(n)
        } else {
            None
        }
    };
    ContextPage {
        uri: uri.to_string(),
        kind: ContextKind::Album,
        title,
        subtitle: artists,
        description: str_path(&a, &["date", "isoString"])
            .map(|s| s.get(..4).unwrap_or(s.as_str()).to_string())
            .unwrap_or_default(),
        image,
        tracks,
        related: Vec::new(),
        shelves: Vec::new(),
        total,
        next_offset,
        loading: false,
        error: String::new(),
    }
}

fn parse_artist_page(raw: &Value, uri: &str) -> ContextPage {
    let a = raw
        .pointer("/data/artistUnion")
        .or_else(|| raw.pointer("/artistUnion"))
        .cloned()
        .unwrap_or(Value::Null);
    let title = str_path(&a, &["profile", "name"]).unwrap_or_else(|| "Artist".into());
    let image = a
        .pointer("/visuals/avatarImage/sources/0/url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| first_image(&a));
    let monthly = a
        .pointer("/stats/monthlyListeners")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let tracks = a
        .pointer("/discography/topTracks/items")
        .and_then(|arr| arr.as_array())
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let uid = str_path(row, &["uid"]);
            let data = row.pointer("/track").unwrap_or(row);
            parse_context_track(data, uid)
        })
        .collect::<Vec<_>>();

    // Discography: overview returns albums / singles / compilations as
    // items[].releases.items[0]; popularReleasesAlbums are release objects directly.
    let popular = collect_releases(
        a.pointer("/discography/popularReleasesAlbums/items"),
        false,
    );
    let albums = collect_releases(a.pointer("/discography/albums/items"), true);
    let singles = collect_releases(a.pointer("/discography/singles/items"), true);
    let comps = collect_releases(a.pointer("/discography/compilations/items"), true);
    let appears = collect_releases(a.pointer("/relatedContent/appearsOn/items"), true);
    let related_artists = a
        .pointer("/relatedContent/relatedArtists/items")
        .and_then(|arr| arr.as_array())
        .into_iter()
        .flatten()
        .filter_map(parse_card)
        .collect::<Vec<_>>();

    let mut shelves = Vec::new();
    // Prefer structured discography; fall back to popular-releases row.
    if !albums.is_empty() {
        shelves.push(HomeShelf {
            title: "Albums".into(),
            items: albums,
        });
    }
    if !singles.is_empty() {
        shelves.push(HomeShelf {
            title: "Singles and EPs".into(),
            items: singles,
        });
    }
    if !comps.is_empty() {
        shelves.push(HomeShelf {
            title: "Compilations".into(),
            items: comps,
        });
    }
    if shelves.is_empty() && !popular.is_empty() {
        shelves.push(HomeShelf {
            title: "Discography".into(),
            items: popular.clone(),
        });
    }
    if !appears.is_empty() {
        shelves.push(HomeShelf {
            title: "Appears on".into(),
            items: appears,
        });
    }
    if !related_artists.is_empty() {
        shelves.push(HomeShelf {
            title: "Fans also like".into(),
            items: related_artists,
        });
    }

    let related = if popular.is_empty() {
        shelves
            .iter()
            .find(|s| s.title == "Albums")
            .map(|s| s.items.clone())
            .unwrap_or_default()
    } else {
        popular
    };
    let total = tracks.len() as i64;
    ContextPage {
        uri: uri.to_string(),
        kind: ContextKind::Artist,
        title,
        subtitle: if monthly > 0 {
            format!("{} monthly listeners", format_count(monthly))
        } else {
            "Artist".into()
        },
        description: str_path(&a, &["profile", "biography", "text"]).unwrap_or_default(),
        image,
        tracks,
        related,
        shelves,
        total,
        next_offset: None,
        loading: false,
        error: String::new(),
    }
}

/// Artist discography rows: either a release object or `{ releases: { items: [release] } }`.
fn collect_releases(node: Option<&Value>, nested: bool) -> Vec<HomeItem> {
    node.and_then(|arr| arr.as_array())
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let data = if nested {
                row.pointer("/releases/items/0")
                    .or_else(|| row.pointer("/data"))
                    .unwrap_or(row)
            } else {
                row.pointer("/data").unwrap_or(row)
            };
            let mut item = parse_card(data)?;
            if item.subtitle.is_empty() {
                if let Some(y) = data.pointer("/date/year").and_then(|v| v.as_i64()) {
                    let kind = str_path(data, &["type"]).unwrap_or_default();
                    item.subtitle = if kind.is_empty() {
                        y.to_string()
                    } else {
                        format!("{y} · {}", kind.to_lowercase())
                    };
                }
            }
            Some(item)
        })
        .collect()
}

fn parse_liked_tracks(raw: &Value) -> Vec<ContextTrack> {
    // Sloptify `SpotifyLibrary.likedTracks`: uri often lives on `track._uri` when
    // `track.data.uri` is missing.
    raw.pointer("/data/me/library/tracks/items")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let data = row
                .pointer("/track/data")
                .or_else(|| row.pointer("/itemV2/data"))
                .or_else(|| row.pointer("/track"))
                .unwrap_or(row);
            let uri = str_path(data, &["uri"])
                .or_else(|| str_path(row, &["track", "_uri"]))
                .or_else(|| str_path(row, &["itemV2", "_uri"]))
                .or_else(|| str_path(data, &["_uri"]))?;
            parse_context_track_with_uri(data, uri, None)
        })
        .collect()
}

fn parse_context_track(data: &Value, uid: Option<String>) -> Option<ContextTrack> {
    let uri = str_path(data, &["uri"])?;
    parse_context_track_with_uri(data, uri, uid)
}

fn parse_context_track_with_uri(
    data: &Value,
    uri: String,
    uid: Option<String>,
) -> Option<ContextTrack> {
    if !(uri.starts_with("spotify:track:")
        || uri.starts_with("spotify:episode:")
        || uri.starts_with("spotify:local:"))
    {
        return None;
    }
    let name = str_path(data, &["name"]).unwrap_or_else(|| uri.clone());
    let artists = artist_names(data);
    let album = str_path(data, &["albumOfTrack", "name"])
        .or_else(|| str_path(data, &["album", "name"]))
        .unwrap_or_default();
    let album_uri = str_path(data, &["albumOfTrack", "uri"])
        .or_else(|| str_path(data, &["album", "uri"]))
        .filter(|u| u.starts_with("spotify:album:"));
    let artist_uri = data
        .pointer("/artists/items/0/uri")
        .or_else(|| data.pointer("/artists/items/0/data/uri"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            data.pointer("/artists/0/uri")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .filter(|u| u.starts_with("spotify:artist:"));
    let image = first_image(data).or_else(|| {
        data.pointer("/albumOfTrack/coverArt/sources/0/url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    });
    let duration_ms = data
        .pointer("/duration/totalMilliseconds")
        .or_else(|| data.pointer("/trackDuration/totalMilliseconds"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    Some(ContextTrack {
        uri,
        name,
        artists,
        album,
        image,
        duration_ms,
        uid,
        album_uri,
        artist_uri,
    })
}

fn parse_profile(raw: &Value) -> UserProfile {
    let p = raw
        .pointer("/data/me/profile")
        .or_else(|| raw.pointer("/me/profile"))
        .cloned()
        .unwrap_or(Value::Null);
    let name = str_path(&p, &["name"])
        .or_else(|| str_path(&p, &["username"]))
        .unwrap_or_else(|| "You".into());
    let username = str_path(&p, &["username"]).unwrap_or_default();
    let uri = str_path(&p, &["uri"]).unwrap_or_default();
    let image = p
        .pointer("/avatar/sources/0/url")
        .or_else(|| p.pointer("/avatar/sources/1/url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| first_image(&p));
    UserProfile {
        uri,
        name,
        username,
        image,
    }
}

fn artist_names(data: &Value) -> String {
    if let Some(arr) = data.pointer("/artists/items").and_then(|a| a.as_array()) {
        let names: Vec<String> = arr
            .iter()
            .filter_map(|a| {
                str_path(a, &["profile", "name"]).or_else(|| str_path(a, &["name"]))
            })
            .collect();
        if !names.is_empty() {
            return names.join(", ");
        }
    }
    if let Some(arr) = data.pointer("/artists").and_then(|a| a.as_array()) {
        let names: Vec<String> = arr
            .iter()
            .filter_map(|a| str_path(a, &["name"]))
            .collect();
        if !names.is_empty() {
            return names.join(", ");
        }
    }
    String::new()
}

fn format_count(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn parse_library(raw: &Value) -> Vec<HomeItem> {
    let rows = raw
        .pointer("/data/me/libraryV3/items")
        .or_else(|| raw.pointer("/libraryV3/items"))
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten();
    rows.filter_map(|row| {
        let content = row.pointer("/item").unwrap_or(row);
        parse_card(content.pointer("/data").unwrap_or(content))
            .or_else(|| parse_card(content))
    })
    .collect()
}

fn parse_liked(raw: &Value) -> Vec<HomeItem> {
    let rows = raw
        .pointer("/data/me/library/tracks/items")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten();
    rows.filter_map(|row| {
        let data = row
            .pointer("/track/data")
            .or_else(|| row.pointer("/itemV2/data"))
            .or_else(|| row.pointer("/track"))
            .unwrap_or(row);
        let mut card = parse_card(data);
        if card.as_ref().is_none_or(|c| c.uri.is_empty()) {
            if let Some(uri) = str_path(row, &["track", "_uri"])
                .or_else(|| str_path(data, &["_uri"]))
            {
                if let Some(c) = card.as_mut() {
                    c.uri = uri;
                } else {
                    card = Some(HomeItem {
                        uri,
                        name: str_path(data, &["name"]).unwrap_or_else(|| "Track".into()),
                        subtitle: artist_names(data),
                        image: first_image(data),
                    });
                }
            }
        }
        card.filter(|c| !c.uri.is_empty())
    })
    .collect()
}

fn iana_timezone() -> String {
    std::env::var("TZ").unwrap_or_else(|_| "UTC".into())
}

fn parse_home(raw: &Value) -> HomeFeed {
    let home = raw
        .pointer("/data/home")
        .or_else(|| raw.pointer("/home"))
        .cloned()
        .unwrap_or(Value::Null);

    let greeting = str_path(&home, &["greeting", "transformedLabel"]).unwrap_or_default();

    let chips = home
        .pointer("/homeChips")
        .and_then(|c| c.as_array())
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some(HomeChip {
                id: str_path(c, &["id"])?,
                label: str_path(c, &["label", "transformedLabel"])?,
            })
        })
        .collect();

    let shelves = home
        .pointer("/sectionContainer/sections/items")
        .and_then(|s| s.as_array())
        .into_iter()
        .flatten()
        .filter_map(parse_shelf)
        .collect();

    HomeFeed {
        greeting,
        chips,
        shelves,
    }
}

fn parse_shelf(section: &Value) -> Option<HomeShelf> {
    let mut title = str_path(section, &["data", "title", "transformedLabel"])
        .or_else(|| str_path(section, &["data", "title", "text"]))
        .or_else(|| str_path(section, &["data", "title"]))
        .unwrap_or_default();
    if title.is_empty() || title.starts_with("spotify:") {
        let uri = str_path(section, &["uri"]).unwrap_or_default();
        title = if uri.contains("recents") || title.contains("recents") {
            "Recents".into()
        } else if !title.is_empty() && !title.starts_with("spotify:") {
            title
        } else {
            "For you".into()
        };
    }
    let items = section
        .pointer("/sectionItems/items")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .filter_map(|it| parse_card(it.pointer("/content").unwrap_or(it)))
        .collect::<Vec<_>>();
    if items.is_empty() {
        return None;
    }
    Some(HomeShelf { title, items })
}

fn parse_card(content: &Value) -> Option<HomeItem> {
    let data = content.pointer("/data").unwrap_or(content);
    let uri = str_path(data, &["uri"])
        .or_else(|| str_path(content, &["uri"]))
        .unwrap_or_default();
    if uri.is_empty() {
        return None;
    }
    let name = str_path(data, &["name"])
        .or_else(|| str_path(data, &["title"]))
        .or_else(|| str_path(data, &["profile", "name"]))
        .unwrap_or_default();
    let subtitle = str_path(data, &["publisher", "name"])
        .or_else(|| {
            data.pointer("/artists/items")
                .and_then(|a| a.as_array())
                .and_then(|a| a.first())
                .and_then(|a| str_path(a, &["profile", "name"]))
        })
        .or_else(|| str_path(data, &["ownerV2", "data", "name"]))
        .unwrap_or_default();
    let image = first_image(data);
    // Drop nameless / placeholder cards (show up as grey "Artist" tiles).
    if name.is_empty() || (name == "Artist" && image.is_none()) {
        return None;
    }
    let name = if name.starts_with("spotify:") {
        uri_label(&uri)
    } else {
        name
    };
    Some(HomeItem {
        uri,
        name,
        subtitle,
        image,
    })
}

fn uri_label(uri: &str) -> String {
    if uri.contains("recents") {
        "Recents".into()
    } else if uri.contains(":playlist:") {
        "Playlist".into()
    } else if uri.contains(":album:") {
        "Album".into()
    } else if uri.contains(":artist:") {
        "Artist".into()
    } else {
        uri.rsplit(':').next().unwrap_or(uri).to_string()
    }
}

fn parse_search(raw: &Value) -> Vec<HomeItem> {
    let mut out = Vec::new();
    for path in [
        "/data/searchV2/tracksV2/items",
        "/data/searchV2/topResultsV2/itemsV2",
        "/data/searchV2/albums/items",
        "/data/searchV2/playlists/items",
    ] {
        if let Some(arr) = raw.pointer(path).and_then(|a| a.as_array()) {
            for it in arr {
                let content = it
                    .pointer("/item")
                    .or_else(|| it.pointer("/data"))
                    .unwrap_or(it);
                if let Some(card) = parse_card(content) {
                    out.push(card);
                }
            }
        }
    }
    out
}

fn first_image(data: &Value) -> Option<String> {
    // Same order as Sloptify `Mappers.kt` card(): coverArt → artist avatar → images → …
    for path in [
        "/coverArt/sources/0/url",
        "/visuals/avatarImage/sources/0/url",
        "/visuals/avatarImage/sources/1/url",
        "/images/items/0/sources/0/url",
        "/images/items/0/sources/1/url",
        "/avatar/sources/0/url",
        "/image/sources/0/url",
        "/visualIdentity/image/0/url",
        "/albumOfTrack/coverArt/sources/0/url",
        "/avatars/0/sources/0/url",
        "/headerImage/sources/0/url",
        "/visuals/headerImage/sources/0/url",
    ] {
        if let Some(u) = data.pointer(path).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            return Some(u.to_string());
        }
    }
    // Rare: sources array without nested items wrapper.
    if let Some(arr) = data.pointer("/visuals/avatarImage/sources").and_then(|v| v.as_array()) {
        for src in arr {
            if let Some(u) = src.get("url").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                return Some(u.to_string());
            }
        }
    }
    None
}

fn str_path(v: &Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for p in path {
        cur = cur.get(*p)?;
    }
    cur.as_str().map(|s| s.to_string())
}
