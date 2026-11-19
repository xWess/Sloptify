use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use tokio::time::timeout;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::ad_block;

pub const DEFAULT_CDP_URL: &str = "http://127.0.0.1:9222";
pub const SPOTIFY_ORIGIN: &str = "https://open.spotify.com";
pub const BINDING_NAME: &str = "__mspHostPost";

#[derive(Debug, Clone, serde::Deserialize)]
struct TargetInfo {
    #[serde(rename = "type")]
    kind: String,
    url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    ws_url: Option<String>,
}

pub(crate) type WsWriter = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

/// CDP BridgeHost equivalent of Sloptify's `CdpHost`.
pub struct CdpHost {
    next_id: AtomicU64,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>,
    writer: Arc<Mutex<WsWriter>>,
    page_loads: Arc<RwLock<u64>>,
}

impl CdpHost {
    /// Connect and return the host plus a channel of bridge `__mspHostPost` payloads.
    /// Also emits `"__page_load__"` on each `Page.loadEventFired` so the session can re-inject.
    pub async fn connect(cdp_url: &str) -> Result<(Self, mpsc::UnboundedReceiver<String>)> {
        let client = reqwest::Client::new();
        let base = cdp_url.trim_end_matches('/');
        let targets: Vec<TargetInfo> = client
            .get(format!("{base}/json/list"))
            .send()
            .await
            .context("list CDP targets")?
            .json()
            .await
            .context("parse CDP target list")?;

        let page = if let Some(t) = targets.into_iter().find(|t| {
            t.kind == "page" && t.url.starts_with(SPOTIFY_ORIGIN) && t.ws_url.is_some()
        }) {
            t
        } else {
            let created: TargetInfo = client
                .put(format!("{base}/json/new?{SPOTIFY_ORIGIN}"))
                .send()
                .await
                .context("open Spotify tab")?
                .json()
                .await
                .context("parse new target")?;
            created
        };

        let ws_url = page
            .ws_url
            .ok_or_else(|| anyhow!("Spotify tab has no webSocketDebuggerUrl"))?;

        tracing::info!(url = %page.url, "attaching CDP to Spotify tab");

        let (ws, _) = connect_async(&ws_url)
            .await
            .with_context(|| format!("CDP websocket {ws_url}"))?;
        let (writer, mut reader) = ws.split();

        let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        let page_loads = Arc::new(RwLock::new(0u64));

        let pending_reader = pending.clone();
        let loads = page_loads.clone();
        let writer_shared = Arc::new(Mutex::new(writer));
        let writer_fetch = writer_shared.clone();
        let fetch_ids = Arc::new(AtomicU64::new(900_000));
        tokio::spawn(async move {
            while let Some(Ok(msg)) = reader.next().await {
                let text = match msg {
                    Message::Text(t) => t.to_string(),
                    Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                    Message::Close(_) => break,
                    _ => continue,
                };
                let Ok(v) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                    let result = if let Some(err) = v.get("error") {
                        Err(anyhow!("CDP error: {err}"))
                    } else {
                        Ok(v.get("result").cloned().unwrap_or(Value::Null))
                    };
                    if let Some(tx) = pending_reader.lock().await.remove(&id) {
                        let _ = tx.send(result);
                    }
                    continue;
                }
                let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
                let params = v.get("params").cloned().unwrap_or(Value::Null);
                match method {
                    "Runtime.bindingCalled" => {
                        let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        if name == BINDING_NAME {
                            if let Some(payload) = params.get("payload").and_then(|p| p.as_str()) {
                                let _ = msg_tx.send(payload.to_string());
                            }
                        }
                    }
                    "Page.loadEventFired" => {
                        *loads.write().await += 1;
                        let _ = msg_tx.send("__page_load__".into());
                    }
                    // AdBlock: fail ad hosts (Request) + rewrite track-playback (Response).
                    "Fetch.requestPaused" => {
                        let writer = writer_fetch.clone();
                        let pending = pending_reader.clone();
                        let ids = fetch_ids.clone();
                        let params = params.clone();
                        tokio::spawn(async move {
                            crate::host_fetch::handle_request_paused(
                                &params, writer, pending, ids,
                            )
                            .await;
                        });
                    }
                    _ => {}
                }
            }
            tracing::warn!("CDP websocket closed");
        });

        let host = Self {
            next_id: AtomicU64::new(1),
            pending,
            writer: writer_shared,
            page_loads,
        };

        host.send("Runtime.enable", json!({})).await?;
        host.send("Page.enable", json!({})).await?;
        host.send("Network.enable", json!({})).await?;
        host.send("Runtime.addBinding", json!({ "name": BINDING_NAME }))
            .await?;

        // Request: refuse ad/tracker hosts (incl. *.scdn.co for mp3-ad). Response: withoutAds.
        let mut patterns: Vec<Value> = ad_block::fetch_url_patterns()
            .into_iter()
            .map(|url_pattern| {
                json!({
                    "urlPattern": url_pattern,
                    "requestStage": "Request"
                })
            })
            .collect();
        patterns.push(json!({
            "urlPattern": "*/track-playback/*",
            "requestStage": "Request"
        }));
        patterns.push(json!({
            "urlPattern": "*/track-playback/*",
            "requestStage": "Response"
        }));
        host.send("Fetch.enable", json!({ "patterns": patterns }))
            .await?;

        // Sloptify: Chromium is the player — AdBlock on for free local playback.
        ad_block::set_active(true);

        Ok((host, msg_rx))
    }

    pub async fn send(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let payload = json!({ "id": id, "method": method, "params": params });
        self.writer
            .lock()
            .await
            .send(Message::Text(payload.to_string().into()))
            .await
            .context("write CDP frame")?;
        match timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => bail!("CDP response channel closed for {method}"),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                bail!("CDP timeout for {method}")
            }
        }
    }

    pub async fn evaluate(&self, script: &str) -> Result<()> {
        let result = self
            .send(
                "Runtime.evaluate",
                json!({
                    "expression": script,
                    "awaitPromise": false,
                    "returnByValue": false,
                }),
            )
            .await?;
        if let Some(exc) = result.get("exceptionDetails") {
            bail!("page script error: {exc}");
        }
        Ok(())
    }

    pub async fn evaluate_value(&self, script: &str) -> Result<Value> {
        let result = self
            .send(
                "Runtime.evaluate",
                json!({
                    "expression": script,
                    "awaitPromise": true,
                    "returnByValue": true,
                }),
            )
            .await?;
        if let Some(exc) = result.get("exceptionDetails") {
            bail!("page script error: {exc}");
        }
        Ok(result
            .pointer("/result/value")
            .cloned()
            .unwrap_or(Value::Null))
    }

    pub async fn current_url(&self) -> Result<String> {
        let v = self.evaluate_value("location.href").await?;
        Ok(v.as_str().unwrap_or("").to_string())
    }

    pub async fn inject_script(&self, source: &str) -> Result<()> {
        self.evaluate(source).await
    }

    /// Persist the bridge across SPA / full navigations in this tab.
    pub async fn install_on_new_documents(&self, source: &str) -> Result<()> {
        self.send(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": source }),
        )
        .await?;
        Ok(())
    }

    pub async fn reload(&self) -> Result<()> {
        self.send("Page.reload", json!({ "ignoreCache": false })).await?;
        Ok(())
    }

    /// Navigate the Spotify tab (Sloptify `WebPlayerHost` reload → `playerRoot()`).
    ///
    /// Idle `/preferences` swallows both CDP `Page.navigate` and `location.assign`.
    /// Prefer clicking Spotify's own home link (SPA router); fall back to assign / CDP.
    pub async fn navigate(&self, url: &str) -> Result<()> {
        let url_json = serde_json::to_string(url).unwrap_or_else(|_| "\"https://open.spotify.com/\"".into());
        let script = format!(
            r#"(function(u){{
  try {{
    var path = '/';
    try {{ path = new URL(u, location.origin).pathname || '/'; }} catch (_) {{}}
    if (path === '/' || path === '') {{
      var el = document.querySelector('a[href="/"]')
        || document.querySelector('[aria-label="Home"]')
        || Array.prototype.find.call(document.querySelectorAll('a'), function(a){{
             return (a.getAttribute('href') || '') === '/';
           }});
      if (el) {{ el.click(); return 'click'; }}
    }}
    if (location.href !== u) location.assign(u);
    return 'assign';
  }} catch (_) {{ return 'fail'; }}
}})({url_json})"#
        );
        match self.evaluate(&script).await {
            Ok(()) => Ok(()),
            Err(_) => {
                self.send("Page.navigate", json!({ "url": url })).await?;
                Ok(())
            }
        }
    }

    pub async fn page_load_count(&self) -> u64 {
        *self.page_loads.read().await
    }
}
