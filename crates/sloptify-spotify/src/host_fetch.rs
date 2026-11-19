//! CDP Fetch handlers for Orpheon-style AdBlock:
//! - Request: fail ad/tracker hosts
//! - Response: rewrite track-playback state machines (`without_ads`)

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use futures_util::SinkExt;
use serde_json::{json, Value};
use tokio::sync::{oneshot, Mutex};
use tokio_tungstenite::tungstenite::Message;

use crate::ad_block;
use crate::ad_schedule;
use crate::host::WsWriter;

type Pending = Arc<Mutex<std::collections::HashMap<u64, oneshot::Sender<Result<Value>>>>>;

/// ~50 ms mono silent MP3 (ffmpeg anullsrc). Landed ads "finish" instead of buffering forever.
const SILENT_MP3_B64: &str = "SUQzBAAAAAAAIlRTU0UAAAAOAAADTGF2ZjYzLjEuMTAyAAAAAAAAAAAAAAD/+0DAAAAAAAAAAAAAAAAAAAAAAABJbmZvAAAADwAAAAMAAAHuAJSUlJSUlJSUlJSUlJSUlJSUlJSUlJSUlJSUlJSUlJSUlMrKysrKysrKysrKysrKysrKysrKysrKysrKysrKysrKyv///////////////////////////////////////////wAAAABMYXZjNjMuMS4AAAAAAAAAAAAAAAAkAqMAAAAAAAAB7tU71IQAAAAAAP/7EMQAA8AAAaQAAAAgAAA0gAAABExBTUU0LjBVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVV//sQxCmDwAABpAAAACAAADSAAAAEVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVX/+xDEUwPAAAGkAAAAIAAANIAAAARVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVVQ==";

pub async fn handle_request_paused(
    params: &Value,
    writer: Arc<Mutex<WsWriter>>,
    pending: Pending,
    fetch_ids: Arc<AtomicU64>,
) {
    let request_id = params
        .get("requestId")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let url = params
        .pointer("/request/url")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    // Response stage exposes status; Request stage does not.
    let is_response = params.get("responseStatusCode").is_some();
    let track_playback = url.contains("/track-playback/");

    if !is_response {
        // Oct 20: failing the ad MP3 leaves the page stuck on the break forever.
        // Fulfill with a tiny silent MP3 so the player "plays" the ad and advances.
        if ad_block::is_ad_audio(&url) {
            tracing::info!(%url, "AdBlock fulfill silent MP3 (avoid Oct-20 stuck)");
            let _ = fire(
                &writer,
                &fetch_ids,
                "Fetch.fulfillRequest",
                json!({
                    "requestId": request_id,
                    "responseCode": 200,
                    "responseHeaders": [
                        { "name": "Content-Type", "value": "audio/mpeg" },
                        { "name": "Access-Control-Allow-Origin", "value": "*" }
                    ],
                    "body": SILENT_MP3_B64
                }),
            )
            .await;
            return;
        }
        if ad_block::is_blocked(&url) {
            tracing::info!(%url, "AdBlock Fetch.failRequest");
            let _ = fire(
                &writer,
                &fetch_ids,
                "Fetch.failRequest",
                json!({
                    "requestId": request_id,
                    "errorReason": "BlockedByClient"
                }),
            )
            .await;
            return;
        }
        let _ = fire(
            &writer,
            &fetch_ids,
            "Fetch.continueRequest",
            json!({ "requestId": request_id }),
        )
        .await;
        return;
    }

    // Response stage — rewrite track-playback when AdBlock is on.
    if !(ad_block::active() && track_playback) {
        let _ = fire(
            &writer,
            &fetch_ids,
            "Fetch.continueRequest",
            json!({ "requestId": request_id }),
        )
        .await;
        return;
    }

    let status = params
        .get("responseStatusCode")
        .and_then(|v| v.as_i64())
        .unwrap_or(200);
    if !(200..300).contains(&status) {
        let _ = fire(
            &writer,
            &fetch_ids,
            "Fetch.continueRequest",
            json!({ "requestId": request_id }),
        )
        .await;
        return;
    }

    match get_body(&writer, &pending, &fetch_ids, &request_id).await {
        Ok((body, _b64)) => {
            // Orpheon ControlRelay: rewrite first, then read adAhead from what the page sees.
            let final_body = ad_schedule::without_ads(&body).unwrap_or(body);
            ad_schedule::record(&final_body);
            let encoded = base64_encode(final_body.as_bytes());
            let headers = response_headers(params);
            let _ = fire(
                &writer,
                &fetch_ids,
                "Fetch.fulfillRequest",
                json!({
                    "requestId": request_id,
                    "responseCode": status,
                    "responseHeaders": headers,
                    "body": encoded
                }),
            )
            .await;
        }
        Err(err) => {
            tracing::warn!(?err, "AdBlock getResponseBody failed — continue");
            let _ = fire(
                &writer,
                &fetch_ids,
                "Fetch.continueRequest",
                json!({ "requestId": request_id }),
            )
            .await;
        }
    }
}

fn response_headers(params: &Value) -> Value {
    params
        .get("responseHeaders")
        .cloned()
        .unwrap_or_else(|| json!([]))
}

fn base64_encode(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(T[((n >> 18) & 63) as usize]);
        out.push(T[((n >> 12) & 63) as usize]);
        out.push(T[((n >> 6) & 63) as usize]);
        out.push(T[(n & 63) as usize]);
        i += 3;
    }
    match bytes.len() - i {
        1 => {
            let n = (bytes[i] as u32) << 16;
            out.push(T[((n >> 18) & 63) as usize]);
            out.push(T[((n >> 12) & 63) as usize]);
            out.push(b'=');
            out.push(b'=');
        }
        2 => {
            let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
            out.push(T[((n >> 18) & 63) as usize]);
            out.push(T[((n >> 12) & 63) as usize]);
            out.push(T[((n >> 6) & 63) as usize]);
            out.push(b'=');
        }
        _ => {}
    }
    String::from_utf8(out).unwrap_or_default()
}

async fn fire(
    writer: &Arc<Mutex<WsWriter>>,
    fetch_ids: &Arc<AtomicU64>,
    method: &str,
    params: Value,
) -> Result<()> {
    let id = fetch_ids.fetch_add(1, Ordering::SeqCst);
    let payload = json!({ "id": id, "method": method, "params": params });
    writer
        .lock()
        .await
        .send(Message::Text(payload.to_string().into()))
        .await
        .map_err(|e| anyhow!("fetch fire: {e}"))?;
    Ok(())
}

async fn get_body(
    writer: &Arc<Mutex<WsWriter>>,
    pending: &Pending,
    fetch_ids: &Arc<AtomicU64>,
    request_id: &str,
) -> Result<(String, bool)> {
    let id = fetch_ids.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = oneshot::channel();
    pending.lock().await.insert(id, tx);
    let payload = json!({
        "id": id,
        "method": "Fetch.getResponseBody",
        "params": { "requestId": request_id }
    });
    writer
        .lock()
        .await
        .send(Message::Text(payload.to_string().into()))
        .await
        .map_err(|e| anyhow!("getResponseBody send: {e}"))?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(8), rx)
        .await
        .map_err(|_| anyhow!("getResponseBody timeout"))?
        .map_err(|_| anyhow!("getResponseBody closed"))??;
    let b64 = result
        .get("base64Encoded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let body = result
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let text = if b64 {
        String::from_utf8(base64_decode(&body)?).unwrap_or_default()
    } else {
        body
    };
    Ok((text, b64))
}

fn base64_decode(s: &str) -> Result<Vec<u8>> {
    fn val(c: u8) -> Result<u8> {
        Ok(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(anyhow!("bad base64")),
        })
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let a = val(bytes[i])?;
        let b = val(bytes[i + 1])?;
        let pad_c = bytes[i + 2] == b'=';
        let pad_d = bytes[i + 3] == b'=';
        let c = if pad_c { 0 } else { val(bytes[i + 2])? };
        let d = if pad_d { 0 } else { val(bytes[i + 3])? };
        let n = ((a as u32) << 18) | ((b as u32) << 12) | ((c as u32) << 6) | (d as u32);
        out.push(((n >> 16) & 0xff) as u8);
        if !pad_c {
            out.push(((n >> 8) & 0xff) as u8);
        }
        if !pad_d {
            out.push((n & 0xff) as u8);
        }
        i += 4;
    }
    Ok(out)
}
