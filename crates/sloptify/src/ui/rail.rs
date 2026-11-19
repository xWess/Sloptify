//! Library rail.

use dioxus::prelude::*;
use icons::{ChevronDown, ChevronLeft, ChevronRight};
use sloptify_spotify::HomeItem;

use super::helpers::{display_name, send};
use super::nav_ops::{go_nav, item_activate, navigate_back, navigate_forward};
use super::types::{CatalogSlice, CtxMenu, MenuAction, Nav, LIKED_URI};
use super::widgets::{Cover, NavBtn};
use crate::nav::NavStack;
use crate::session::{SessionHandle, UiCommand};

#[derive(Clone, Copy, PartialEq, Eq)]
enum LibKind {
    Playlist,
    Artist,
    Album,
    Other,
}

fn lib_kind(uri: &str) -> LibKind {
    if uri.contains(":artist:") {
        LibKind::Artist
    } else if uri.contains(":album:") {
        LibKind::Album
    } else if uri.contains(":playlist:")
        || uri.contains("collection:tracks")
        || uri.contains(":user:")
    {
        LibKind::Playlist
    } else {
        LibKind::Other
    }
}

fn open_menu(
    mut menu: Signal<Option<CtxMenu>>,
    e: &Event<MouseData>,
    title: String,
    actions: Vec<(String, MenuAction)>,
) {
    e.prevent_default();
    e.stop_propagation();
    let pt = e.client_coordinates();
    menu.set(Some(CtxMenu {
        x: pt.x,
        y: pt.y,
        title,
        actions,
    }));
}

#[component]
pub fn LibraryRail(
    library: Vec<HomeItem>,
    mut nav: Signal<Nav>,
    mut sec_playlists: Signal<bool>,
    mut sec_artists: Signal<bool>,
    mut sec_albums: Signal<bool>,
    open_ctx: String,
    in_context: bool,
) -> Element {
    let session = use_context::<SessionHandle>();
    let mut stack = use_context::<Signal<NavStack>>();
    let catalog = use_context::<Signal<CatalogSlice>>();

    use_effect({
        let session = session.clone();
        move || {
            send(
                &session,
                UiCommand::LoadLibrary {
                    filter: String::new(),
                },
            );
        }
    });

    let playlists: Vec<_> = library
        .iter()
        .filter(|i| lib_kind(&i.uri) == LibKind::Playlist)
        .cloned()
        .collect();
    let artists: Vec<_> = library
        .iter()
        .filter(|i| lib_kind(&i.uri) == LibKind::Artist)
        .cloned()
        .collect();
    let albums: Vec<_> = library
        .iter()
        .filter(|i| lib_kind(&i.uri) == LibKind::Album)
        .cloned()
        .collect();
    let others: Vec<_> = library
        .iter()
        .filter(|i| lib_kind(&i.uri) == LibKind::Other)
        .cloned()
        .collect();
    let liked_on = open_ctx == LIKED_URI;
    let can_back = stack().can_back();
    let can_forward = stack().can_forward();
    let ctx_page = catalog().context.clone();

    rsx! {
        aside { class: "rail",
            div { class: "rail-brand",
                span { class: "rail-mark", "O" }
                span { class: "rail-brand-name", "Sloptify" }
                div { class: "rail-hist",
                    button {
                        class: "rail-hist-btn",
                        disabled: !can_back,
                        title: "Back",
                        onclick: {
                            let session = session.clone();
                            let ctx = ctx_page.clone();
                            move |_| navigate_back(&session, &mut nav, &mut stack, ctx.as_ref())
                        },
                        ChevronLeft { class: "size-4" }
                    }
                    button {
                        class: "rail-hist-btn",
                        disabled: !can_forward,
                        title: "Forward",
                        onclick: {
                            let session = session.clone();
                            let ctx = ctx_page.clone();
                            move |_| navigate_forward(&session, &mut nav, &mut stack, ctx.as_ref())
                        },
                        ChevronRight { class: "size-4" }
                    }
                }
            }
            div { class: "rail-nav",
                NavBtn {
                    label: "Home", icon: "home",
                    active: !in_context && nav() == Nav::Home,
                    onclick: { let session = session.clone(); move |_| go_nav(&session, &mut nav, Nav::Home) }
                }
                NavBtn {
                    label: "Search", icon: "search",
                    active: !in_context && nav() == Nav::Search,
                    onclick: { let session = session.clone(); move |_| go_nav(&session, &mut nav, Nav::Search) }
                }
                NavBtn {
                    label: "Your Library", icon: "library",
                    active: !in_context && nav() == Nav::Library,
                    onclick: { let session = session.clone(); move |_| go_nav(&session, &mut nav, Nav::Library) }
                }
            }
            div { class: "rail-scroll",
                RailLibRow {
                    name: "Liked Songs".to_string(),
                    subtitle: "Collection".to_string(),
                    image: None,
                    uri: LIKED_URI.to_string(),
                    artist_cover: false,
                    active: liked_on,
                    heart: true,
                }
                RailSection {
                    label: "Playlists", open: sec_playlists, count: playlists.len(),
                    on_toggle: move |_| sec_playlists.set(!sec_playlists()),
                }
                if sec_playlists() {
                    for item in playlists {
                        RailLibRow {
                            key: "{item.uri}",
                            name: display_name(&item.name),
                            subtitle: if item.subtitle.is_empty() { "Playlist".to_string() } else { item.subtitle.clone() },
                            image: item.image.clone(),
                            uri: item.uri.clone(),
                            artist_cover: false,
                            active: open_ctx == item.uri,
                            heart: false,
                        }
                    }
                }
                RailSection {
                    label: "Artists", open: sec_artists, count: artists.len(),
                    on_toggle: move |_| sec_artists.set(!sec_artists()),
                }
                if sec_artists() {
                    for item in artists {
                        RailLibRow {
                            key: "{item.uri}",
                            name: display_name(&item.name),
                            subtitle: if item.subtitle.is_empty() { "Artist".to_string() } else { item.subtitle.clone() },
                            image: item.image.clone(),
                            uri: item.uri.clone(),
                            artist_cover: true,
                            active: open_ctx == item.uri,
                            heart: false,
                        }
                    }
                }
                RailSection {
                    label: "Albums", open: sec_albums, count: albums.len(),
                    on_toggle: move |_| sec_albums.set(!sec_albums()),
                }
                if sec_albums() {
                    for item in albums {
                        RailLibRow {
                            key: "{item.uri}",
                            name: display_name(&item.name),
                            subtitle: if item.subtitle.is_empty() { "Album".to_string() } else { item.subtitle.clone() },
                            image: item.image.clone(),
                            uri: item.uri.clone(),
                            artist_cover: false,
                            active: open_ctx == item.uri,
                            heart: false,
                        }
                    }
                }
                if !others.is_empty() {
                    p { class: "rail-sec-btn mt-1 !cursor-default", "Saved" }
                    for item in others {
                        RailLibRow {
                            key: "{item.uri}",
                            name: display_name(&item.name),
                            subtitle: item.subtitle.clone(),
                            image: item.image.clone(),
                            uri: item.uri.clone(),
                            artist_cover: false,
                            active: open_ctx == item.uri,
                            heart: false,
                        }
                    }
                }
                if library.is_empty() {
                    p { class: "px-2 py-6 text-center text-[0.78rem] text-muted-foreground",
                        "Loading your library…"
                    }
                }
            }
        }
    }
}

#[component]
fn RailSection(
    label: &'static str,
    open: Signal<bool>,
    count: usize,
    on_toggle: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        button {
            class: "rail-sec-btn mt-2",
            onclick: move |e| on_toggle.call(e),
            if open() {
                ChevronDown { class: "size-3.5 shrink-0" }
            } else {
                ChevronRight { class: "size-3.5 shrink-0" }
            }
            span { class: "min-w-0 flex-1 truncate text-left", "{label}" }
            span { class: "tabular-nums opacity-70", "{count}" }
        }
    }
}

#[component]
fn RailLibRow(
    name: String,
    subtitle: String,
    image: Option<String>,
    uri: String,
    artist_cover: bool,
    active: bool,
    heart: bool,
) -> Element {
    let session = use_context::<SessionHandle>();
    let mut stack = use_context::<Signal<NavStack>>();
    let nav = use_context::<Signal<Nav>>();
    let catalog = use_context::<Signal<CatalogSlice>>();
    let mut ctx_menu = use_context::<Signal<Option<CtxMenu>>>();
    let cover = if artist_cover {
        "cover-artist size-10 shrink-0".to_string()
    } else {
        "cover-soft size-10 shrink-0".to_string()
    };
    let row_class = if active { "rail-lib-row on" } else { "rail-lib-row" };
    let item = HomeItem {
        uri: uri.clone(),
        name: name.clone(),
        subtitle: subtitle.clone(),
        image: image.clone(),
    };

    rsx! {
        button {
            class: "{row_class}",
            onclick: {
                let session = session.clone();
                let item = item.clone();
                let ctx = catalog().context.clone();
                move |_| item_activate(&session, nav(), &mut stack, ctx.as_ref(), &item)
            },
            oncontextmenu: {
                let item = item.clone();
                move |e: Event<MouseData>| {
                    if item.uri == LIKED_URI {
                        open_menu(
                            ctx_menu, &e, "Liked Songs".to_string(),
                            vec![
                                ("Open".to_string(), MenuAction::Open {
                                    uri: LIKED_URI.to_string(),
                                    title: Some("Liked Songs".to_string()),
                                    image: None,
                                }),
                                ("Play".to_string(), MenuAction::Play { uri: LIKED_URI.to_string() }),
                            ],
                        );
                    } else {
                        open_menu(
                            ctx_menu, &e, display_name(&item.name),
                            super::helpers::item_menu_actions(&item),
                        );
                    }
                }
            },
            if heart {
                div { class: "cover-soft flex size-10 shrink-0 items-center justify-center bg-[var(--surface)] text-sm font-bold", "♥" }
            } else {
                Cover { url: image, class: cover }
            }
            div { class: "meta",
                span { class: "name", "{name}" }
                span { class: "sub", "{subtitle}" }
            }
        }
    }
}
