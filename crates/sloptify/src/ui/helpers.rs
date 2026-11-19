//! Pure helpers.

use dioxus::prelude::*;
use sloptify_spotify::{is_browsable_uri, BridgeSnapshot, ContextTrack, HomeItem, TrackRef};

use super::types::{MenuAction, PlayerSlice};
use crate::session::{SessionHandle, UiCommand};

pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub paused: bool,
    pub pos: i64,
    pub dur: i64,
    pub image: Option<String>,
    pub context: String,
    pub context_uri: Option<String>,
    pub album_uri: Option<String>,
    pub album_name: Option<String>,
    pub artist_uri: Option<String>,
    pub is_ad: bool,
    pub can_skip_next: bool,
    pub can_skip_previous: bool,
    pub can_seek: bool,
    /// Short timeline hint when skip/seek is blocked ("Ad", "Skip limit", …).
    pub skip_hint: Option<String>,
}

pub fn send(session: &SessionHandle, cmd: UiCommand) {
    let _ = session.cmd_tx.send(cmd);
}

/// Flip play/pause in the chrome immediately, then ask the bridge.
/// Without this the icon waits on CDP round-trip + state push.
pub fn toggle_transport(mut player: Signal<PlayerSlice>, session: &SessionHandle) {
    let now = now_unix_ms();
    let mut resume = false;
    player.with_mut(|p| {
        if let Some(st) = p.snapshot.state.as_mut() {
            if st.is_paused {
                st.is_paused = false;
                st.timestamp = Some(now);
                resume = true;
            } else {
                // Freeze the interpolated playhead so resume continues cleanly.
                st.position_ms = st.live_position_ms(now);
                st.timestamp = Some(now);
                st.is_paused = true;
            }
        } else {
            resume = true;
        }
    });
    if resume {
        send(session, UiCommand::Resume);
    } else {
        send(session, UiCommand::Pause);
    }
}

pub fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn fmt_ms(ms: i64) -> String {
    let s = (ms / 1000).max(0);
    format!("{}:{:02}", s / 60, s % 60)
}

pub fn trunc(s: &str, n: usize) -> String {
    let mut t: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        t.push('…');
    }
    t
}

fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// FigCraft cassette SVG shell (title/artist filled via textContent — don't remount).
pub fn cassette_svg_shell() -> &'static str {
    const SVG: &str = include_str!("../../assets/sloptify-cassette.svg");
    SVG
}

pub fn escape_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c => out.push(c),
        }
    }
    out
}

/// FigCraft cassette SVG with live title / artist on the J-card.
#[allow(dead_code)]
pub fn cassette_svg(title: &str, artist: &str) -> String {
    cassette_svg_shell()
        .replace("__TITLE__", &escape_xml(&trunc(title, 26)))
        .replace("__ARTIST__", &escape_xml(&trunc(artist, 34)))
}

pub fn display_name(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() || t.starts_with("spotify:") {
        "Untitled".into()
    } else {
        t.into()
    }
}

pub fn pretty_description(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() || t.starts_with("spotify:") {
        String::new()
    } else {
        t.into()
    }
}

pub fn humanize_shelf_title(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() || t.starts_with("spotify:") {
        if t.contains("recents") {
            return "Spun lately".into();
        }
        return "Shelves".into();
    }
    t.to_string()
}

/// Short editorial greeting for the listening-room home.
pub fn editorial_greet(spotify: &str) -> &'static str {
    let s = spotify.to_ascii_lowercase();
    if s.contains("morning") {
        "Morning."
    } else if s.contains("afternoon") {
        "Afternoon."
    } else if s.contains("evening") || s.contains("night") {
        "Night."
    } else {
        "Listen."
    }
}

pub fn artists_of(t: &TrackRef) -> String {
    t.artists
        .as_ref()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.name.clone())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "—".into())
}

pub fn now_playing(snapshot: &BridgeSnapshot) -> NowPlaying {
    if let Some(s) = &snapshot.state {
        let track = s.track.as_ref();
        let is_ad = track.is_some_and(|t| t.is_ad());
        let r = &s.restrictions;
        let skip_hint = if is_ad {
            Some(
                track
                    .and_then(|t| t.advertiser.clone())
                    .filter(|a| !a.trim().is_empty())
                    .map(|a| format!("Ad · {a}"))
                    .unwrap_or_else(|| "Ad".into()),
            )
        } else if r.skips_exhausted() {
            Some("Skip limit".into())
        } else if !r.can_skip_next {
            Some("Can't skip".into())
        } else {
            None
        };
        let title = if is_ad {
            track
                .and_then(|t| t.name.clone())
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| "Advertisement".into())
        } else {
            s.title()
        };
        let artist = if is_ad {
            track
                .and_then(|t| t.advertiser.clone())
                .filter(|a| !a.trim().is_empty())
                .unwrap_or_else(|| "Ad".into())
        } else {
            s.artist_line()
        };
        NowPlaying {
            title,
            artist,
            paused: s.is_paused,
            pos: s.position_ms,
            dur: s.duration_ms.max(1),
            image: track.and_then(|t| t.image.clone()),
            context: s
                .context_name
                .clone()
                .unwrap_or_else(|| "Now playing".into()),
            context_uri: s.context_uri.clone().filter(|u| {
                !u.is_empty() && !u.contains(":ad:") && is_browsable_uri(u)
            }),
            album_uri: track.and_then(|t| t.album.as_ref()).and_then(|a| a.uri.clone()),
            album_name: track.and_then(|t| t.album.as_ref()).and_then(|a| a.name.clone()),
            artist_uri: track
                .and_then(|t| t.artists.as_ref())
                .and_then(|a| a.first())
                .and_then(|a| a.uri.clone()),
            is_ad,
            can_skip_next: r.can_skip_next,
            can_skip_previous: r.can_skip_previous,
            can_seek: r.can_seek,
            skip_hint,
        }
    } else {
        NowPlaying {
            title: "Nothing playing".into(),
            artist: "Pick something from Home".into(),
            paused: true,
            pos: 0,
            dur: 1,
            image: None,
            context: "Now playing".into(),
            context_uri: None,
            album_uri: None,
            album_name: None,
            artist_uri: None,
            is_ad: false,
            can_skip_next: true,
            can_skip_previous: true,
            can_seek: true,
            skip_hint: None,
        }
    }
}

pub fn item_menu_actions(item: &HomeItem) -> Vec<(String, MenuAction)> {
    let uri = item.uri.clone();
    let mut actions = vec![
        ("Play".into(), MenuAction::Play { uri: uri.clone() }),
        ("Play next".into(), MenuAction::PlayNext { uri: uri.clone() }),
        ("Add to queue".into(), MenuAction::QueueAdd { uri: uri.clone() }),
    ];
    if is_browsable_uri(&uri) {
        actions.push((
            "Open".into(),
            MenuAction::Open {
                uri: uri.clone(),
                title: Some(item.name.clone()).filter(|s| !s.is_empty()),
                image: item.image.clone(),
            },
        ));
    }
    if uri.starts_with("spotify:track:") {
        actions.push(("".into(), MenuAction::Sep));
        actions.push((
            "Add / remove Liked Songs".into(),
            MenuAction::ToggleLike { uri: uri.clone() },
        ));
        actions.push((
            "Go to song radio".into(),
            MenuAction::SongRadio {
                uri: uri.clone(),
                title: Some(item.name.clone()).filter(|s| !s.is_empty()),
            },
        ));
    }
    if uri.starts_with("spotify:artist:") {
        actions.push(("".into(), MenuAction::Sep));
        actions.push((
            "Go to artist radio".into(),
            MenuAction::SongRadio {
                uri: uri.clone(),
                title: Some(item.name.clone()).filter(|s| !s.is_empty()),
            },
        ));
    }
    actions.push(("".into(), MenuAction::Sep));
    actions.push(("Copy link".into(), MenuAction::CopyLink { uri }));
    actions
}

pub fn track_menu_actions(track: &ContextTrack) -> Vec<(String, MenuAction)> {
    let mut actions = vec![
        ("Play".into(), MenuAction::Play { uri: track.uri.clone() }),
        ("Play next".into(), MenuAction::PlayNext { uri: track.uri.clone() }),
        ("Add to queue".into(), MenuAction::QueueAdd { uri: track.uri.clone() }),
        ("".into(), MenuAction::Sep),
        (
            "Add / remove Liked Songs".into(),
            MenuAction::ToggleLike { uri: track.uri.clone() },
        ),
        (
            "Go to song radio".into(),
            MenuAction::SongRadio {
                uri: track.uri.clone(),
                title: Some(track.name.clone()),
            },
        ),
    ];
    if let Some(album_uri) = track.album_uri.clone() {
        actions.push((
            "Go to album".into(),
            MenuAction::Open {
                uri: album_uri,
                title: Some(track.album.clone()).filter(|s| !s.is_empty()),
                image: track.image.clone(),
            },
        ));
    }
    if let Some(artist_uri) = track.artist_uri.clone() {
        let artist_name = track
            .artists
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        actions.push((
            "Go to artist".into(),
            MenuAction::Open {
                uri: artist_uri,
                title: Some(artist_name).filter(|s| !s.is_empty()),
                image: None,
            },
        ));
    }
    actions.push(("".into(), MenuAction::Sep));
    actions.push((
        "Copy link".into(),
        MenuAction::CopyLink {
            uri: track.uri.clone(),
        },
    ));
    actions
}
