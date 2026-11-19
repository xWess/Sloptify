use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use tokio::time::timeout;

use crate::host::CdpHost;
use crate::messages::{BridgeMessage, PlayerState, Queue};

/// Bundled Sloptify/Myspoti player bridge (GPL-3.0), same file Android and CDP use.
pub const PLAYER_BRIDGE_JS: &str = include_str!("../../../assets/bridge/player-bridge.js");

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("bridge not ready — sign in to Spotify in the Chromium window if needed")]
    NotReady,
    #[error("bridge call timed out")]
    Timeout,
    #[error("bridge error: {0}")]
    Remote(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSnapshot {
    pub ready: bool,
    pub state: Option<PlayerState>,
    pub queue: Option<Queue>,
    pub last_error: Option<String>,
}

impl BridgeSnapshot {
    /// True when nothing the chrome UI cares about changed (playhead ticks ignored).
    /// Queue compares lightly — full queue payloads are loaded on demand in the sheet.
    pub fn ui_eq(&self, other: &Self) -> bool {
        self.ready == other.ready
            && self.last_error == other.last_error
            && queue_shell_eq(&self.queue, &other.queue)
            && match (&self.state, &other.state) {
                (None, None) => true,
                (Some(a), Some(b)) => a.ui_eq(b),
                _ => false,
            }
    }
}

fn queue_shell_eq(a: &Option<crate::messages::Queue>, b: &Option<crate::messages::Queue>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.queued.len() == b.queued.len()
                && a.next_up.len() == b.next_up.len()
                && a.current.as_ref().and_then(|t| t.uri.as_ref())
                    == b.current.as_ref().and_then(|t| t.uri.as_ref())
        }
        _ => false,
    }
}

/// Request/response + pushed events over a CDP host (Sloptify `Bridge`).
pub struct Bridge {
    host: Arc<CdpHost>,
    next_id: AtomicI64,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, String>>>>>,
    snapshot: Arc<RwLock<BridgeSnapshot>>,
    events: mpsc::UnboundedSender<BridgeMessage>,
}

impl Bridge {
    pub async fn attach(
        host: CdpHost,
        mut messages: mpsc::UnboundedReceiver<String>,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<BridgeMessage>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let host = Arc::new(host);
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let snapshot = Arc::new(RwLock::new(BridgeSnapshot::default()));

        let bridge = Arc::new(Self {
            host: host.clone(),
            next_id: AtomicI64::new(1),
            pending: pending.clone(),
            snapshot: snapshot.clone(),
            events: event_tx,
        });

        let bridge_reader = bridge.clone();
        tokio::spawn(async move {
            while let Some(raw) = messages.recv().await {
                if raw == "__page_load__" {
                    tracing::info!("page load — re-injecting bridge");
                    bridge_reader.snapshot.write().await.ready = false;
                    if let Err(err) = bridge_reader.inject().await {
                        tracing::warn!(?err, "re-inject after load failed");
                    }
                    continue;
                }
                if let Err(err) = bridge_reader.handle_raw(&raw).await {
                    tracing::debug!(?err, "dropped bridge message");
                }
            }
        });

        (bridge, event_rx)
    }

    pub async fn inject(&self) -> Result<()> {
        // Survive later navigations (login redirects, in-app routes).
        let _ = self.host.install_on_new_documents(PLAYER_BRIDGE_JS).await;
        self.host.inject_script(PLAYER_BRIDGE_JS).await?;
        // If the IIFE early-returned (already present), force a ready announce.
        let _ = self.reannounce().await;
        Ok(())
    }

    pub async fn snapshot(&self) -> BridgeSnapshot {
        self.snapshot.read().await.clone()
    }

    pub async fn set_queue(&self, queue: Queue) {
        self.snapshot.write().await.queue = Some(queue);
    }

    pub async fn reset_ready(&self) {
        self.snapshot.write().await.ready = false;
    }

    async fn handle_raw(&self, raw: &str) -> Result<()> {
        let msg: BridgeMessage = serde_json::from_str(raw)
            .map_err(|e| anyhow!("bad bridge json: {e}; raw={raw}"))?;
        match &msg {
            BridgeMessage::Ready { .. } => {
                self.snapshot.write().await.ready = true;
                self.snapshot.write().await.last_error = None;
                tracing::info!("Spotify player bridge ready");
                // Do not call ads.off here: handle_raw runs on the message loop, so a
                // round-trip would deadlock until timeout. The bridge JS latches AdManagers
                // in attach() / pushState() (Orpheon).
            }
            BridgeMessage::State { state } => {
                self.snapshot.write().await.state = state.clone();
            }
            BridgeMessage::Queue { queue } => {
                self.snapshot.write().await.queue = Some(queue.clone());
            }
            BridgeMessage::Result {
                id,
                ok,
                value,
                error,
            } => {
                if let Some(tx) = self.pending.lock().await.remove(id) {
                    if *ok {
                        let _ = tx.send(Ok(value.clone().unwrap_or(Value::Null)));
                    } else {
                        let _ = tx.send(Err(error.clone().unwrap_or_else(|| "unknown".into())));
                    }
                }
            }
            BridgeMessage::Error { error } => {
                // Soft: a late boot timeout in an iframe / prefs shell must not
                // tear down a bridge that already announced ready.
                let ready = self.snapshot.read().await.ready;
                if ready && error.contains("player services not found") {
                    tracing::debug!(%error, "ignoring late bridge boot noise (already ready)");
                } else {
                    self.snapshot.write().await.last_error = Some(error.clone());
                    tracing::warn!(%error, "bridge boot error");
                }
            }
            BridgeMessage::Log { line } => {
                tracing::info!(%line, "bridge");
            }
            BridgeMessage::Other => {}
        }
        let _ = self.events.send(msg);
        Ok(())
    }

    pub async fn call(&self, method: &str, args: Value) -> Result<Value, BridgeError> {
        self.call_timeout(method, args, Duration::from_secs(20))
            .await
    }

    /// Sloptify `Bridge.call(..., timeoutMs)` — ping uses ~3s; play/resume can be shorter.
    pub async fn call_timeout(
        &self,
        method: &str,
        args: Value,
        max: Duration,
    ) -> Result<Value, BridgeError> {
        if !self.snapshot.read().await.ready {
            return Err(BridgeError::NotReady);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let args_json = args.to_string();
        let method_json = serde_json::to_string(method).unwrap();
        let script = format!(
            "(window.Myspoti && window.Myspoti.call) ? (window.Myspoti.call({id}, {method_json}, {args_json}).catch(() => {{}}), true) : false"
        );
        self.host
            .evaluate(&script)
            .await
            .map_err(BridgeError::Other)?;

        match timeout(max, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(err))) => Err(BridgeError::Remote(err)),
            Ok(Err(_)) => Err(BridgeError::Other(anyhow!("call channel closed"))),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(BridgeError::Timeout)
            }
        }
    }

    /// Sloptify `Bridge.ping`: page answers `bridge.version` within a few seconds.
    pub async fn ping(&self, max: Duration) -> bool {
        self.call_timeout("bridge.version", Value::Object(Default::default()), max)
            .await
            .is_ok()
    }

    /// Sloptify `reloadAndWait`: soft-reload the Web Player tab and wait for a new `ready`.
    pub async fn reload_page(&self) -> Result<(), BridgeError> {
        self.reset_ready().await;
        // Sloptify WebPlayerHost PLAYER_URL — idle prefs SPA is fine; hard nav resets the player.
        self.host
            .navigate("https://open.spotify.com/")
            .await
            .map_err(BridgeError::Other)?;
        // Match Sloptify's 1.5s settle so the old page's ready is not mistaken for the new one.
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        self.wait_ready(Duration::from_secs(45)).await
    }

    pub async fn wait_ready(&self, max: Duration) -> Result<(), BridgeError> {
        let start = std::time::Instant::now();
        // Force an inject on the first loop iteration.
        let mut last_inject = start
            .checked_sub(Duration::from_secs(10))
            .unwrap_or(start);

        loop {
            if self.snapshot.read().await.ready {
                return Ok(());
            }
            if start.elapsed() > max {
                let hint = self
                    .snapshot
                    .read()
                    .await
                    .last_error
                    .clone()
                    .unwrap_or_else(|| "no ready message".into());
                return Err(BridgeError::Other(anyhow!(
                    "timed out waiting for bridge ({hint})"
                )));
            }
            if last_inject.elapsed() > Duration::from_secs(2) {
                if let Err(err) = self.inject().await {
                    tracing::warn!(?err, "inject while waiting");
                    return Err(BridgeError::Other(err));
                }
                last_inject = std::time::Instant::now();
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    pub async fn reannounce(&self) -> Result<()> {
        self.host
            .evaluate(
                "window.__mspReannounce = true; if (window.Myspoti && window.Myspoti.announce) window.Myspoti.announce();",
            )
            .await
    }

    pub async fn hide_player_window(&self) -> Result<()> {
        self.host.hide_window().await
    }

    pub async fn show_player_window(&self) -> Result<()> {
        self.host.show_window().await
    }

    pub async fn page_url(&self) -> Result<String> {
        self.host.current_url().await
    }

    pub async fn navigate(&self, url: &str) -> Result<()> {
        self.host.navigate(url).await
    }

    pub async fn browser_close(&self) -> Result<()> {
        self.host.browser_close().await
    }

    /// Spotify idles on `/preferences` after a while; the bridge can still say
    /// ready while GraphQL / playback are stuck. Force the player root.
    pub async fn ensure_player_root(&self) -> Result<(), BridgeError> {
        let url = self.page_url().await.unwrap_or_default();
        let idle = url.contains("/preferences")
            || url.contains("/account")
            || url.contains("/about-us");
        if !idle {
            return Ok(());
        }
        tracing::info!(%url, "leaving idle Spotify shell for player root");
        // SPA home click usually keeps Myspoti; only reset if we still look idle.
        self.host
            .navigate("https://open.spotify.com/")
            .await
            .map_err(BridgeError::Other)?;
        tokio::time::sleep(Duration::from_millis(800)).await;
        let after = self.page_url().await.unwrap_or_default();
        if after.contains("/preferences")
            || after.contains("/account")
            || after.contains("/about-us")
        {
            self.reset_ready().await;
            self.host
                .navigate("https://open.spotify.com/")
                .await
                .map_err(BridgeError::Other)?;
            tokio::time::sleep(Duration::from_millis(1_500)).await;
            return self.wait_ready(Duration::from_secs(45)).await;
        }
        if !self.snapshot.read().await.ready {
            self.inject().await?;
            self.wait_ready(Duration::from_secs(20)).await?;
        }
        Ok(())
    }
}
