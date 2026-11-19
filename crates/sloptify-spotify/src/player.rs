use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::ad_block;
use crate::ad_schedule;
use crate::bridge::{Bridge, BridgeError, BridgeSnapshot};

/// Sloptify `watchStalls` thresholds (this-device Web Player).
const STUCK_MS: u64 = 8_000;
const END_STUCK_MS: u64 = 1_000;
const WATCH_COOLDOWN: Duration = Duration::from_secs(12);

/// Skip as soon as an ad is seen (audio is silenced first so nothing is heard).
const AD_COOLDOWN: Duration = Duration::from_secs(14);
/// After a failed reload escape, wait this long (quiet) before another reload.
/// Free-tier ads refuse skipToNext — reloading every few seconds just re-queues the break.
const AD_RETRY: Duration = Duration::from_secs(12);
/// Jump to the post-break song this far before the current one ends (Orpheon used ~1 s;
/// we start earlier so a slow poll still catches second-shape breaks).
const AD_PREEMPT_MS: i64 = 2_500;

/// High-level player controls over the page bridge (Sloptify command names).
pub struct Player {
    bridge: Arc<Bridge>,
}

/// Tracks playhead freezes and runs Sloptify-style repair (`reloadAndPlayOn`).
#[derive(Debug, Default)]
pub struct StallWatch {
    last_uri: Option<String>,
    last_pos: i64,
    stuck_since: Option<Instant>,
    attempts: u8,
    last_repair: Option<Instant>,
}

/// When the Web Player lands on an audio ad, silence immediately and escape
/// (Orpheon ADS.md: reload is last resort; play the listening context, not track-as-context).
#[derive(Debug, Default)]
pub struct AdWatch {
    last_skip: Option<Instant>,
    quiet: bool,
    /// How many reload escapes failed in a row (backs off; reset on clear music).
    failed_escapes: u8,
    /// Last playlist/album/artist/collection context while music played (survives ad state).
    last_context: Option<String>,
    /// Post-break track URI remembered while `adAhead` was armed (survives current=ad replies).
    last_next: Option<String>,
}

impl Player {
    pub fn new(bridge: Arc<Bridge>) -> Self {
        Self { bridge }
    }

    pub fn bridge(&self) -> Arc<Bridge> {
        self.bridge.clone()
    }

    pub async fn snapshot(&self) -> BridgeSnapshot {
        self.bridge.snapshot().await
    }

    pub async fn ensure_ready(&self) -> Result<(), BridgeError> {
        self.bridge.inject().await?;
        self.bridge.wait_ready(Duration::from_secs(45)).await?;
        self.bridge.ensure_player_root().await
    }

    pub async fn pause(&self) -> Result<(), BridgeError> {
        self.bridge
            .call_timeout("player.pause", json!({}), Duration::from_secs(8))
            .await
            .map(|_| ())
    }

    pub async fn resume(&self) -> Result<(), BridgeError> {
        // Sloptify `resumePage`: never sit on the 20s bridge timeout — a stuck
        // `player.resume` (ads, frozen renderer) is recovered by a secret page reload.
        self.resume_sloptify().await
    }

    /// Mirrors Sloptify `SpotifyPlayer.resumePage` / `reloadAndPlayOn`.
    async fn resume_sloptify(&self) -> Result<(), BridgeError> {
        let snap = self.bridge.snapshot().await;
        let state = snap.state.as_ref();
        let track = state.and_then(|s| s.track.as_ref());
        let is_ad = track.map(|t| t.is_ad()).unwrap_or(false);
        let context = state.and_then(|s| s.context_uri.clone());
        let track_uri = track
            .filter(|t| t.is_playable_music())
            .and_then(|t| t.uri.clone());

        // Dead page while paused → reload instead of waiting out resume (Sloptify comment:
        // "instead of the resume waiting out its 20 s timeout").
        if !self.bridge.ping(Duration::from_secs(3)).await {
            tracing::info!("resume: page not answering — reloading Web Player (Sloptify path)");
            self.bridge.reload_page().await?;
            return self.play_after_reload(context.as_deref(), track_uri.as_deref()).await;
        }

        // Ads: silence first (no audible fragment), then reload into the next real song.
        if is_ad {
            tracing::info!("resume: on an ad — quiet + reloadAndPlayOn(next)");
            let _ = self.set_quiet(true).await;
            let result = self.reload_and_play_on(true).await;
            let _ = self.set_quiet(false).await;
            return result;
        }

        match self
            .bridge
            .call_timeout("player.resume", json!({}), Duration::from_secs(8))
            .await
        {
            Ok(_) => Ok(()),
            Err(BridgeError::Timeout) => {
                tracing::info!("resume: timed out — reloading Web Player (Sloptify path)");
                self.bridge.reload_page().await?;
                self.play_after_reload(context.as_deref(), track_uri.as_deref())
                    .await
            }
            Err(err) => Err(err),
        }
    }

    /// Sloptify `reloadAndPlayOn` — soft-reload the Web Player and resume context/track.
    ///
    /// `next = true`: play the song after the current one (stuck at end, or an ad break).
    /// Orpheon ADS.md Oct 20: after reload, play the **listening context** again — never
    /// `spotify:track:…` as contextUri (that re-queues a fresh preroll on the new device).
    pub async fn reload_and_play_on(&self, next: bool) -> Result<(), BridgeError> {
        self.reload_and_play_on_with(None, next).await
    }

    async fn reload_and_play_on_with(
        &self,
        preferred_context: Option<&str>,
        next: bool,
    ) -> Result<(), BridgeError> {
        let snap = self.bridge.snapshot().await;
        let state = snap.state.as_ref();
        let snap_context = state.and_then(|s| s.context_uri.clone());
        let context = listening_context(preferred_context.or(snap_context.as_deref()));
        let position = state.map(|s| s.position_ms).unwrap_or(0);
        let onward = if next {
            ad_schedule::due()
                .and_then(|d| d.next_uri)
                .or_else(|| next_music_uri(&snap))
        } else {
            state
                .and_then(|s| s.track.as_ref())
                .filter(|t| t.is_playable_music())
                .and_then(|t| t.uri.clone())
        };

        tracing::info!(next, ?onward, ?context, "reloadAndPlayOn (context, not track-as-ctx)");
        self.bridge.reload_page().await?;

        if next {
            // ADS.md Oct 20: "playing the context again starts a song within a second".
            // Prefer context-only; skipTo trackUri can re-queue a preroll on the new device.
            let ctx = context
                .as_deref()
                .unwrap_or("spotify:collection:tracks");
            // Context only — a second play with trackUri re-queues a preroll on the new device.
            tracing::info!(%ctx, ?onward, "ad: play listening context after reload");
            self.bridge
                .call_timeout(
                    "player.play",
                    json!({ "contextUri": ctx }),
                    Duration::from_secs(12),
                )
                .await?;
            return self.escape_ads().await;
        }

        self.play_after_reload(context.as_deref(), onward.as_deref())
            .await?;
        if position > 1_500 {
            let _ = self.seek(position).await;
        }
        Ok(())
    }

    /// Silence the media element without Chromium treating it as "muted autoplay"
    /// (Sloptify `media.quiet` / SILENT = 0.001).
    pub async fn set_quiet(&self, on: bool) -> Result<(), BridgeError> {
        self.bridge
            .call_timeout("media.quiet", json!({ "on": on }), Duration::from_secs(3))
            .await
            .map(|_| ())
    }

    /// Orpheon ADS.md path for local Chromium:
    /// 1. `withoutAds` — advance→skip_next only (never show_next; never rewrite current ref)
    /// 2. `adAhead` — play post-break track ~1 s early in the **same** session (no reload)
    /// 3. Refuse ad MP3 hosts
    /// 4. Already on an ad (Oct 20 stuck): quiet + **one** reload, then play listening context
    pub async fn watch_ads(&self, watch: &mut AdWatch) -> Result<(), BridgeError> {
        let snap = self.bridge.snapshot().await;
        let Some(state) = snap.state.as_ref() else {
            return self.ensure_loud(watch).await;
        };
        if state.is_paused {
            return Ok(());
        }

        let on_ad = state.track.as_ref().is_some_and(|t| t.is_ad());
        if !on_ad {
            if let Some(ctx) = listening_context(state.context_uri.as_deref()) {
                watch.last_context = Some(ctx);
            }
            if let Some(a) = ad_schedule::ahead() {
                watch.last_next = Some(a.next_uri);
            }
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let pos = state.live_position_ms(now_ms);
        let dur = state.duration_ms.max(1);
        let near_end = !on_ad && dur > 3_000 && pos + AD_PREEMPT_MS >= dur;

        // Orpheon adAhead: skip_next also→ad — play show_next's song before the end.
        if near_end {
            if let Some(ahead) = ad_schedule::ahead() {
                let cur = state.track.as_ref().and_then(|t| t.uri.as_deref());
                let matches = ahead.song_uri.as_deref() == cur || ahead.song_uri.is_none();
                if matches && !watch.last_skip.is_some_and(|t| t.elapsed() < AD_COOLDOWN) {
                    watch.last_skip = Some(Instant::now());
                    watch.last_next = Some(ahead.next_uri.clone());
                    let next = ahead.next_uri.clone();
                    let ctx = listening_context(
                        watch
                            .last_context
                            .as_deref()
                            .or(state.context_uri.as_deref()),
                    );
                    tracing::info!(%next, ?ctx, "ad: adAhead play before break (no reload)");
                    let _ = self.set_quiet(true).await;
                    watch.quiet = true;
                    let play = self.play_after_reload(ctx.as_deref(), Some(&next)).await;
                    if play.is_ok() {
                        if self.settle_off_ad(Duration::from_millis(900)).await {
                            watch.failed_escapes = 0;
                            ad_schedule::clear();
                            return self.ensure_loud(watch).await;
                        }
                    }
                }
            }
            let _ = self.refresh_queue().await;
            if queue_leads_with_ad(&self.bridge.snapshot().await) {
                // Fall through if somehow already on ad.
            } else if !on_ad {
                return self.ensure_loud(watch).await;
            }
        }

        if !on_ad {
            watch.failed_escapes = 0;
            return self.ensure_loud(watch).await;
        }

        let ctx = listening_context(
            watch
                .last_context
                .as_deref()
                .or(state.context_uri.as_deref()),
        );
        let next = watch
            .last_next
            .clone()
            .or_else(|| ad_schedule::ahead().map(|a| a.next_uri))
            .or_else(|| next_music_uri(&snap));

        // First tick on an ad: quiet + jump immediately (no multi-second stare).
        if !watch.quiet {
            let _ = self.set_quiet(true).await;
            watch.quiet = true;
            watch.last_skip = Some(Instant::now());
            tracing::info!(
                ad_block = ad_block::active(),
                ?next,
                "ad: quiet + instant escape"
            );
            if state.restrictions.can_skip_next {
                let _ = self.next().await;
            } else if let Some(uri) = next.as_deref() {
                let _ = self
                    .play_after_reload(ctx.as_deref(), Some(uri))
                    .await;
            }
            return Ok(());
        }

        // Clamped silent ad should clear in <1 s; poll briefly before reload.
        if watch
            .last_skip
            .is_some_and(|t| t.elapsed() < Duration::from_millis(900))
            && watch.failed_escapes == 0
        {
            if !self.currently_on_ad().await {
                watch.failed_escapes = 0;
                ad_schedule::clear();
                return self.ensure_loud(watch).await;
            }
            return Ok(());
        }

        let cooldown = if watch.failed_escapes == 0 {
            Duration::from_millis(400)
        } else {
            AD_RETRY
                .saturating_mul(watch.failed_escapes.saturating_add(1) as u32)
                .max(AD_RETRY)
        };
        if watch.last_skip.is_some_and(|t| t.elapsed() < cooldown) && watch.failed_escapes > 0 {
            return Ok(());
        }
        if watch.failed_escapes >= 3 {
            return Ok(());
        }

        watch.last_skip = Some(Instant::now());

        // One more in-session play before the heavy reload.
        if let Some(uri) = next.as_deref() {
            tracing::info!(%uri, ?ctx, "ad: in-session play past break");
            let _ = self.play_after_reload(ctx.as_deref(), Some(uri)).await;
            if self.settle_off_ad(Duration::from_millis(800)).await {
                watch.failed_escapes = 0;
                ad_schedule::clear();
                return self.ensure_loud(watch).await;
            }
        }

        tracing::info!(?next, ?ctx, "ad: reload + play listening context");
        let result = self.reload_and_play_on_with(ctx.as_deref(), true).await;
        if !self.currently_on_ad().await {
            watch.failed_escapes = 0;
            ad_schedule::clear();
            return self.ensure_loud(watch).await;
        }

        watch.quiet = true;
        let _ = self.set_quiet(true).await;
        watch.failed_escapes = watch.failed_escapes.saturating_add(1).min(5);
        tracing::warn!(failed = watch.failed_escapes, "ad: still current after escape");
        result
    }

    async fn ensure_loud(&self, watch: &mut AdWatch) -> Result<(), BridgeError> {
        if watch.quiet {
            if let Err(err) = self.set_quiet(false).await {
                tracing::warn!(?err, "media.quiet off");
            } else {
                watch.quiet = false;
            }
        }
        Ok(())
    }

    /// After reload+play: wait for music — do **not** re-play (that restarts the ad).
    async fn escape_ads(&self) -> Result<(), BridgeError> {
        let _ = self.settle_off_ad(Duration::from_millis(1_500)).await;
        Ok(())
    }

    async fn currently_on_ad(&self) -> bool {
        self.bridge
            .snapshot()
            .await
            .state
            .as_ref()
            .and_then(|s| s.track.as_ref())
            .is_some_and(|t| t.is_ad())
    }

    /// Poll until current item is not an ad. Keeps quiet; never calls play.
    async fn settle_off_ad(&self, budget: Duration) -> bool {
        let deadline = Instant::now() + budget;
        while Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(280)).await;
            if !self.currently_on_ad().await {
                tracing::info!("ad: settled — music playing");
                let _ = self.set_quiet(false).await;
                let _ = self
                    .bridge
                    .call_timeout("player.resume", json!({}), Duration::from_secs(5))
                    .await;
                return true;
            }
            let _ = self.set_quiet(true).await;
        }
        false
    }

    /// Sloptify stall watchdog tick. Call ~every 1–2s while Ready.
    pub async fn watch_stalls(&self, watch: &mut StallWatch) -> Result<(), BridgeError> {
        let snap = self.bridge.snapshot().await;
        let Some(state) = snap.state.as_ref() else {
            watch.reset();
            return Ok(());
        };
        if state.is_paused || state.is_buffering {
            watch.reset();
            return Ok(());
        }
        let track = state.track.as_ref();
        if track.map(|t| t.is_ad()).unwrap_or(false) {
            watch.reset();
            return Ok(());
        }
        let uri = track.and_then(|t| t.uri.clone());
        let pos = state.live_position_ms(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
        );
        let dur = state.duration_ms.max(1);
        let near_end = pos + 1_500 >= dur;

        if uri != watch.last_uri || (pos > watch.last_pos + 400) {
            watch.last_uri = uri;
            watch.last_pos = pos;
            watch.stuck_since = None;
            watch.attempts = 0;
            return Ok(());
        }

        // Position not advancing while "playing".
        let stuck_for = match watch.stuck_since {
            Some(t) => t.elapsed(),
            None => {
                watch.stuck_since = Some(Instant::now());
                watch.last_pos = pos;
                return Ok(());
            }
        };
        let threshold = if near_end {
            Duration::from_millis(END_STUCK_MS)
        } else {
            Duration::from_millis(STUCK_MS)
        };
        if stuck_for < threshold {
            return Ok(());
        }
        if watch
            .last_repair
            .is_some_and(|t| t.elapsed() < WATCH_COOLDOWN)
        {
            return Ok(());
        }

        watch.attempts = watch.attempts.saturating_add(1);
        watch.last_repair = Some(Instant::now());
        watch.stuck_since = None;

        if watch.attempts == 1 {
            tracing::info!(pos, dur, "stall: soft repair (seek/next)");
            if near_end {
                let _ = self.next().await;
            } else {
                let _ = self.seek(pos.saturating_add(500).min(dur.saturating_sub(250))).await;
            }
        } else {
            tracing::info!(attempt = watch.attempts, "stall: reloadAndPlayOn");
            self.reload_and_play_on(near_end).await?;
            watch.attempts = 0;
        }
        Ok(())
    }

    async fn play_after_reload(
        &self,
        context: Option<&str>,
        track_uri: Option<&str>,
    ) -> Result<(), BridgeError> {
        let ctx = context
            .filter(|c| !c.is_empty() && !c.contains(":ad:"))
            .unwrap_or("spotify:collection:tracks");
        let mut args = json!({ "contextUri": ctx });
        if let Some(t) = track_uri {
            args["trackUri"] = json!(t);
        }
        tracing::info!(%ctx, ?track_uri, "player.play after reload");
        self.bridge
            .call_timeout("player.play", args, Duration::from_secs(12))
            .await
            .map(|_| ())
    }

    pub async fn next(&self) -> Result<(), BridgeError> {
        self.bridge
            .call_timeout("player.next", json!({}), Duration::from_secs(8))
            .await
            .map(|_| ())
    }

    pub async fn previous(&self) -> Result<(), BridgeError> {
        self.bridge
            .call_timeout("player.previous", json!({}), Duration::from_secs(8))
            .await
            .map(|_| ())
    }

    pub async fn seek(&self, position_ms: i64) -> Result<(), BridgeError> {
        self.bridge
            .call_timeout(
                "player.seek",
                json!({ "positionMs": position_ms }),
                Duration::from_secs(8),
            )
            .await
            .map(|_| ())
    }

    pub async fn set_volume(&self, volume: f64) -> Result<(), BridgeError> {
        self.bridge
            .call_timeout(
                "volume.set",
                json!({ "level": volume }),
                Duration::from_secs(8),
            )
            .await
            .map(|_| ())
    }

    pub async fn get_volume(&self) -> Result<f64, BridgeError> {
        let v = self
            .bridge
            .call_timeout("volume.get", json!({}), Duration::from_secs(5))
            .await?;
        Ok(v.as_f64()
            .or_else(|| v.get("value").and_then(|x| x.as_f64()))
            .unwrap_or(0.8))
    }

    pub async fn set_shuffle(&self, on: bool) -> Result<(), BridgeError> {
        self.bridge
            .call_timeout("player.setShuffle", json!({ "on": on }), Duration::from_secs(8))
            .await
            .map(|_| ())
    }

    /// `mode`: 0 off, 1 context, 2 track
    pub async fn set_repeat(&self, mode: i64) -> Result<(), BridgeError> {
        self.bridge
            .call_timeout(
                "player.setRepeat",
                json!({ "mode": mode.clamp(0, 2) }),
                Duration::from_secs(8),
            )
            .await
            .map(|_| ())
    }

    pub async fn refresh_queue(&self) -> Result<(), BridgeError> {
        let value = self
            .bridge
            .call_timeout("player.queue", json!({}), Duration::from_secs(8))
            .await?;
        let queue = if value.is_null() {
            crate::messages::Queue::default()
        } else {
            serde_json::from_value::<crate::messages::Queue>(value).unwrap_or_default()
        };
        self.bridge.set_queue(queue).await;
        Ok(())
    }

    pub async fn queue_skip_to(&self, uri: &str, uid: Option<&str>) -> Result<(), BridgeError> {
        let mut args = json!({ "uri": uri });
        if let Some(uid) = uid.filter(|u| !u.is_empty()) {
            args["uid"] = json!(uid);
        }
        self.bridge
            .call_timeout("queue.skipTo", args, Duration::from_secs(8))
            .await
            .map(|_| ())
    }

    pub async fn library_contains(&self, uri: &str) -> Result<bool, BridgeError> {
        let v = self
            .bridge
            .call_timeout(
                "library.contains",
                json!({ "uris": [uri] }),
                Duration::from_secs(8),
            )
            .await?;
        if let Some(b) = v.as_bool() {
            return Ok(b);
        }
        if let Some(arr) = v.as_array() {
            return Ok(arr.first().and_then(|x| x.as_bool()).unwrap_or(false));
        }
        if let Some(arr) = v.get(uri).and_then(|x| x.as_bool()) {
            return Ok(arr);
        }
        Ok(false)
    }

    pub async fn library_add(&self, uri: &str) -> Result<(), BridgeError> {
        self.bridge
            .call_timeout(
                "library.add",
                json!({ "uris": [uri] }),
                Duration::from_secs(8),
            )
            .await
            .map(|_| ())
    }

    pub async fn library_remove(&self, uri: &str) -> Result<(), BridgeError> {
        self.bridge
            .call_timeout(
                "library.remove",
                json!({ "uris": [uri] }),
                Duration::from_secs(8),
            )
            .await
            .map(|_| ())
    }

    pub async fn queue_add(&self, uri: &str) -> Result<(), BridgeError> {
        self.bridge
            .call_timeout(
                "queue.add",
                json!({ "uris": [uri] }),
                Duration::from_secs(8),
            )
            .await
            .map(|_| ())
    }

    /// Sloptify `extras.radio` — seed track/artist → inspired-by playlist URI.
    pub async fn radio_from_seed(&self, seed_uri: &str) -> Result<Option<String>, BridgeError> {
        let value = self
            .bridge
            .call_timeout(
                "radio.fromSeed",
                json!({ "uri": seed_uri }),
                Duration::from_secs(12),
            )
            .await?;
        Ok(value.as_str().filter(|s| !s.is_empty()).map(|s| s.to_string()))
    }

    /// Immersive "Play next" — insert before the first queued / next-up item.
    pub async fn queue_play_next(&self, uri: &str) -> Result<(), BridgeError> {
        let _ = self.refresh_queue().await;
        let snap = self.snapshot().await;
        let before = snap.queue.as_ref().and_then(|q| {
            q.queued
                .first()
                .or_else(|| q.next_up.first())
                .and_then(|t| t.uri.clone().map(|u| (u, t.uid.clone())))
        });
        if let Some((before_uri, before_uid)) = before {
            let mut before_obj = json!({ "uri": before_uri });
            if let Some(uid) = before_uid.filter(|u| !u.is_empty()) {
                before_obj["uid"] = json!(uid);
            }
            self.bridge
                .call_timeout(
                    "queue.insert",
                    json!({ "uris": [uri], "before": before_obj }),
                    Duration::from_secs(8),
                )
                .await
                .map(|_| ())
        } else {
            self.queue_add(uri).await
        }
    }

    pub async fn hide_window(&self) -> anyhow::Result<()> {
        self.bridge.hide_player_window().await
    }

    pub async fn show_window(&self) -> anyhow::Result<()> {
        self.bridge.show_player_window().await
    }

    /// `token.get` — anonymous means the user must sign in (Sloptify Android behaviour).
    pub async fn token_anonymous(&self) -> Result<Option<bool>, BridgeError> {
        let v = self.bridge.call("token.get", json!({})).await?;
        Ok(v.get("isAnonymous").and_then(|x| x.as_bool()))
    }

    pub async fn page_url(&self) -> anyhow::Result<String> {
        self.bridge.page_url().await
    }
}

impl StallWatch {
    fn reset(&mut self) {
        self.stuck_since = None;
        self.attempts = 0;
    }
}

/// Playlist / album / artist / collection — never a bare track or ad URI.
fn listening_context(raw: Option<&str>) -> Option<String> {
    let c = raw?.trim();
    if c.is_empty() || c.contains(":ad:") || c.contains(":track:") {
        return None;
    }
    Some(c.to_string())
}

/// First real song after the current item: page `next`, then queue / next-up.
fn next_music_uri(snap: &BridgeSnapshot) -> Option<String> {
    if let Some(uri) = snap
        .state
        .as_ref()
        .and_then(|s| s.next.as_ref())
        .filter(|t| t.is_playable_music())
        .and_then(|t| t.uri.clone())
    {
        return Some(uri);
    }
    let queue = snap.queue.as_ref()?;
    queue
        .queued
        .iter()
        .chain(queue.next_up.iter())
        .find(|t| t.is_playable_music())
        .and_then(|t| t.uri.clone())
}

fn queue_leads_with_ad(snap: &BridgeSnapshot) -> bool {
    let Some(queue) = snap.queue.as_ref() else {
        return false;
    };
    queue
        .queued
        .iter()
        .chain(queue.next_up.iter())
        .next()
        .is_some_and(|t| t.is_ad())
}
