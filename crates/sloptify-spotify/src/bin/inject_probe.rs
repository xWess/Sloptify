//! Probe: attach to CDP, inject bridge, print messages for 15s.
use std::time::Duration;

use sloptify_spotify::{Bridge, CdpHost, DEFAULT_CDP_URL};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let cdp = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_CDP_URL.to_string());
    eprintln!("connecting to {cdp}");
    let (host, messages) = CdpHost::connect(&cdp).await?;
    let (bridge, mut events) = Bridge::attach(host, messages).await;
    eprintln!("injecting…");
    bridge.inject().await?;
    eprintln!("waiting for ready (15s)…");

    let wait = tokio::spawn({
        let bridge = bridge.clone();
        async move {
            match bridge.wait_ready(Duration::from_secs(15)).await {
                Ok(()) => eprintln!("READY ok"),
                Err(e) => eprintln!("READY failed: {e}"),
            }
            let snap = bridge.snapshot().await;
            eprintln!("snapshot ready={} err={:?}", snap.ready, snap.last_error);
        }
    });

    let deadline = tokio::time::Instant::now() + Duration::from_secs(16);
    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            ev = events.recv() => {
                match ev {
                    Some(m) => eprintln!("event: {m:?}"),
                    None => break,
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
    let _ = wait.await;
    Ok(())
}
