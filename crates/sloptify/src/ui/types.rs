//! Shared UI types.

use sloptify_spotify::{BridgeSnapshot, ContextPage, HomeFeed, HomeItem, UserProfile};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Home,
    Search,
    Library,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    None,
    NowPlaying,
    Queue,
    Account,
    Plugins,
}

#[derive(Clone, PartialEq)]
pub enum MenuAction {
    Play { uri: String },
    PlayNext { uri: String },
    QueueAdd { uri: String },
    Open {
        uri: String,
        title: Option<String>,
        image: Option<String>,
    },
    ToggleLike { uri: String },
    SongRadio { uri: String, title: Option<String> },
    CopyLink { uri: String },
    Sep,
}

#[derive(Clone, PartialEq)]
pub struct CtxMenu {
    pub x: f64,
    pub y: f64,
    pub title: String,
    pub actions: Vec<(String, MenuAction)>,
}

#[derive(Clone, PartialEq)]
pub struct CatalogSlice {
    pub phase: crate::session::Phase,
    pub status: String,
    pub login_visible: bool,
    pub home: HomeFeed,
    pub search_results: Vec<HomeItem>,
    pub library: Vec<HomeItem>,
    pub context: Option<ContextPage>,
    pub profile: UserProfile,
}

#[derive(Clone, PartialEq)]
pub struct PlayerSlice {
    pub snapshot: BridgeSnapshot,
    pub volume: f64,
    pub liked: bool,
    pub toast: String,
}

pub const LIKED_URI: &str = "spotify:collection:tracks";
pub const SHELF_CAP: usize = 12;
