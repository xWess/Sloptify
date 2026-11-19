//! Overlay sheets + plugin host + context menu.

use dioxus::prelude::*;
use icons::{House, Library, Search};
use sloptify_spotify::{TrackRef, UserProfile};

use super::helpers::{artists_of, send};
use super::nav_ops::run_menu_action;
use super::types::{CatalogSlice, CtxMenu, MenuAction, Nav, PlayerSlice, Sheet};
use super::widgets::Cover;
use crate::nav::NavStack;
use crate::plugins::{notify_track_changed, PluginsState};
use crate::session::{SessionHandle, UiCommand};

#[component]
pub fn QueueSheet(player: Signal<PlayerSlice>, mut sheet: Signal<Sheet>) -> Element {
    let session = use_context::<SessionHandle>();
    // Ensure queue is fresh whenever this panel mounts.
    use_effect(move || {
        send(&session, UiCommand::RefreshQueue);
    });
    let queue = player().snapshot.queue.clone();
    let next_up = queue.as_ref().map(|q| q.next_up.clone()).unwrap_or_default();
    let queued = queue.as_ref().map(|q| q.queued.clone()).unwrap_or_default();
    let current = queue.as_ref().and_then(|q| q.current.clone());
    let empty = queued.is_empty() && next_up.is_empty() && current.is_none();

    rsx! {
        // Absolute (not fixed) — WebKitGTK parks `fixed` overlays off to the left.
        div { class: "queue-layer",
            div {
                class: "queue-backdrop",
                onclick: move |_| sheet.set(Sheet::None),
            }
            aside { class: "queue-drawer",
                div { class: "queue-head",
                    h2 { class: "queue-title", "Queue" }
                    button {
                        class: "queue-close",
                        onclick: move |_| sheet.set(Sheet::None),
                        "Close"
                    }
                }
                div { class: "queue-body sloptify-scroll",
                    if let Some(cur) = current {
                        p { class: "queue-sec", "Now playing" }
                        QueueRow { track: cur, playable: false, current: true }
                    }
                    if !queued.is_empty() {
                        p { class: "queue-sec", "Next in queue" }
                        for (i, t) in queued.into_iter().enumerate() {
                            QueueRow { key: "q{i}", track: t, playable: true, current: false }
                        }
                    }
                    if !next_up.is_empty() {
                        p { class: "queue-sec", "Next up" }
                        for (i, t) in next_up.into_iter().enumerate() {
                            QueueRow { key: "n{i}", track: t, playable: true, current: false }
                        }
                    }
                    if empty {
                        p { class: "queue-empty", "Queue is empty — play something from Home." }
                    }
                }
            }
        }
    }
}

#[component]
fn QueueRow(track: TrackRef, playable: bool, current: bool) -> Element {
    let session = use_context::<SessionHandle>();
    let name = track.name.clone().unwrap_or_default();
    let artist = artists_of(&track);
    let image = track.image.clone();
    let uri = track.uri.clone().unwrap_or_default();
    let row_class = if current { "queue-row on" } else { "queue-row" };

    rsx! {
        button {
            class: "{row_class}",
            disabled: !playable || uri.is_empty(),
            onclick: {
                let session = session.clone();
                let uri = uri.clone();
                move |_| {
                    if !uri.is_empty() {
                        send(&session, UiCommand::PlayUri { uri: uri.clone() });
                    }
                }
            },
            Cover { url: image, class: "queue-cover cover-soft".to_string() }
            div { class: "queue-meta",
                p { class: "queue-name", "{name}" }
                p { class: "queue-artist", "{artist}" }
            }
        }
    }
}

#[component]
pub fn AccountSheet(profile: UserProfile, mut sheet: Signal<Sheet>) -> Element {
    let session = use_context::<SessionHandle>();
    let name = if profile.name.is_empty() {
        "Signed in".to_string()
    } else {
        profile.name.clone()
    };
    let user = profile.username.clone();

    rsx! {
        // Single root so the menu actually mounts in desktop WebView.
        div { class: "account-layer",
            div {
                class: "account-scrim",
                onclick: move |_| sheet.set(Sheet::None),
            }
            div {
                class: "account-menu",
                role: "menu",
                onclick: move |e: Event<MouseData>| e.stop_propagation(),
                div { class: "account-menu-head",
                    Cover {
                        url: profile.image.clone(),
                        class: "size-10 shrink-0 cover-artist".to_string()
                    }
                    div { class: "min-w-0",
                        p { class: "truncate text-sm font-semibold", "{name}" }
                        if !user.is_empty() {
                            p { class: "text-muted-foreground truncate text-xs", "@{user}" }
                        }
                    }
                }
                button {
                    class: "account-menu-row",
                    role: "menuitem",
                    onclick: move |_| sheet.set(Sheet::Plugins),
                    span { "Plugins" }
                    span { class: "text-muted-foreground text-xs", "Marketplace" }
                }
                button {
                    class: "account-menu-row danger",
                    role: "menuitem",
                    onclick: {
                        let session = session.clone();
                        move |_| {
                            send(&session, UiCommand::SignOut);
                            sheet.set(Sheet::None);
                        }
                    },
                    "Sign out"
                }
            }
        }
    }
}

#[component]
pub fn PluginsSheet(mut plugins: Signal<PluginsState>, mut sheet: Signal<Sheet>) -> Element {
    let mut url_input = use_signal(String::new);

    use_future(move || async move {
        let next = PluginsState::refresh_remote().await;
        plugins.set(next);
    });

    let state = plugins();
    let busy = state.busy;
    let entries: Vec<_> = state
        .entries
        .iter()
        .filter(|e| {
            let id = e.listing.id.as_str();
            !(id.contains("lossless") || id.contains("flac") || id.starts_with("audio."))
        })
        .cloned()
        .collect();

    rsx! {
        div { class: "plugin-layer",
            div {
                class: "plugin-backdrop",
                onclick: move |_| sheet.set(Sheet::None),
            }
            aside { class: "plugin-drawer",
                div { class: "flex items-start justify-between gap-3 border-b border-[var(--rule)] px-5 py-4",
                    div { class: "min-w-0",
                        h2 { class: "text-lg font-bold", "Plugins" }
                        p { class: "text-muted-foreground mt-1 text-xs",
                            "Community catalog + zip installs. Enable to activate host effects."
                        }
                        if !state.status.is_empty() {
                            p { class: "mt-2 text-xs font-medium text-[var(--accent)]", "{state.status}" }
                        }
                    }
                    button {
                        class: "shrink-0 rounded-full px-3 py-1.5 text-sm font-semibold text-muted-foreground hover:bg-[var(--raised)] hover:text-foreground",
                        onclick: move |_| sheet.set(Sheet::None),
                        "Close"
                    }
                }
                div { class: "plugin-url-row",
                    input {
                        r#type: "url",
                        placeholder: "Install from zip URL…",
                        value: "{url_input()}",
                        disabled: busy,
                        oninput: move |e| url_input.set(e.value()),
                        onkeydown: move |e: Event<KeyboardData>| {
                            if e.key() == Key::Enter && !busy {
                                let url = url_input();
                                spawn(async move {
                                    let mut next = plugins();
                                    next.busy = true;
                                    next.status = "Downloading…".into();
                                    plugins.set(next);
                                    let done = PluginsState::install_url(&url).await;
                                    plugins.set(done);
                                });
                            }
                        },
                    }
                    button {
                        disabled: busy || url_input().trim().is_empty(),
                        onclick: move |_| {
                            let url = url_input();
                            spawn(async move {
                                let mut next = plugins();
                                next.busy = true;
                                next.status = "Downloading…".into();
                                plugins.set(next);
                                let done = PluginsState::install_url(&url).await;
                                plugins.set(done);
                            });
                        },
                        "Install URL"
                    }
                }
                div { class: "sloptify-scroll flex-1 space-y-3 overflow-auto px-4 py-4",
                    if entries.is_empty() {
                        p { class: "text-muted-foreground px-1 py-8 text-center text-sm",
                            "No plugins in the catalog yet."
                        }
                    }
                    for entry in entries {
                        {
                            let id = entry.listing.id.clone();
                            let name = entry.listing.name.clone();
                            let desc = entry.listing.description.clone();
                            let author = entry.listing.author.clone();
                            let kind = entry.listing.kind.label();
                            let installed = entry.installed;
                            let enabled = entry.enabled;
                            let remote = entry
                                .listing
                                .download_url
                                .as_ref()
                                .is_some_and(|u| !u.is_empty());
                            rsx! {
                                article { class: "plugin-card",
                                    key: "{id}",
                                    div { class: "plugin-meta",
                                        span { class: "plugin-chip", "{kind}" }
                                        if !author.is_empty() {
                                            span { class: "text-muted-foreground text-[0.72rem]", "{author}" }
                                        }
                                        if remote {
                                            span { class: "text-muted-foreground text-[0.72rem]", "zip" }
                                        }
                                        if installed {
                                            span { class: "text-muted-foreground text-[0.72rem]",
                                                if enabled { "Enabled" } else { "Installed" }
                                            }
                                        }
                                    }
                                    h3 { "{name}" }
                                    p { class: "text-muted-foreground m-0 text-[0.82rem] leading-snug", "{desc}" }
                                    div { class: "plugin-actions",
                                        if installed {
                                            button {
                                                class: if enabled { "primary" } else { "" },
                                                disabled: busy,
                                                onclick: {
                                                    let id = id.clone();
                                                    move |_| {
                                                        plugins.set(PluginsState::set_enabled(&id, !enabled));
                                                    }
                                                },
                                                if enabled { "Disable" } else { "Enable" }
                                            }
                                            button {
                                                class: "danger",
                                                disabled: busy,
                                                onclick: {
                                                    let id = id.clone();
                                                    move |_| {
                                                        plugins.set(PluginsState::uninstall(&id));
                                                    }
                                                },
                                                "Remove"
                                            }
                                        } else {
                                            button {
                                                class: "primary",
                                                disabled: busy,
                                                onclick: {
                                                    let id = id.clone();
                                                    move |_| {
                                                        let id = id.clone();
                                                        spawn(async move {
                                                            let mut next = plugins();
                                                            next.busy = true;
                                                            next.status = "Installing…".into();
                                                            plugins.set(next);
                                                            let done = PluginsState::install(&id).await;
                                                            plugins.set(done);
                                                        });
                                                    }
                                                },
                                                "Install"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "plugin-contrib",
                    p { class: "m-0 text-[0.8rem] leading-snug",
                        "Publish: zip a folder with "
                        code { "manifest.json" }
                        " + "
                        code { "main.js" }
                        " (GitHub Release works), paste the zip URL here, or PR a listing into "
                        code { "plugins/catalog.json" }
                        " with "
                        code { "downloadUrl" }
                        "."
                    }
                }
            }
        }
    }
}

#[component]
pub fn PluginHost(
    player: Signal<PlayerSlice>,
    plugins: Signal<PluginsState>,
    sheet: Signal<Sheet>,
) -> Element {
    let session = use_context::<SessionHandle>();
    let mut last_notify_uri = use_signal(|| None::<String>);
    let mut last_scrobble_uri = use_signal(|| None::<String>);

    use_effect(move || {
        let plug = plugins();
        let snap = player().snapshot;
        let Some(state) = snap.state.as_ref() else { return; };
        let Some(track) = state.track.as_ref() else { return; };
        let Some(uri) = track.uri.clone().filter(|u| !u.is_empty() && !track.is_ad()) else {
            return;
        };
        let title = state.title();
        let artist = state.artist_line();

        if plug.has_desktop_notify() && last_notify_uri() != Some(uri.clone()) {
            last_notify_uri.set(Some(uri.clone()));
            notify_track_changed(&title, &artist);
        }
        if plug.has_scrobble_hook() && last_scrobble_uri() != Some(uri.clone()) {
            last_scrobble_uri.set(Some(uri));
            tracing::info!(%title, %artist, "scrobble hook (plugin host)");
        }
    });

    rsx! {
        if plugins().has_keyboard_nav() && sheet() != Sheet::NowPlaying {
            div {
                class: "plugin-keynav-trap",
                tabindex: "0",
                onkeydown: {
                    let session = session.clone();
                    move |e: KeyboardEvent| {
                        let paused = player()
                            .snapshot
                            .state
                            .as_ref()
                            .map(|s| s.is_paused)
                            .unwrap_or(true);
                        match e.key() {
                            Key::Character(ref c) if c == " " || c == "k" => {
                                e.prevent_default();
                                if *c == " " {
                                    if paused { send(&session, UiCommand::Resume); }
                                    else { send(&session, UiCommand::Pause); }
                                } else {
                                    send(&session, UiCommand::Next);
                                }
                            }
                            Key::Character(ref c) if c == "j" => {
                                e.prevent_default();
                                send(&session, UiCommand::Previous);
                            }
                            Key::ArrowRight => {
                                e.prevent_default();
                                send(&session, UiCommand::Next);
                            }
                            Key::ArrowLeft => {
                                e.prevent_default();
                                send(&session, UiCommand::Previous);
                            }
                            _ => {}
                        }
                    }
                },
            }
        }
    }
}

#[component]
pub fn ContextMenuOverlay(mut menu: Signal<Option<CtxMenu>>) -> Element {
    let session = use_context::<SessionHandle>();
    let mut stack = use_context::<Signal<NavStack>>();
    let nav = use_context::<Signal<Nav>>();
    let catalog = use_context::<Signal<CatalogSlice>>();
    let Some(m) = menu() else {
        return rsx! {};
    };
    // Keep the panel on-screen; coords are viewport/client.
    let left = m.x.clamp(8.0, 1200.0);
    let top = m.y.clamp(8.0, 900.0);
    let style = format!("left:{left}px;top:{top}px");

    rsx! {
        div {
            class: "ctx-menu-layer",
            onclick: move |_| menu.set(None),
            oncontextmenu: move |e: Event<MouseData>| {
                e.prevent_default();
                menu.set(None);
            },
            div {
                class: "ctx-menu",
                style: "{style}",
                role: "menu",
                onclick: move |e: Event<MouseData>| e.stop_propagation(),
                p { class: "ctx-menu-title", "{m.title}" }
                for (label, action) in m.actions {
                    {
                        match action {
                            MenuAction::Sep => rsx! {
                                div { class: "ctx-menu-sep" }
                            },
                            action => rsx! {
                                button {
                                    class: "ctx-menu-item",
                                    role: "menuitem",
                                    onclick: {
                                        let session = session.clone();
                                        let ctx = catalog().context.clone();
                                        move |_| {
                                            run_menu_action(
                                                &session, nav(), &mut stack, ctx.as_ref(), action.clone(),
                                            );
                                            menu.set(None);
                                        }
                                    },
                                    "{label}"
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

// silence unused icon imports if tree-shaken differently
#[allow(dead_code)]
fn _icons() {
    let _ = (House, Library, Search);
}
