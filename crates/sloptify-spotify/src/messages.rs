use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum BridgeMessage {
    #[serde(rename = "ready")]
    Ready {
        #[serde(default)]
        version: Option<i64>,
    },
    #[serde(rename = "state")]
    State {
        state: Option<PlayerState>,
    },
    #[serde(rename = "queue")]
    Queue {
        queue: Queue,
    },
    #[serde(rename = "result")]
    Result {
        id: i64,
        ok: bool,
        #[serde(default)]
        value: Option<Value>,
        #[serde(default)]
        error: Option<String>,
    },
    #[serde(rename = "error")]
    Error {
        error: String,
    },
    /// Bridge → host info line (e.g. `ads.off audio,inStream`).
    #[serde(rename = "log")]
    Log {
        line: String,
    },
    #[serde(other)]
    Other,
}

fn default_true() -> bool {
    true
}

/// Web Player transport permissions (normalized by the bridge from `can*` or `disallow_*`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerRestrictions {
    #[serde(default = "default_true")]
    pub can_skip_next: bool,
    #[serde(default = "default_true")]
    pub can_skip_previous: bool,
    #[serde(default = "default_true")]
    pub can_seek: bool,
    #[serde(default = "default_true")]
    pub can_pause: bool,
    #[serde(default = "default_true")]
    pub can_resume: bool,
    /// Raw next-skip disallow reasons (e.g. `ad_disallow`, skip-limit strings).
    #[serde(default)]
    pub disallow_skipping_next_reasons: Vec<String>,
    #[serde(default)]
    pub disallow_seeking_reasons: Vec<String>,
}

impl Default for PlayerRestrictions {
    fn default() -> Self {
        Self {
            can_skip_next: true,
            can_skip_previous: true,
            can_seek: true,
            can_pause: true,
            can_resume: true,
            disallow_skipping_next_reasons: Vec::new(),
            disallow_seeking_reasons: Vec::new(),
        }
    }
}

impl PlayerRestrictions {
    /// Free-tier hourly skip budget exhausted (not an ad block).
    pub fn skips_exhausted(&self) -> bool {
        if self.can_skip_next {
            return false;
        }
        self.disallow_skipping_next_reasons.iter().any(|r| {
            let t = r.to_ascii_lowercase();
            t.contains("unlimited_skips")
                || t.contains("no_skips_remaining")
                || t.contains("no_further_skip")
                || (t.contains("skip")
                    && (t.contains("remain")
                        || t.contains("limit")
                        || t.contains("exhausted")))
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    #[serde(default)]
    pub track: Option<TrackRef>,
    /// Next non-ad item from the page state (bridge filters ads out of previous/next).
    #[serde(default)]
    pub next: Option<TrackRef>,
    #[serde(default)]
    pub previous: Option<TrackRef>,
    #[serde(default)]
    pub is_paused: bool,
    #[serde(default)]
    pub is_buffering: bool,
    #[serde(default)]
    pub position_ms: i64,
    /// Wall-clock ms when `position_ms` was sampled (Web Player `timestamp`).
    #[serde(default)]
    pub timestamp: Option<i64>,
    #[serde(default)]
    pub duration_ms: i64,
    #[serde(default)]
    pub context_uri: Option<String>,
    #[serde(default)]
    pub context_name: Option<String>,
    #[serde(default)]
    pub shuffle: bool,
    /// 0 = off, 1 = context, 2 = track
    #[serde(default)]
    pub repeat: i64,
    #[serde(default)]
    pub restrictions: PlayerRestrictions,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackRef {
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// Web Player: "track" | "episode" | "ad" | …
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub advertiser: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub artists: Option<Vec<ArtistRef>>,
    #[serde(default)]
    pub album: Option<AlbumRef>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
}

impl TrackRef {
    pub fn is_ad(&self) -> bool {
        if self.kind.as_deref() == Some("ad") {
            return true;
        }
        if self
            .uri
            .as_deref()
            .is_some_and(|u| u.starts_with("spotify:ad:"))
        {
            return true;
        }
        if self.provider.as_deref() == Some("ad") {
            return true;
        }
        if self
            .advertiser
            .as_deref()
            .is_some_and(|a| !a.trim().is_empty())
        {
            return true;
        }
        self.name.as_deref().is_some_and(|n| {
            let t = n.trim();
            t.eq_ignore_ascii_case("advertisement") || t.eq_ignore_ascii_case("ad")
        })
    }

    pub fn is_playable_music(&self) -> bool {
        !self.is_ad()
            && self.uri.as_deref().is_some_and(|u| {
                u.starts_with("spotify:track:")
                    || u.starts_with("spotify:episode:")
                    || u.starts_with("spotify:local:")
            })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRef {
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumRef {
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Queue {
    #[serde(default)]
    pub current: Option<TrackRef>,
    #[serde(default, deserialize_with = "vec_skip_null")]
    pub queued: Vec<TrackRef>,
    #[serde(default, deserialize_with = "vec_skip_null")]
    pub next_up: Vec<TrackRef>,
}

fn vec_skip_null<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let raw: Option<Vec<Option<T>>> = Option::deserialize(deserializer)?;
    Ok(raw
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .collect())
}

impl PlayerState {
    pub fn title(&self) -> String {
        self.track
            .as_ref()
            .and_then(|t| t.name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Nothing playing".into())
    }

    pub fn artist_line(&self) -> String {
        self.track
            .as_ref()
            .and_then(|t| t.artists.as_ref())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "—".into())
    }

    /// Advance playhead from sample timestamp when not paused.
    pub fn live_position_ms(&self, now_unix_ms: i64) -> i64 {
        let base = self.position_ms.max(0);
        let dur = self.duration_ms.max(1);
        if self.is_paused {
            return base.min(dur);
        }
        let advanced = match self.timestamp {
            Some(ts) if ts > 0 => base + (now_unix_ms - ts).max(0),
            _ => base,
        };
        advanced.clamp(0, dur)
    }

    /// Equality for UI redraws — playhead is interpolated in the chrome; never push on ticks.
    pub fn ui_eq(&self, other: &Self) -> bool {
        self.track == other.track
            && self.is_paused == other.is_paused
            && self.is_buffering == other.is_buffering
            && self.duration_ms == other.duration_ms
            && self.context_uri == other.context_uri
            && self.context_name == other.context_name
            && self.shuffle == other.shuffle
            && self.repeat == other.repeat
            && self.restrictions == other.restrictions
    }
}
