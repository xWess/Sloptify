//! In-app navigation history (back / forward) for catalog + context pages.

use sloptify_spotify::ContextPage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootTab {
    Home,
    Search,
    Library,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HistEntry {
    Root(RootTab),
    Context {
        uri: String,
        title: Option<String>,
        image: Option<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NavStack {
    pub back: Vec<HistEntry>,
    pub forward: Vec<HistEntry>,
}

impl NavStack {
    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn clear_forward(&mut self) {
        self.forward.clear();
    }

    pub fn push(&mut self, entry: HistEntry) {
        // Avoid stacking identical consecutive contexts.
        if self.back.last() == Some(&entry) {
            return;
        }
        self.back.push(entry);
        self.forward.clear();
        if self.back.len() > 64 {
            self.back.drain(0..self.back.len() - 64);
        }
    }
}

pub fn current_entry(tab: RootTab, context: Option<&ContextPage>) -> HistEntry {
    if let Some(c) = context {
        HistEntry::Context {
            uri: c.uri.clone(),
            title: Some(c.title.clone()).filter(|s| !s.is_empty()),
            image: c.image.clone(),
        }
    } else {
        HistEntry::Root(tab)
    }
}

pub fn root_from_nav(nav_home: bool, nav_search: bool, nav_library: bool) -> RootTab {
    if nav_search {
        RootTab::Search
    } else if nav_library {
        RootTab::Library
    } else if nav_home {
        RootTab::Home
    } else {
        RootTab::Home
    }
}
