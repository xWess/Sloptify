//! Sloptify shell — Dioxus + Rust/UI. Catalog UI is isolated from playhead ticks.

mod browse;
mod context;
mod helpers;
mod nav_ops;
mod player;
mod rail;
mod sheets;
mod types;
mod widgets;

use dioxus::prelude::*;
use icons::{House, Library, Search};

use crate::nav::NavStack;
use crate::plugins::PluginsState;
use crate::session::{spawn_session, Phase, SessionHandle, UiCommand};

use browse::{HomeView, LibraryView, SearchView};
use context::ContextView;
use helpers::send;
use player::{NowPlayingSheet, PlayerBar};
use rail::LibraryRail;
use sheets::{
    AccountSheet, ContextMenuOverlay, PluginHost, PluginsSheet, QueueSheet,
};
use widgets::ProfileDisc;
use types::{CatalogSlice, CtxMenu, Nav, PlayerSlice, Sheet};
use widgets::tab_class;

pub fn app() -> Element {
    let session = use_hook(spawn_session);
    use_context_provider(|| session.clone());

    let mut catalog = use_signal(|| CatalogSlice {
        phase: Phase::Starting,
        status: "Starting…".into(),
        home: Default::default(),
        search_results: Vec::new(),
        library: Vec::new(),
        context: None,
        profile: Default::default(),
    });
    let mut player = use_signal(|| PlayerSlice {
        snapshot: Default::default(),
        volume: 0.8,
        liked: false,
        toast: String::new(),
    });
    let nav = use_signal(|| Nav::Home);
    let mut playing_uri = use_signal(|| None::<String>);
    let chip = use_signal(|| 0usize);
    let search = use_signal(String::new);
    let sheet = use_signal(|| Sheet::None);
    let seek_drag = use_signal(|| None::<i64>);
    let vol_drag = use_signal(|| None::<f64>);
    let ctx_menu = use_signal(|| None::<CtxMenu>);
    let stack = use_signal(NavStack::default);
    let plugins = use_signal(PluginsState::refresh);
    use_context_provider(|| ctx_menu);
    use_context_provider(|| stack);
    use_context_provider(|| nav);
    use_context_provider(|| catalog);

    {
        let session = session.clone();
        use_future(move || {
            let mut rx = session.state_rx.clone();
            async move {
                loop {
                    let s = rx.borrow().clone();
                    let next_cat = CatalogSlice {
                        phase: s.phase,
                        status: s.status.clone(),
                        home: s.home.clone(),
                        search_results: s.search_results.clone(),
                        library: s.library.clone(),
                        context: s.context.clone(),
                        profile: s.profile.clone(),
                    };
                    let next_play = PlayerSlice {
                        snapshot: s.snapshot.clone(),
                        volume: s.volume,
                        liked: s.liked,
                        toast: String::new(),
                    };
                    let next_uri = next_play
                        .snapshot
                        .state
                        .as_ref()
                        .and_then(|st| st.track.as_ref())
                        .and_then(|t| t.uri.clone());

                    {
                        let cur = catalog.peek();
                        let same = cur.phase == next_cat.phase
                            && cur.status == next_cat.status
                            && cur.home == next_cat.home
                            && cur.search_results == next_cat.search_results
                            && cur.library == next_cat.library
                            && cur.context == next_cat.context
                            && cur.profile == next_cat.profile;
                        if !same {
                            drop(cur);
                            catalog.set(next_cat);
                        }
                    }

                    {
                        let cur = player.peek();
                        let same_shell = cur.volume == next_play.volume
                            && cur.liked == next_play.liked
                            && cur.snapshot.ui_eq(&next_play.snapshot);
                        // ui_eq ignores playhead on purpose (tick spam) — but seek MUST land in the chrome.
                        let playhead_changed = match (
                            cur.snapshot.state.as_ref(),
                            next_play.snapshot.state.as_ref(),
                        ) {
                            (Some(a), Some(b)) => {
                                a.position_ms != b.position_ms || a.timestamp != b.timestamp
                            }
                            (None, None) => false,
                            _ => true,
                        };
                        if !same_shell {
                            drop(cur);
                            player.set(next_play);
                        } else if playhead_changed {
                            drop(cur);
                            player.with_mut(|p| {
                                if let (Some(dst), Some(src)) =
                                    (p.snapshot.state.as_mut(), next_play.snapshot.state.as_ref())
                                {
                                    dst.position_ms = src.position_ms;
                                    dst.timestamp = src.timestamp;
                                    dst.is_paused = src.is_paused;
                                    dst.is_buffering = src.is_buffering;
                                } else {
                                    p.snapshot.state = next_play.snapshot.state.clone();
                                }
                            });
                        }
                    }

                    if playing_uri.peek().as_ref() != next_uri.as_ref() {
                        playing_uri.set(next_uri);
                    }

                    if rx.changed().await.is_err() {
                        break;
                    }
                }
            }
        });
    }

    {
        let session = session.clone();
        let mut last_uri = use_signal(|| None::<String>);
        use_effect(move || {
            let uri = player()
                .snapshot
                .state
                .as_ref()
                .and_then(|s| s.track.as_ref())
                .and_then(|t| t.uri.clone());
            if uri != last_uri() {
                last_uri.set(uri.clone());
                if uri.is_some() {
                    send(&session, UiCommand::SyncLiked);
                }
            }
        });
    }

    let cat = catalog();
    let session_cmd = session.clone();

    match cat.phase {
        Phase::Starting | Phase::Error => rsx! {
            div { class: "dark h-full shell-root",
                div { class: "flex h-full flex-col items-center justify-center gap-5 px-6 text-center",
                    h1 { class: "text-[3rem] font-bold tracking-[-0.04em]", "Sloptify" }
                    p { class: "text-muted-foreground max-w-[360px] leading-relaxed", "{cat.status}" }
                    if cat.phase == Phase::Error {
                        button {
                            class: "rounded-full bg-primary px-9 py-3 text-base font-bold text-primary-foreground",
                            onclick: move |_| send(&session_cmd, UiCommand::ContinueAfterLogin),
                            "Try again"
                        }
                    }
                }
            }
        },
        Phase::NeedsLogin => rsx! {
            div { class: "dark h-full shell-root",
                div { class: "flex h-full flex-col items-center justify-center gap-5 px-6 text-center",
                    h1 { class: "text-[3rem] font-bold tracking-[-0.04em]", "Sloptify" }
                    p { class: "text-muted-foreground max-w-[360px] leading-relaxed", "{cat.status}" }
                    button {
                        class: "rounded-full bg-primary px-9 py-3.5 text-base font-bold text-primary-foreground",
                        onclick: move |_| send(&session_cmd, UiCommand::ShowLogin),
                        "Log in with Spotify"
                    }
                    p { class: "text-muted-foreground text-[0.85rem]",
                        "One-time sign-in. The Web Player stays hidden after that."
                    }
                }
            }
        },
        Phase::Ready => rsx! {
            ReadyShell {
                catalog,
                player,
                nav,
                chip,
                search,
                sheet,
                seek_drag,
                vol_drag,
                ctx_menu,
                stack,
                plugins,
                playing_uri,
            }
        },
    }
}

#[component]
fn ReadyShell(
    catalog: Signal<CatalogSlice>,
    player: Signal<PlayerSlice>,
    mut nav: Signal<Nav>,
    chip: Signal<usize>,
    search: Signal<String>,
    mut sheet: Signal<Sheet>,
    seek_drag: Signal<Option<i64>>,
    vol_drag: Signal<Option<f64>>,
    ctx_menu: Signal<Option<CtxMenu>>,
    mut stack: Signal<NavStack>,
    plugins: Signal<PluginsState>,
    playing_uri: Signal<Option<String>>,
) -> Element {
    let session = use_context::<SessionHandle>();
    let mut tick = use_signal(|| 0u64);
    let mut sec_playlists = use_signal(|| true);
    let mut sec_artists = use_signal(|| true);
    let mut sec_albums = use_signal(|| true);
    let mut rail_w = use_signal(|| 220.0_f64);
    let mut rail_drag = use_signal(|| false);

    // Dev/screenshot helper: SLOPTIFY_SHOT_NP=1 opens fullscreen now-playing once.
    let mut shot_np_done = use_signal(|| false);
    use_effect(move || {
        if shot_np_done() {
            return;
        }
        if std::env::var_os("SLOPTIFY_SHOT_NP").is_some() {
            shot_np_done.set(true);
            sheet.set(Sheet::NowPlaying);
        }
    });

    use_future(move || async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let paused = player
                .peek()
                .snapshot
                .state
                .as_ref()
                .map(|s| s.is_paused)
                .unwrap_or(true);
            if !paused {
                tick.set(tick() + 1);
            }
        }
    });

    let cat = catalog();
    let dense = plugins().has_theme_dense();
    let open_ctx = cat
        .context
        .as_ref()
        .map(|c| c.uri.clone())
        .unwrap_or_default();
    let in_context = cat.context.is_some();
    let w = rail_w();
    let shell_class = {
        let mut c = String::from("sloptify-shell");
        if dense {
            c.push_str(" dense-lists");
        }
        if rail_drag() {
            c.push_str(" rail-dragging");
        }
        if w < 160.0 {
            c.push_str(" rail-compact");
        }
        c
    };
    let shell_style = format!("--rail-w: {w}px");

    rsx! {
        div { class: "dark h-full shell-root bg-background text-foreground",
            div {
                class: "{shell_class}",
                style: "{shell_style}",
                onmousemove: move |e: Event<MouseData>| {
                    if rail_drag() {
                        let x = e.client_coordinates().x;
                        rail_w.set(x.clamp(72.0, 420.0));
                    }
                },
                onmouseup: move |_| rail_drag.set(false),
                onmouseleave: move |_| rail_drag.set(false),

                LibraryRail {
                    library: cat.library.clone(),
                    nav,
                    sec_playlists,
                    sec_artists,
                    sec_albums,
                    open_ctx,
                    in_context,
                }

                div {
                    class: "rail-splitter",
                    onmousedown: move |e: Event<MouseData>| {
                        e.prevent_default();
                        rail_drag.set(true);
                    },
                }

                main {
                    class: if in_context {
                        "main main-locked sloptify-scroll"
                    } else {
                        "main sloptify-scroll overflow-auto"
                    },
                    div { class: "main-top",
                        ProfileDisc {
                            name: if cat.profile.name.is_empty() {
                                "Account".to_string()
                            } else {
                                cat.profile.name.clone()
                            },
                            image: cat.profile.image.clone(),
                            onclick: move |e: Event<MouseData>| {
                                e.prevent_default();
                                e.stop_propagation();
                                // Toggle; profile sits above the account scrim (z-index).
                                if sheet() == Sheet::Account {
                                    sheet.set(Sheet::None);
                                } else {
                                    sheet.set(Sheet::Account);
                                }
                            },
                        }
                    }
                    if let Some(page) = cat.context.clone() {
                        // Own scrollport — hero stays put (no blurred-layer scroll jank).
                        div { class: "main-body ctx",
                            ContextView { page, playing_uri: playing_uri() }
                        }
                    } else {
                        div { class: "main-body browse space-y-[1.4rem] px-7 pt-3 pb-5",
                            match nav() {
                                Nav::Home => rsx! { HomeView { home: cat.home.clone(), chip } },
                                Nav::Search => rsx! { SearchView { results: cat.search_results.clone(), search } },
                                Nav::Library => rsx! { LibraryView { items: cat.library.clone() } },
                            }
                        }
                    }
                }

                PlayerBar {
                    player,
                    seek_drag,
                    vol_drag,
                    sheet,
                    tick,
                }

                nav { class: "tabs grid grid-cols-3 border-t border-[var(--rule)] bg-black",
                    button {
                        class: tab_class(nav() == Nav::Home),
                        onclick: move |_| nav.set(Nav::Home),
                        House { class: "size-5" }
                        "Home"
                    }
                    button {
                        class: tab_class(nav() == Nav::Search),
                        onclick: move |_| nav.set(Nav::Search),
                        Search { class: "size-5" }
                        "Search"
                    }
                    button {
                        class: tab_class(nav() == Nav::Library),
                        onclick: {
                            let session = session.clone();
                            move |_| {
                                nav.set(Nav::Library);
                                send(&session, UiCommand::LoadLibrary { filter: String::new() });
                            }
                        },
                        Library { class: "size-5" }
                        "Library"
                    }
                }
            }

            PluginHost { player, plugins, sheet }

            match sheet() {
                Sheet::None => rsx! {},
                Sheet::NowPlaying => rsx! {
                    NowPlayingSheet {
                        player,
                        seek_drag,
                        vol_drag,
                        tick,
                        sheet,
                        stack,
                        nav,
                        context: cat.context.clone(),
                    }
                },
                Sheet::Queue => rsx! { QueueSheet { player, sheet } },
                Sheet::Account => rsx! { AccountSheet { profile: cat.profile.clone(), sheet } },
                Sheet::Plugins => rsx! { PluginsSheet { plugins, sheet } },
            }

            ContextMenuOverlay { menu: ctx_menu }
        }
    }
}
