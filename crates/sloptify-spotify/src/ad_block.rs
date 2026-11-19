//! Orpheon `AdBlock` — refuse ad / tracker hosts while the blocker is active.
//!
//! Upstream (Android): only while music plays on a Spotify Connect device the app
//! sent to. Sloptify's Chromium *is* the playback engine, so we keep the same
//! host list and CDP Fetch fail path, and activate for free local playback
//! (Premium has no stream ads; blocking is a no-op).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

const LIST: &str = include_str!("../assets/page-filter.txt");

static ACTIVE: AtomicBool = AtomicBool::new(false);
static RULES: OnceLock<Rules> = OnceLock::new();

#[derive(Debug, Clone)]
struct Rule {
    host: String,
    path: Option<String>,
}

impl Rule {
    fn matches(&self, url_host: &str, url_path: &str) -> bool {
        let host_ok = url_host == self.host || url_host.ends_with(&format!(".{}", self.host));
        if !host_ok {
            return false;
        }
        match &self.path {
            None => true,
            Some(p) => url_path.starts_with(p),
        }
    }
}

#[derive(Debug, Default)]
struct Rules {
    ads: Vec<Rule>,
    trackers: Vec<Rule>,
}

fn parse(text: &str) -> Rules {
    let mut ads = Vec::new();
    let mut trackers = Vec::new();
    let mut into: Option<&mut Vec<Rule>> = None;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            into = match line {
                "[ads]" => Some(&mut ads),
                "[trackers]" => Some(&mut trackers),
                _ => None,
            };
            continue;
        }
        let (host, path) = match line.find('/') {
            Some(i) => (line[..i].to_ascii_lowercase(), Some(line[i..].to_string())),
            None => (line.to_ascii_lowercase(), None),
        };
        if let Some(bucket) = into.as_mut() {
            bucket.push(Rule { host, path });
        }
    }
    Rules { ads, trackers }
}

fn rules() -> &'static Rules {
    RULES.get_or_init(|| parse(LIST))
}

/// Distinct hosts for CDP `Fetch.enable` url patterns (Orpheon `AdBlock.hosts`).
pub fn hosts() -> Vec<&'static str> {
    let r = rules();
    let mut out: Vec<&str> = r
        .ads
        .iter()
        .chain(r.trackers.iter())
        .map(|rule| rule.host.as_str())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// CDP `Fetch.enable` url patterns for Request stage.
///
/// Path rules emit `*://host/path*` and `*://*.host/path*` so `scdn.co/mp3-ad/` pauses
/// `https://audio-ak.scdn.co/mp3-ad/…` without intercepting every `*.scdn.co` asset.
/// Host-only rules get `*://host/*` and `*://*.host/*`.
pub fn fetch_url_patterns() -> Vec<String> {
    let r = rules();
    let mut out = Vec::new();
    for rule in r.ads.iter().chain(r.trackers.iter()) {
        let h = &rule.host;
        match &rule.path {
            Some(path) => {
                out.push(format!("*://{h}{path}*"));
                out.push(format!("*://*.{h}{path}*"));
            }
            None => {
                out.push(format!("*://{h}/*"));
                out.push(format!("*://*.{h}/*"));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Orpheon `AdBlock.setActive` — never on Premium (`premium = true` forces off).
pub fn set_active(on: bool) {
    let next = on; // Premium gating can be wired when AccountPlan exists.
    ACTIVE.store(next, Ordering::Relaxed);
    tracing::info!(active = next, "AdBlock {}", if next { "on" } else { "off" });
}

/// Orpheon `AdBlock.isBlocked` — only while [active].
pub fn is_blocked(url: &str) -> bool {
    is_blocked_with(url, active())
}

/// Ad media (MP3/CDN) — fulfill with silence so the page can leave the break.
/// Trackers stay hard-failed.
pub fn is_ad_audio(url: &str) -> bool {
    if !active() {
        return false;
    }
    let rest = url.split("://").nth(1).unwrap_or(url);
    let (authority_raw, path_raw) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let host = authority_raw
        .rsplit('@')
        .next()
        .unwrap_or(authority_raw)
        .split(':')
        .next()
        .unwrap_or(authority_raw)
        .to_ascii_lowercase();
    let path = path_raw
        .split(['?', '#'])
        .next()
        .filter(|p| !p.is_empty())
        .unwrap_or("/");
    rules()
        .ads
        .iter()
        .any(|rule| rule.matches(&host, path))
}

pub fn is_blocked_with(url: &str, blocking: bool) -> bool {
    if !blocking {
        return false;
    }
    let rest = url.split("://").nth(1).unwrap_or(url);
    let (authority_raw, path_raw) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let host = authority_raw
        .rsplit('@')
        .next()
        .unwrap_or(authority_raw)
        .split(':')
        .next()
        .unwrap_or(authority_raw)
        .to_ascii_lowercase();
    let path = path_raw
        .split(['?', '#'])
        .next()
        .filter(|p| !p.is_empty())
        .unwrap_or("/");
    let r = rules();
    r.ads
        .iter()
        .chain(r.trackers.iter())
        .any(|rule| rule.matches(&host, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_adstudio_when_active() {
        assert!(is_blocked_with(
            "https://adstudio-assets.scdn.co/mp3/abc.mp3",
            true
        ));
        assert!(!is_blocked_with(
            "https://adstudio-assets.scdn.co/mp3/abc.mp3",
            false
        ));
        set_active(true);
        assert!(is_ad_audio("https://adstudio-assets.scdn.co/mp3/abc.mp3"));
        assert!(!is_ad_audio("https://www.googletagmanager.com/gtm.js"));
        set_active(false);
    }

    #[test]
    fn leaves_music_cdn() {
        assert!(!is_blocked_with(
            "https://audio-ak-spotify-com.akamaized.net/audio/x",
            true
        ));
    }

    #[test]
    fn path_prefix_rule() {
        assert!(is_blocked_with("https://scdn.co/mp3-ad/foo.mp3", true));
        assert!(!is_blocked_with("https://scdn.co/other/foo.mp3", true));
        // Real ad CDN host is a subdomain — isBlocked already matched; Fetch patterns must too.
        assert!(is_blocked_with("https://audio-ak.scdn.co/mp3-ad/abc", true));
        assert!(!is_blocked_with("https://audio-ak.scdn.co/mp3/abc", true));
    }

    #[test]
    fn fetch_patterns_cover_scdn_subdomains() {
        let p = fetch_url_patterns();
        assert!(p.iter().any(|x| x == "*://scdn.co/mp3-ad/*"));
        assert!(p.iter().any(|x| x == "*://*.scdn.co/mp3-ad/*"));
        assert!(p.iter().any(|x| x == "*://adstudio-assets.scdn.co/*"));
        assert!(p.iter().any(|x| x == "*://*.adstudio-assets.scdn.co/*"));
        // Must not blanket-pause every scdn.co asset (covers, music).
        assert!(!p.iter().any(|x| x == "*://*.scdn.co/*"));
    }
}
