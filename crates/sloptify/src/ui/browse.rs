//! Home / Search / Library views.

use dioxus::prelude::*;
use icons::Play;

use sloptify_spotify::{HomeFeed, HomeItem};

use super::helpers::{display_name, editorial_greet, humanize_shelf_title, send, trunc};
use super::nav_ops::item_activate;
use super::types::{CatalogSlice, CtxMenu, MenuAction, Nav, SHELF_CAP};
use super::widgets::Cover;
use crate::components::ui::button::Button;
use crate::components::ui::empty::{Empty, EmptyDescription, EmptyHeader, EmptyTitle};
use crate::components::ui::input::{Input, InputType};
use crate::nav::NavStack;
use crate::session::{SessionHandle, UiCommand};
use dioxus::prelude::Key;

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
pub fn HomeView(home: HomeFeed, mut chip: Signal<usize>) -> Element {
    let session = use_context::<SessionHandle>();
    let mut stack = use_context::<Signal<NavStack>>();
    let nav = use_context::<Signal<Nav>>();
    let catalog = use_context::<Signal<CatalogSlice>>();
    let mut ctx_menu = use_context::<Signal<Option<CtxMenu>>>();
    let greet = editorial_greet(&home.greeting);
    let chips: Vec<(String, String)> = if home.chips.is_empty() {
        vec![("".to_string(), "All".to_string())]
    } else {
        home.chips
            .iter()
            .map(|c| (c.id.clone(), c.label.clone()))
            .collect()
    };
    let shelves = home.shelves.clone();
    let first = shelves.first().cloned();
    let rest: Vec<_> = shelves.iter().skip(1).cloned().collect();
    let empty = shelves.is_empty();
    let featured = first
        .as_ref()
        .and_then(|s| s.items.first().cloned());
    let spun: Vec<HomeItem> = first
        .as_ref()
        .map(|s| s.items.iter().skip(1).cloned().collect())
        .unwrap_or_default();

    rsx! {
        div { class: "home-screen",
            p { class: "home-kicker", "SLOPTIFY" }
            h1 { class: "home-greet", "{greet}" }
            p { class: "home-sub", "Pick up where the tape left off" }

            if chips.len() > 1 {
                div { class: "home-facets",
                    for (i, (id, label)) in chips.into_iter().enumerate() {
                        {
                            let session = session.clone();
                            let id = id.clone();
                            let on = chip() == i;
                            rsx! {
                                button {
                                    key: "{i}",
                                    class: if on { "home-facet on" } else { "home-facet" },
                                    onclick: move |_| {
                                        chip.set(i);
                                        send(&session, UiCommand::RefreshHome { facet: id.clone() });
                                    },
                                    "{label}"
                                }
                            }
                        }
                    }
                }
            }

            if empty {
                Empty { class: "mt-10 border-border",
                    EmptyHeader {
                        EmptyTitle { "Loading your home…" }
                        EmptyDescription { "Fetching shelves from Spotify." }
                    }
                }
            } else {
                if let Some(item) = featured {
                    {
                        let session = session.clone();
                        let item_c = item.clone();
                        let uri = item.uri.clone();
                        let name = trunc(&item.name, 40);
                        let sub = trunc(&item.subtitle, 48);
                        let image = item.image.clone();
                        rsx! {
                            button {
                                key: "feat-{uri}",
                                class: "home-continue",
                                onclick: {
                                    let session = session.clone();
                                    let item = item_c.clone();
                                    let ctx = catalog().context.clone();
                                    move |_| item_activate(&session, nav(), &mut stack, ctx.as_ref(), &item)
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
                                Cover { url: image, class: "home-continue-cover".to_string() }
                                div { class: "home-continue-meta",
                                    p { class: "home-continue-kicker", "CONTINUE LISTENING" }
                                    p { class: "home-continue-title", "{name}" }
                                    if !sub.is_empty() {
                                        p { class: "home-continue-sub", "{sub}" }
                                    }
                                }
                                span { class: "home-continue-play",
                                    Play { class: "size-5 fill-current" }
                                }
                            }
                        }
                    }
                }

                for shelf in rest {
                    {
                        let title = humanize_shelf_title(&shelf.title);
                        rsx! {
                            section { class: "home-shelf",
                                key: "{title}",
                                h2 { class: "home-section-label", "{title}" }
                                div { class: "shelf-row",
                                    for item in shelf.items.into_iter().take(SHELF_CAP) {
                                        {
                                            let session = session.clone();
                                            let item_c = item.clone();
                                            let uri = item.uri.clone();
                                            let name = trunc(&item.name, 22);
                                            let image = item.image.clone();
                                            rsx! {
                                                button {
                                                    key: "{uri}",
                                                    class: "home-sleeve",
                                                    onclick: {
                                                        let session = session.clone();
                                                        let item = item_c.clone();
                                                        let ctx = catalog().context.clone();
                                                        move |_| item_activate(&session, nav(), &mut stack, ctx.as_ref(), &item)
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
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if !spun.is_empty() {
                    section { class: "home-spun",
                        h2 { class: "home-section-label", "SPUN LATELY" }
                        div { class: "home-spun-row",
                            for item in spun.into_iter().take(10) {
                                {
                                    let session = session.clone();
                                    let item_c = item.clone();
                                    let uri = item.uri.clone();
                                    let name = trunc(&item.name, 20);
                                    let sub = trunc(&item.subtitle, 22);
                                    let image = item.image.clone();
                                    rsx! {
                                        button {
                                            key: "spun-{uri}",
                                            class: "home-spun-chip",
                                            onclick: {
                                                let session = session.clone();
                                                let item = item_c.clone();
                                                let ctx = catalog().context.clone();
                                                move |_| item_activate(&session, nav(), &mut stack, ctx.as_ref(), &item)
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
                                            Cover { url: image, class: "home-spun-cover".to_string() }
                                            div { class: "home-spun-meta",
                                                p { class: "home-spun-title", "{name}" }
                                                p { class: "home-spun-sub", "{sub}" }
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
pub fn SearchView(results: Vec<HomeItem>, mut search: Signal<String>) -> Element {
    let session = use_context::<SessionHandle>();
    let mut stack = use_context::<Signal<NavStack>>();
    let nav = use_context::<Signal<Nav>>();
    let catalog = use_context::<Signal<CatalogSlice>>();
    let mut ctx_menu = use_context::<Signal<Option<CtxMenu>>>();
    let session_go = session.clone();

    rsx! {
        h1 { class: "mb-4 text-[2rem] font-bold tracking-[-0.03em]", "Search" }
        div { class: "search-pill mb-[18px] flex gap-2.5",
            Input {
                class: "flex-1",
                r#type: InputType::Search,
                value: search(),
                placeholder: "What do you want to listen to?",
                oninput: move |e: FormEvent| search.set(e.value()),
                onkeydown: {
                    let session = session.clone();
                    move |e: KeyboardEvent| {
                        if e.key() == Key::Enter {
                            send(&session, UiCommand::Search { query: search() });
                        }
                    }
                },
            }
            Button {
                class: "h-12 rounded-full bg-primary px-[22px] font-bold text-primary-foreground hover:bg-primary/90",
                onclick: move |_| send(&session_go, UiCommand::Search { query: search() }),
                "Go"
            }
        }
        div { class: "flex flex-col gap-1.5",
            if results.is_empty() {
                p { class: "text-muted-foreground py-10", "Search albums, artists, playlists…" }
            } else {
                for item in results {
                    {
                        let session = session.clone();
                        let item_c = item.clone();
                        let uri = item.uri.clone();
                        let name = item.name.clone();
                        let sub = item.subtitle.clone();
                        let image = item.image.clone();
                        rsx! {
                            button {
                                key: "{uri}",
                                class: "flex w-full items-center gap-3 rounded-lg px-2.5 py-2 text-left hover:bg-[var(--surface)]",
                                onclick: {
                                    let session = session.clone();
                                    let item = item_c.clone();
                                    let ctx = catalog().context.clone();
                                    move |_| item_activate(&session, nav(), &mut stack, ctx.as_ref(), &item)
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
                                Cover { url: image, class: "size-12 shrink-0 rounded".to_string() }
                                div { class: "min-w-0",
                                    p { class: "truncate font-semibold", "{name}" }
                                    p { class: "text-muted-foreground mt-0.5 truncate text-[0.82rem]", "{sub}" }
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
pub fn LibraryView(items: Vec<HomeItem>) -> Element {
    let session = use_context::<SessionHandle>();
    let mut stack = use_context::<Signal<NavStack>>();
    let nav = use_context::<Signal<Nav>>();
    let catalog = use_context::<Signal<CatalogSlice>>();
    let mut ctx_menu = use_context::<Signal<Option<CtxMenu>>>();

    rsx! {
        h1 { class: "mb-4 text-[2rem] font-bold tracking-[-0.03em]", "Your Library" }
        if items.is_empty() {
            Empty { class: "mt-10 border-border",
                EmptyHeader {
                    EmptyTitle { "Library is empty" }
                    EmptyDescription { "Playlists and albums you save show up here." }
                }
            }
        } else {
            div { class: "flex flex-col gap-1",
                for item in items {
                    {
                        let item_c = item.clone();
                        let uri = item.uri.clone();
                        let name = display_name(&item.name);
                        let sub = item.subtitle.clone();
                        let image = item.image.clone();
                        rsx! {
                            button {
                                key: "{uri}",
                                class: "flex w-full items-center gap-3 rounded-lg px-2.5 py-2 text-left hover:bg-[var(--surface)]",
                                onclick: {
                                    let session = session.clone();
                                    let item = item_c.clone();
                                    let ctx = catalog().context.clone();
                                    move |_| item_activate(&session, nav(), &mut stack, ctx.as_ref(), &item)
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
                                Cover { url: image, class: "size-12 shrink-0".to_string() }
                                div { class: "min-w-0",
                                    p { class: "truncate font-semibold", "{name}" }
                                    p { class: "text-muted-foreground mt-0.5 truncate text-[0.82rem]", "{sub}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
