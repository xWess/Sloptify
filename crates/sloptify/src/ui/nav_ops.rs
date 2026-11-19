//! Navigation history helpers.

use dioxus::prelude::*;
use sloptify_spotify::{is_browsable_uri, ContextPage, HomeItem};

use super::helpers::send;
use super::types::{MenuAction, Nav};
use crate::nav::{current_entry, HistEntry, NavStack, RootTab};
use crate::session::{SessionHandle, UiCommand};

pub fn tab_of(nav: Nav) -> RootTab {
    match nav {
        Nav::Home => RootTab::Home,
        Nav::Search => RootTab::Search,
        Nav::Library => RootTab::Library,
    }
}

pub fn go_nav(session: &SessionHandle, nav: &mut Signal<Nav>, next: Nav) {
    send(session, UiCommand::CloseContext);
    nav.set(next);
    if next == Nav::Library {
        send(
            session,
            UiCommand::LoadLibrary {
                filter: String::new(),
            },
        );
    }
}

pub fn open_context_nav(
    session: &SessionHandle,
    nav: Nav,
    stack: &mut Signal<NavStack>,
    context: Option<&ContextPage>,
    uri: String,
    title: Option<String>,
    image: Option<String>,
) {
    let mut s = stack();
    s.push(current_entry(tab_of(nav), context));
    stack.set(s);
    send(
        session,
        UiCommand::OpenContext {
            uri,
            title,
            image,
        },
    );
}

pub fn navigate_back(
    session: &SessionHandle,
    nav: &mut Signal<Nav>,
    stack: &mut Signal<NavStack>,
    context: Option<&ContextPage>,
) {
    let mut s = stack();
    let Some(prev) = s.back.pop() else {
        return;
    };
    if let Some(cur) = context {
        s.forward.push(HistEntry::Context {
            uri: cur.uri.clone(),
            title: Some(cur.title.clone()).filter(|t| !t.is_empty()),
            image: cur.image.clone(),
        });
    } else {
        s.forward.push(HistEntry::Root(tab_of(nav())));
    }
    stack.set(s);
    match prev {
        HistEntry::Root(tab) => {
            send(session, UiCommand::CloseContext);
            nav.set(match tab {
                RootTab::Home => Nav::Home,
                RootTab::Search => Nav::Search,
                RootTab::Library => Nav::Library,
            });
        }
        HistEntry::Context { uri, title, image } => {
            send(
                session,
                UiCommand::OpenContext { uri, title, image },
            );
        }
    }
}

pub fn navigate_forward(
    session: &SessionHandle,
    nav: &mut Signal<Nav>,
    stack: &mut Signal<NavStack>,
    context: Option<&ContextPage>,
) {
    let mut s = stack();
    let Some(next) = s.forward.pop() else {
        return;
    };
    if let Some(cur) = context {
        s.back.push(HistEntry::Context {
            uri: cur.uri.clone(),
            title: Some(cur.title.clone()).filter(|t| !t.is_empty()),
            image: cur.image.clone(),
        });
    } else {
        s.back.push(HistEntry::Root(tab_of(nav())));
    }
    stack.set(s);
    match next {
        HistEntry::Root(tab) => {
            send(session, UiCommand::CloseContext);
            nav.set(match tab {
                RootTab::Home => Nav::Home,
                RootTab::Search => Nav::Search,
                RootTab::Library => Nav::Library,
            });
        }
        HistEntry::Context { uri, title, image } => {
            send(
                session,
                UiCommand::OpenContext { uri, title, image },
            );
        }
    }
}

pub fn item_activate(
    session: &SessionHandle,
    nav: Nav,
    stack: &mut Signal<NavStack>,
    context: Option<&ContextPage>,
    item: &HomeItem,
) {
    if is_browsable_uri(&item.uri) {
        open_context_nav(
            session,
            nav,
            stack,
            context,
            item.uri.clone(),
            Some(item.name.clone()).filter(|s| !s.is_empty() && !s.starts_with("spotify:")),
            item.image.clone(),
        );
    } else {
        send(
            session,
            UiCommand::PlayUri {
                uri: item.uri.clone(),
            },
        );
    }
}

pub fn run_menu_action(
    session: &SessionHandle,
    nav: Nav,
    stack: &mut Signal<NavStack>,
    context: Option<&ContextPage>,
    action: MenuAction,
) {
    use super::types::MenuAction::*;
    match action {
        Play { uri } => send(session, UiCommand::PlayUri { uri }),
        PlayNext { uri } => send(session, UiCommand::QueuePlayNext { uri }),
        QueueAdd { uri } => send(session, UiCommand::QueueAdd { uri }),
        Open { uri, title, image } => {
            open_context_nav(session, nav, stack, context, uri, title, image)
        }
        ToggleLike { uri } => send(session, UiCommand::ToggleLikeUri { uri }),
        SongRadio { uri, title } => send(session, UiCommand::SongRadio { uri, title }),
        CopyLink { uri } => send(session, UiCommand::CopyLink { uri }),
        Sep => {}
    }
}
