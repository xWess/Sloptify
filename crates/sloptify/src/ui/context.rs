//! Context (playlist / album / artist / liked) screen.

use dioxus::prelude::*;
use icons::Play;
use sloptify_spotify::{ContextKind, ContextPage, ContextTrack, HomeItem, HomeShelf};

use super::helpers::{display_name, fmt_ms, pretty_description, send, trunc, track_menu_actions};
use super::nav_ops::item_activate;
use super::types::{CatalogSlice, CtxMenu, MenuAction, Nav, SHELF_CAP};
use super::widgets::Cover;
use crate::nav::NavStack;
use crate::session::{SessionHandle, UiCommand};

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
pub fn ContextView(page: ContextPage, playing_uri: Option<String>) -> Element {
    let session = use_context::<SessionHandle>();
    let kind = page.kind;
    let hero_class = match kind {
        ContextKind::Playlist => "ctx-hero playlist",
        ContextKind::Album => "ctx-hero album",
        ContextKind::Artist => "ctx-hero artist",
        ContextKind::Liked => "ctx-hero liked",
        ContextKind::Other => "ctx-hero other",
    };
    let title = display_name(&page.title);
    let subtitle = pretty_description(&page.subtitle);
    let desc = pretty_description(&page.description);
    let tracks = page.tracks.clone();
    let shelves = page.shelves.clone();
    let uri = page.uri.clone();
    let image = page.image.clone();
    let can_more = page.next_offset.is_some() && !page.loading;
    let meta = {
        let mut parts = Vec::new();
        if !subtitle.is_empty() {
            parts.push(subtitle.clone());
        }
        if !desc.is_empty() && desc != subtitle && kind != ContextKind::Artist {
            parts.push(desc.clone());
        }
        parts.join("  ·  ")
    };
    // Album context: tracks share art — show index only. Else show thumbs.
    let show_art = kind != ContextKind::Album;
    let show_album_line = kind == ContextKind::Artist || kind == ContextKind::Other;
    let is_artist = kind == ContextKind::Artist;

    rsx! {
        div { class: "ctx-screen",
            div { class: "{hero_class}",
                // Ambient wash only — a live blurred <img> tanks WebKit scroll FPS.
                div { class: "ctx-hero-wash" }
                div { class: "ctx-hero-fade" }
                Cover {
                    url: image.clone(),
                    class: if kind == ContextKind::Artist {
                        "ctx-cover cover-artist".to_string()
                    } else {
                        "ctx-cover cover-soft".to_string()
                    }
                }
                div { class: "ctx-hero-meta",
                    p { class: "ctx-kicker",
                        match kind {
                            ContextKind::Playlist => "PLAYLIST",
                            ContextKind::Album => "ALBUM",
                            ContextKind::Artist => "ARTIST",
                            ContextKind::Liked => "COLLECTION",
                            ContextKind::Other => "CONTEXT",
                        }
                    }
                    h1 { class: "ctx-title", "{title}" }
                    if !meta.is_empty() {
                        p { class: "ctx-meta-line", "{meta}" }
                    }
                    div { class: "ctx-actions",
                        button {
                            class: "imm-play ctx-play",
                            title: "Play",
                            onclick: {
                                let session = session.clone();
                                let uri = uri.clone();
                                move |_| send(&session, UiCommand::PlayUri { uri: uri.clone() })
                            },
                            Play { class: "size-5 fill-current" }
                        }
                        button {
                            class: "ctx-text-btn",
                            onclick: {
                                let session = session.clone();
                                move |_| send(&session, UiCommand::CloseContext)
                            },
                            "Close"
                        }
                    }
                }
            }

            if is_artist {
                ArtistBody {
                    uri: uri.clone(),
                    tracks,
                    shelves,
                    playing_uri,
                    loading: page.loading,
                    error: page.error.clone(),
                }
            } else {
                div { class: "ctx-tracks",
                    div { class: if show_art { "ctx-song-head with-art" } else { "ctx-song-head" },
                        span { class: "ctx-song-head-idx", "#" }
                        if show_art { span { class: "ctx-song-head-art" } }
                        span { "TITLE" }
                        span { class: "text-right", "TIME" }
                    }
                    for (i, track) in tracks.into_iter().enumerate() {
                        {
                            let playing = playing_uri.as_ref() == Some(&track.uri);
                            rsx! {
                                TrackRow {
                                    key: "{track.uri}-{i}",
                                    index: i,
                                    track,
                                    context_uri: uri.clone(),
                                    show_art,
                                    show_album_line,
                                    playing,
                                }
                            }
                        }
                    }
                    if page.loading {
                        p { class: "text-muted-foreground px-4 py-6 text-sm", "Loading…" }
                    } else if !page.error.is_empty() && page.tracks.is_empty() {
                        p { class: "text-muted-foreground px-4 py-6 text-sm", "{page.error}" }
                    }
                    if can_more {
                        button {
                            class: "ctx-more",
                            onclick: {
                                let session = session.clone();
                                move |_| send(&session, UiCommand::LoadMoreContext)
                            },
                            "Load more"
                        }
                    }
                }
            }
        }
    }
}

/// Spotify-style artist page: Popular + discography shelves + appears on / related.
#[component]
fn ArtistBody(
    uri: String,
    tracks: Vec<ContextTrack>,
    shelves: Vec<HomeShelf>,
    playing_uri: Option<String>,
    loading: bool,
    error: String,
) -> Element {
    let session = use_context::<SessionHandle>();
    let mut stack = use_context::<Signal<NavStack>>();
    let nav = use_context::<Signal<Nav>>();
    let catalog = use_context::<Signal<CatalogSlice>>();
    let mut ctx_menu = use_context::<Signal<Option<CtxMenu>>>();
    // 0 = All discography, then one per album/single/compilation shelf index in `disco`.
    let mut disco_tab = use_signal(|| 0usize);
    let mut popular_open = use_signal(|| false);

    let disco: Vec<HomeShelf> = shelves
        .iter()
        .filter(|s| {
            matches!(
                s.title.as_str(),
                "Albums" | "Singles and EPs" | "Compilations" | "Discography"
            )
        })
        .cloned()
        .collect();
    let extra: Vec<HomeShelf> = shelves
        .iter()
        .filter(|s| {
            !matches!(
                s.title.as_str(),
                "Albums" | "Singles and EPs" | "Compilations" | "Discography"
            )
        })
        .cloned()
        .collect();

    let tab_labels: Vec<String> = {
        let mut labels = vec!["All".to_string()];
        for s in &disco {
            let short = match s.title.as_str() {
                "Singles and EPs" => "Singles",
                other => other,
            };
            labels.push(short.to_string());
        }
        labels
    };

    let visible_disco: Vec<HomeItem> = if disco_tab() == 0 {
        disco.iter().flat_map(|s| s.items.clone()).collect()
    } else {
        disco
            .get(disco_tab().saturating_sub(1))
            .map(|s| s.items.clone())
            .unwrap_or_default()
    };

    let popular_total = tracks.len();
    let popular_cap = if popular_open() {
        popular_total
    } else {
        popular_total.min(5)
    };
    let popular_tracks: Vec<(usize, ContextTrack)> = tracks
        .into_iter()
        .enumerate()
        .take(popular_cap)
        .collect();
    let can_expand_popular = popular_total > 5;

    rsx! {
        div { class: "ctx-artist-body",
            if loading && popular_total == 0 && shelves.is_empty() {
                p { class: "text-muted-foreground px-4 py-6 text-sm", "Loading…" }
            } else if !error.is_empty() && popular_total == 0 && shelves.is_empty() {
                p { class: "text-muted-foreground px-4 py-6 text-sm", "{error}" }
            }

            if !popular_tracks.is_empty() {
                section { class: "ctx-section",
                    h2 { class: "ctx-section-label", "Popular" }
                    div { class: "ctx-tracks ctx-tracks-tight",
                        for (i, track) in popular_tracks.into_iter() {
                            {
                                let playing = playing_uri.as_ref() == Some(&track.uri);
                                rsx! {
                                    TrackRow {
                                        key: "pop-{track.uri}-{i}",
                                        index: i,
                                        track,
                                        context_uri: uri.clone(),
                                        show_art: true,
                                        show_album_line: true,
                                        playing,
                                    }
                                }
                            }
                        }
                    }
                    if can_expand_popular {
                        button {
                            class: "ctx-popular-more",
                            onclick: move |_| popular_open.set(!popular_open()),
                            if popular_open() { "Show less" } else { "Show more" }
                        }
                    }
                }
            }

            if !disco.is_empty() {
                section { class: "ctx-section",
                    h2 { class: "ctx-section-label", "Discography" }
                    if tab_labels.len() > 2 {
                        div { class: "ctx-disco-tabs",
                            for (i, label) in tab_labels.into_iter().enumerate() {
                                {
                                    let on = disco_tab() == i;
                                    rsx! {
                                        button {
                                            key: "disco-tab-{i}",
                                            class: if on { "ctx-disco-tab on" } else { "ctx-disco-tab" },
                                            onclick: move |_| disco_tab.set(i),
                                            "{label}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "shelf-row ctx-disco-row",
                        for item in visible_disco.into_iter().take(SHELF_CAP.max(24)) {
                            {
                                let item_c = item.clone();
                                let card_uri = item.uri.clone();
                                let name = trunc(&item.name, 22);
                                let sub = trunc(&item.subtitle, 24);
                                let image = item.image.clone();
                                rsx! {
                                    button {
                                        key: "disco-{card_uri}",
                                        class: "home-sleeve",
                                        onclick: {
                                            let session = session.clone();
                                            let item = item_c.clone();
                                            let ctx = catalog().context.clone();
                                            move |_| item_activate(
                                                &session, nav(), &mut stack, ctx.as_ref(), &item,
                                            )
                                        },
                                        oncontextmenu: {
                                            let item = item_c.clone();
                                            move |e: Event<MouseData>| {
                                                open_menu(
                                                    ctx_menu,
                                                    &e,
                                                    display_name(&item.name),
                                                    super::helpers::item_menu_actions(&item),
                                                );
                                            }
                                        },
                                        div { class: "home-sleeve-art",
                                            Cover { url: image, class: "size-full".to_string() }
                                            span { class: "play-pop home-sleeve-play",
                                                Play { class: "size-5 fill-current" }
                                            }
                                        }
                                        p { class: "home-sleeve-name", "{name}" }
                                        if !sub.is_empty() {
                                            p { class: "home-sleeve-sub", "{sub}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            for shelf in extra {
                {
                    let shelf_title = shelf.title.clone();
                    rsx! {
                        section { class: "ctx-section",
                            key: "{shelf_title}",
                            h2 { class: "ctx-section-label", "{shelf_title}" }
                            div { class: "shelf-row ctx-disco-row",
                                for item in shelf.items.into_iter().take(SHELF_CAP) {
                                    {
                                        let item_c = item.clone();
                                        let card_uri = item.uri.clone();
                                        let name = trunc(&item.name, 22);
                                        let sub = trunc(&item.subtitle, 24);
                                        let image = item.image.clone();
                                        let round = card_uri.contains(":artist:");
                                        rsx! {
                                            button {
                                                key: "{shelf_title}-{card_uri}",
                                                class: if round { "home-sleeve is-artist" } else { "home-sleeve" },
                                                onclick: {
                                                    let session = session.clone();
                                                    let item = item_c.clone();
                                                    let ctx = catalog().context.clone();
                                                    move |_| item_activate(
                                                        &session, nav(), &mut stack, ctx.as_ref(), &item,
                                                    )
                                                },
                                                oncontextmenu: {
                                                    let item = item_c.clone();
                                                    move |e: Event<MouseData>| {
                                                        open_menu(
                                                            ctx_menu,
                                                            &e,
                                                            display_name(&item.name),
                                                            super::helpers::item_menu_actions(&item),
                                                        );
                                                    }
                                                },
                                                div { class: if round { "home-sleeve-art round" } else { "home-sleeve-art" },
                                                    Cover { url: image, class: "size-full".to_string() }
                                                    span { class: "play-pop home-sleeve-play",
                                                        Play { class: "size-5 fill-current" }
                                                    }
                                                }
                                                p { class: "home-sleeve-name", "{name}" }
                                                if !sub.is_empty() {
                                                    p { class: "home-sleeve-sub", "{sub}" }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn TrackRow(
    index: usize,
    track: ContextTrack,
    context_uri: String,
    show_art: bool,
    show_album_line: bool,
    playing: bool,
) -> Element {
    let session = use_context::<SessionHandle>();
    let mut ctx_menu = use_context::<Signal<Option<CtxMenu>>>();
    let row_class = match (playing, show_art) {
        (true, true) => "ctx-song on with-art",
        (true, false) => "ctx-song on",
        (false, true) => "ctx-song with-art",
        (false, false) => "ctx-song",
    };
    let name = track.name.clone();
    let artists = track.artists.clone();
    let album = track.album.clone();
    let dur = fmt_ms(track.duration_ms);
    let idx = format!("{}", index + 1);
    let image = track.image.clone();
    let track_uri = track.uri.clone();
    let track_uid = track.uid.clone();

    rsx! {
        button {
            class: "{row_class}",
            onclick: {
                let session = session.clone();
                let context_uri = context_uri.clone();
                let track_uri = track_uri.clone();
                let track_uid = track_uid.clone();
                move |_| {
                    send(
                        &session,
                        UiCommand::PlayInContext {
                            context_uri: context_uri.clone(),
                            track_uri: track_uri.clone(),
                            track_uid: track_uid.clone(),
                            index: Some(index as i64),
                        },
                    );
                }
            },
            oncontextmenu: {
                let track = track.clone();
                move |e: Event<MouseData>| {
                    open_menu(
                        ctx_menu,
                        &e,
                        display_name(&track.name),
                        track_menu_actions(&track),
                    );
                }
            },
            span { class: "ctx-song-idx", "{idx}" }
            span { class: "ctx-song-play", "aria-hidden": "true",
                Play { class: "size-3.5 fill-current" }
            }
            if show_art {
                Cover { url: image, class: "ctx-song-art".to_string() }
            }
            div { class: "ctx-song-meta",
                p { class: "ctx-song-title", "{name}" }
                p { class: "ctx-song-sub",
                    if show_album_line && !album.is_empty() {
                        "{artists} · {album}"
                    } else {
                        "{artists}"
                    }
                }
            }
            span { class: "ctx-song-time", "{dur}" }
        }
    }
}
