//! Orpheon `AdBlock.withoutAds` / `adAhead` / `AdSchedule` — ported from
//! `BridgeHost.kt` (kota-zo/Orpheon @ e66570b3), measured on free-account desktop 2026-10-20.

use serde_json::{json, Map, Value};
use std::sync::RwLock;

static AHEAD: RwLock<Option<Ahead>> = RwLock::new(None);

/// Orpheon `AdBlock.AdAhead` — only when end **and** skip both lead to an ad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ahead {
    pub song_uid: Option<String>,
    pub song_uri: Option<String>,
    pub next_uri: String,
}

pub fn ahead() -> Option<Ahead> {
    AHEAD.read().ok().and_then(|g| g.clone())
}

/// Back-compat alias used by the player (`due().next_uri`).
pub fn due() -> Option<Due> {
    ahead().map(|a| Due {
        song_uid: a.song_uid,
        song_uri: a.song_uri,
        next_uri: Some(a.next_uri),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    pub song_uid: Option<String>,
    pub song_uri: Option<String>,
    pub next_uri: Option<String>,
}

pub fn clear() {
    if let Ok(mut g) = AHEAD.write() {
        *g = None;
    }
}

/// Store [ad_ahead] from the body **as sent to the page** (after [without_ads]).
/// Never clear on a reply where the ad is already current — that used to wipe the
/// post-break URI right when we needed it for escape.
pub fn record(body: &str) {
    let next = ad_ahead(body);
    if let Ok(mut g) = AHEAD.write() {
        match next {
            Some(a) => {
                tracing::info!(
                    song = ?a.song_uri,
                    next = %a.next_uri,
                    "AdBlock.adAhead armed (skip also→ad)"
                );
                *g = Some(a);
            }
            None => {
                if !has_ad_hint(body) {
                    *g = None;
                }
                // else keep previous Ahead (current-is-ad replies, or first-shape only)
            }
        }
    }
}

fn has_ad_hint(body: &str) -> bool {
    body.contains("\"AD\"")
        || body.contains("spotify:ad:")
        || body.contains("\"is_advertisement\"")
        || body.contains("\"ad_id\"")
}

fn is_ad_track(track: Option<&Value>) -> bool {
    let Some(track) = track else {
        return false;
    };
    if track.get("content_type").and_then(|v| v.as_str()) == Some("AD") {
        return true;
    }
    let Some(md) = track.get("metadata") else {
        return false;
    };
    let flag = md
        .get("is_advertisement")
        .and_then(|v| v.as_str())
        .or_else(|| {
            md.get("is_advertisement")
                .and_then(|v| v.as_bool())
                .map(|b| if b { "true" } else { "false" })
        });
    if flag == Some("true") {
        return true;
    }
    if md
        .get("uri")
        .and_then(|v| v.as_str())
        .is_some_and(|u| u.starts_with("spotify:ad:"))
    {
        return true;
    }
    md.get("ad_id")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty())
}

fn state_i(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn track_of<'a>(tracks: &'a [Value], states: &[Value], i: i64) -> Option<&'a Value> {
    let state = states.get(i as usize)?;
    let ti = state_i(state.get("track")?)? as usize;
    tracks.get(ti)
}

fn way(states: &[Value], i: i64, key: &str) -> Option<i64> {
    states
        .get(i as usize)?
        .pointer(&format!("/transitions/{key}/state_index"))
        .and_then(state_i)
}

fn uri_of(tracks: &[Value], states: &[Value], i: i64) -> Option<String> {
    track_of(tracks, states, i)
        .and_then(|t| t.pointer("/metadata/uri"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn name_of(tracks: &[Value], states: &[Value], i: i64) -> Option<String> {
    track_of(tracks, states, i)
        .and_then(|t| t.pointer("/metadata/name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn repeats_track(states: &[Value], i: i64) -> bool {
    states
        .get(i as usize)
        .and_then(|s| s.pointer("/options/repeating_track"))
        .and_then(|v| v.as_str().or_else(|| v.as_bool().map(|b| if b { "true" } else { "false" })))
        == Some("true")
}

/// Playable repeat-one state of the same song (has `show_next`; display-only ones do not).
fn repeat_of(tracks: &[Value], states: &[Value], i: i64) -> Option<i64> {
    let uri = uri_of(tracks, states, i)?;
    (0..states.len() as i64).find(|&j| {
        j != i
            && !is_ad_track(track_of(tracks, states, j))
            && repeats_track(states, j)
            && uri_of(tracks, states, j).as_deref() == Some(uri.as_str())
            && states
                .get(j as usize)
                .and_then(|s| s.pointer("/transitions/show_next"))
                .is_some_and(|v| v.is_object())
    })
}

/// Orpheon `AdBlock.adAhead` — only when advance **and** skip_next both lead to an ad;
/// `next_uri` comes from `show_next` (URI only — never rewrite advance to that state).
pub fn ad_ahead(body: &str) -> Option<Ahead> {
    if !has_ad_hint(body) {
        return None;
    }
    let root: Value = serde_json::from_str(body).ok()?;
    let machine = root.get("state_machine")?;
    let tracks = machine.get("tracks")?.as_array()?;
    let states = machine.get("states")?.as_array()?;
    let current = root
        .pointer("/updated_state_ref/state_index")
        .and_then(state_i)?;
    if is_ad_track(track_of(tracks, states, current)) {
        return None;
    }
    let end = way(states, current, "advance")?;
    if !is_ad_track(track_of(tracks, states, end)) {
        return None;
    }
    // Skip still reaches music → withoutAds already handles it; no early play.
    if way(states, current, "skip_next")
        .is_some_and(|s| !is_ad_track(track_of(tracks, states, s)))
    {
        return None;
    }
    let show = way(states, current, "show_next")
        .filter(|&i| !is_ad_track(track_of(tracks, states, i)))?;
    let next_uri = uri_of(tracks, states, show)?;
    let song_uid = states
        .get(current as usize)
        .and_then(|s| s.get("track_uid"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Some(Ahead {
        song_uid,
        song_uri: uri_of(tracks, states, current),
        next_uri,
    })
}

/// Orpheon `AdSchedule.dueIn` (diagnostics / older callers).
pub fn due_in(body: &str) -> Option<Due> {
    ad_ahead(body).map(|a| Due {
        song_uid: a.song_uid,
        song_uri: a.song_uri,
        next_uri: Some(a.next_uri),
    }).or_else(|| {
        // First shape: advance→ad but skip→music — no Ahead, still useful for logs.
        if !has_ad_hint(body) {
            return None;
        }
        let root: Value = serde_json::from_str(body).ok()?;
        let machine = root.get("state_machine")?;
        let tracks = machine.get("tracks")?.as_array()?;
        let states = machine.get("states")?.as_array()?;
        let current = root
            .pointer("/updated_state_ref/state_index")
            .and_then(state_i)?;
        if is_ad_track(track_of(tracks, states, current)) {
            return None;
        }
        let end = way(states, current, "advance")?;
        if !is_ad_track(track_of(tracks, states, end)) {
            return None;
        }
        let next = way(states, current, "skip_next")
            .filter(|&i| !is_ad_track(track_of(tracks, states, i)))
            .or_else(|| {
                way(states, current, "show_next")
                    .filter(|&i| !is_ad_track(track_of(tracks, states, i)))
            })?;
        Some(Due {
            song_uid: None,
            song_uri: uri_of(tracks, states, current),
            next_uri: uri_of(tracks, states, next),
        })
    })
}

fn machine_without_ads(machine: &mut Map<String, Value>) -> u32 {
    let tracks = match machine.get("tracks").and_then(|t| t.as_array()) {
        Some(t) => t.clone(),
        None => return 0,
    };
    let Some(states_val) = machine.get_mut("states") else {
        return 0;
    };
    let Some(states) = states_val.as_array_mut() else {
        return 0;
    };
    let snapshot = states.clone();
    let mut changed = 0u32;

    for i in 0..states.len() {
        if is_ad_track(track_of(&tracks, &snapshot, i as i64)) {
            continue;
        }
        let Some(transitions) = states[i]
            .get("transitions")
            .and_then(|t| t.as_object())
            .cloned()
        else {
            continue;
        };
        // Only skip_next is a state the server knows (never show_next).
        let target = transitions
            .get("skip_next")
            .and_then(|t| t.as_object())
            .filter(|t| {
                t.get("state_index")
                    .and_then(state_i)
                    .is_some_and(|idx| !is_ad_track(track_of(&tracks, &snapshot, idx)))
            })
            .cloned();

        let mut next_map = transitions.clone();
        let mut state_changed = false;
        for key in ["advance", "skip_next"] {
            let Some(t) = transitions.get(key).and_then(|v| v.as_object()) else {
                continue;
            };
            let Some(idx) = t.get("state_index").and_then(state_i) else {
                continue;
            };
            if !is_ad_track(track_of(&tracks, &snapshot, idx)) {
                continue;
            }
            if key == "advance" && repeats_track(&snapshot, i as i64) {
                if let Some(again) = repeat_of(&tracks, &snapshot, i as i64) {
                    let mut rewritten = t.clone();
                    rewritten.insert("state_index".into(), json!(again));
                    next_map.insert(key.into(), Value::Object(rewritten));
                    state_changed = true;
                    tracing::info!(
                        key,
                        song = name_of(&tracks, &snapshot, i as i64).as_deref().unwrap_or("?"),
                        "AdBlock.withoutAds → repeat instead of ad"
                    );
                    continue;
                }
                // Repeat one with no repeat state: leave the ad (Orpheon).
                continue;
            }
            let Some(ref target) = target else {
                continue;
            };
            let mut rewritten = target.clone();
            if let Some(paused) = t.get("paused") {
                rewritten.insert("paused".into(), paused.clone());
            }
            next_map.insert(key.into(), Value::Object(rewritten));
            state_changed = true;
            changed += 1;
            tracing::info!(
                key,
                song = name_of(&tracks, &snapshot, i as i64).as_deref().unwrap_or("?"),
                instead = name_of(
                    &tracks,
                    &snapshot,
                    target.get("state_index").and_then(state_i).unwrap_or(-1)
                )
                .as_deref()
                .unwrap_or("?"),
                "AdBlock.withoutAds"
            );
        }
        if state_changed {
            if let Some(obj) = states[i].as_object_mut() {
                obj.insert("transitions".into(), Value::Object(next_map));
            }
        }
    }
    changed
}

/// Clamp ad track durations so a landed break ends almost immediately (with silent MP3).
fn clamp_ad_durations(machine: &mut Map<String, Value>) -> u32 {
    let Some(Value::Array(tracks)) = machine.get_mut("tracks") else {
        return 0;
    };
    let mut n = 0u32;
    for track in tracks.iter_mut() {
        if !is_ad_track(Some(track)) {
            continue;
        }
        let Some(obj) = track.as_object_mut() else {
            continue;
        };
        for k in ["duration_ms", "duration"] {
            obj.insert(k.into(), json!(80));
        }
        let md = obj
            .entry("metadata")
            .or_insert_with(|| json!({}))
            .as_object_mut();
        if let Some(md) = md {
            for k in ["duration_ms", "duration", "length", "length_ms"] {
                md.insert(k.into(), json!(80));
            }
        }
        n += 1;
    }
    n
}

/// Orpheon `AdBlock.withoutAds` — rewrite `state_machine` and `previous_state_machine`,
/// and shrink any ad track's duration so a silent fulfill doesn't wait on a 30 s timer.
pub fn without_ads(body: &str) -> Option<String> {
    if !has_ad_hint(body) {
        return None;
    }
    let mut root: Value = serde_json::from_str(body).ok()?;
    let Some(obj) = root.as_object_mut() else {
        return None;
    };
    let mut changed = 0u32;
    let mut clamped = 0u32;
    for key in ["state_machine", "previous_state_machine"] {
        if let Some(Value::Object(machine)) = obj.get_mut(key) {
            changed += machine_without_ads(machine);
            clamped += clamp_ad_durations(machine);
        }
    }
    if changed == 0 && clamped == 0 {
        return None;
    }
    if changed > 0 {
        tracing::info!(changed, clamped, "AdBlock.withoutAds rewrote transitions");
    } else if clamped > 0 {
        tracing::info!(clamped, "AdBlock.withoutAds clamped ad duration");
    }
    serde_json::to_string(&root).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(ty: &str, name: &str) -> String {
        let uri_ty = if ty == "AD" { "ad" } else { "track" };
        format!(
            r#"{{"content_type":"{ty}","metadata":{{"uri":"spotify:{uri_ty}:{}","name":"{name}"}},"manifest":{{}}}}"#,
            name.replace(' ', "-")
        )
    }

    fn go(to: Option<i64>) -> String {
        match to {
            None => "null".into(),
            Some(i) => format!(r#"{{"state_index":{i},"paused":false}}"#),
        }
    }

    fn state(track: i64, advance: Option<i64>, next: Option<i64>, show: Option<i64>) -> String {
        format!(
            r#"{{"track":{track},"transitions":{{"skip_next":{},"show_next":{},"advance":{}}}}}"#,
            go(next),
            go(show),
            go(advance)
        )
    }

    fn reply(tracks: &[&str], states: &[String], current: i64) -> String {
        format!(
            r#"{{"state_machine":{{"tracks":[{}],"states":[{}],"attributes":{{}}}},"updated_state_ref":{{"state_index":{current}}}}}"#,
            tracks.join(","),
            states.join(",")
        )
    }

    #[test]
    fn first_shape_advance_goes_to_song_after() {
        // Orpheon AdQueueTest first shape.
        let body = reply(
            &[
                &track("AD", "An ad"),
                &track("TRACK", "Before"),
                &track("TRACK", "Playing"),
                &track("TRACK", "After"),
            ],
            &[
                state(0, None, None, None),
                state(1, None, None, None),
                state(2, Some(0), Some(4), Some(4)),
                state(2, None, None, None),
                state(3, None, None, None),
            ],
            2,
        );
        let out = without_ads(&body).expect("rewrite");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v.pointer("/state_machine/states/2/transitions/advance/state_index")
                .and_then(state_i),
            Some(4)
        );
        assert!(ad_ahead(&out).is_none());
        assert!(ad_ahead(&body).is_none()); // skip still music → no Ahead
    }

    #[test]
    fn second_shape_keeps_transitions_but_clamps_ad_and_adahead() {
        let body = reply(
            &[
                &track("AD", "An ad"),
                &track("TRACK", "After"),
                &track("TRACK", "Before"),
                &track("TRACK", "Playing"),
            ],
            &[
                state(0, None, None, None),
                state(0, None, None, None),
                state(3, Some(0), Some(1), Some(4)),
                state(0, None, None, None),
                state(1, None, None, None),
                state(2, None, None, None),
            ],
            2,
        );
        // Transitions stay (skip also→ad); duration is clamped so a silent fulfill ends fast.
        let out = without_ads(&body).expect("clamp");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v.pointer("/state_machine/states/2/transitions/advance/state_index")
                .and_then(state_i),
            Some(0),
            "advance still the ad"
        );
        assert_eq!(
            v.pointer("/state_machine/tracks/0/metadata/duration_ms")
                .and_then(state_i),
            Some(80)
        );
        let a = ad_ahead(&body).expect("ahead");
        assert_eq!(a.song_uri.as_deref(), Some("spotify:track:Playing"));
        assert_eq!(a.next_uri, "spotify:track:After");
    }

    #[test]
    fn record_keeps_ahead_when_current_becomes_ad() {
        let armed = reply(
            &[
                &track("AD", "An ad"),
                &track("TRACK", "After"),
                &track("TRACK", "Before"),
                &track("TRACK", "Playing"),
            ],
            &[
                state(0, None, None, None),
                state(0, None, None, None),
                state(3, Some(0), Some(1), Some(4)),
                state(0, None, None, None),
                state(1, None, None, None),
                state(2, None, None, None),
            ],
            2,
        );
        clear();
        record(&armed);
        assert_eq!(
            ahead().unwrap().next_uri,
            "spotify:track:After"
        );
        let on_ad = reply(
            &[&track("AD", "An ad"), &track("TRACK", "After")],
            &[state(0, None, None, None), state(1, None, None, None)],
            0,
        );
        record(&on_ad);
        assert_eq!(
            ahead().map(|a| a.next_uri),
            Some("spotify:track:After".into()),
            "must not wipe ahead while sitting on the ad"
        );
        clear();
    }

    #[test]
    fn ad_already_current_clamps_duration_only() {
        let body = reply(
            &[&track("AD", "An ad"), &track("TRACK", "B")],
            &[state(0, None, None, None), state(1, None, None, None)],
            0,
        );
        let out = without_ads(&body).expect("clamp");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            v.pointer("/updated_state_ref/state_index")
                .and_then(state_i),
            Some(0),
            "current ref stays on the ad"
        );
        assert_eq!(
            v.pointer("/state_machine/tracks/0/metadata/duration_ms")
                .and_then(state_i),
            Some(80)
        );
        assert!(ad_ahead(&body).is_none());
    }

    #[test]
    fn previous_state_machine_also_rewritten() {
        let inner = reply(
            &[
                &track("AD", "An ad"),
                &track("TRACK", "Playing"),
                &track("TRACK", "After"),
            ],
            &[
                state(0, None, None, None),
                state(1, Some(0), Some(2), Some(2)),
                state(2, None, None, None),
            ],
            1,
        );
        let v: Value = serde_json::from_str(&inner).unwrap();
        let machine = v.get("state_machine").unwrap().clone();
        let body = json!({
            "updated_state_ref": {"state_index": 1},
            "state_machine": machine,
            "previous_state_machine": v.get("state_machine").unwrap().clone(),
        })
        .to_string();
        let out = without_ads(&body).expect("rewrite");
        let o: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            o.pointer("/previous_state_machine/states/1/transitions/advance/state_index")
                .and_then(state_i),
            Some(2)
        );
    }
}
