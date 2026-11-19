//! Visibility of the Web Player.
//!
//! Sloptify Android keeps a WebView as a 1×1 INVISIBLE child inside the app
//! (`WebPlayerHost.attach`). On desktop the equivalent is Chromium on a private
//! Xvfb display (see `chromium.rs`) — Hyprland never gets a window to park.
//!
//! These CDP helpers are kept as soft fallbacks only. Real show/hide is
//! `ChromiumSession::relaunch_as(visible)` from the session layer.

use anyhow::{Context, Result};
use serde_json::json;

use crate::host::CdpHost;

impl CdpHost {
    /// No-op when Chromium is on Xvfb (there is no compositor window). Soft minimize otherwise.
    pub async fn hide_window(&self) -> Result<()> {
        let _ = self.hide_via_cdp().await;
        Ok(())
    }

    /// Soft restore via CDP. Login visibility must use `ChromiumSession::relaunch_as(true)`.
    pub async fn show_window(&self) -> Result<()> {
        let _ = self.show_via_cdp().await;
        Ok(())
    }

    async fn window_id(&self) -> Result<i64> {
        let result = self
            .send("Browser.getWindowForTarget", json!({}))
            .await
            .context("Browser.getWindowForTarget")?;
        result
            .get("windowId")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| anyhow::anyhow!("no windowId in {result}"))
    }

    async fn hide_via_cdp(&self) -> Result<()> {
        let id = self.window_id().await?;
        self.send(
            "Browser.setWindowBounds",
            json!({ "windowId": id, "bounds": { "windowState": "minimized" } }),
        )
        .await?;
        Ok(())
    }

    async fn show_via_cdp(&self) -> Result<()> {
        let id = self.window_id().await?;
        self.send(
            "Browser.setWindowBounds",
            json!({
                "windowId": id,
                "bounds": {
                    "left": 120, "top": 80, "width": 1000, "height": 700,
                    "windowState": "normal"
                }
            }),
        )
        .await?;
        Ok(())
    }
}
