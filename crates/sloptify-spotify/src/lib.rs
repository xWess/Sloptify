//! Spotify Web Player over Chrome DevTools Protocol.
//!
//! Architecture follows Sloptify/Myspoti (`:host-cdp` + `player-bridge.js`):
//! the official open.spotify.com player runs in Chromium; Sloptify drives it
//! through CDP and a page bridge. Audio stays in the browser Widevine CDM.

pub mod ad_block;
pub mod ad_schedule;
mod bridge;
mod host_fetch;
mod catalog;
mod chromium;
mod host;
mod login;
mod messages;
mod player;
mod window;

pub use login::is_login_url;

pub use bridge::{Bridge, BridgeError, BridgeSnapshot, PLAYER_BRIDGE_JS};
pub use catalog::{
    is_browsable_uri, ContextKind, ContextPage, ContextTrack, HomeChip, HomeFeed, HomeItem,
    HomeShelf, SpotifyCatalog, UserProfile,
};
pub use chromium::{ChromiumConfig, ChromiumSession};
pub use host::{CdpHost, BINDING_NAME, DEFAULT_CDP_URL, SPOTIFY_ORIGIN};
pub use messages::{BridgeMessage, PlayerRestrictions, PlayerState, Queue, TrackRef};
pub use player::{AdWatch, Player, StallWatch};

pub const APP_NAME: &str = "Sloptify";
pub const APP_ID: &str = "app.sloptify.desktop";
