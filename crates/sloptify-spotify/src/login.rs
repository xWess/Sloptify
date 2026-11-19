//! Login URL rules from Sloptify Android `LoginLoad` (GPL-3.0 origin).

pub fn is_login_url(url: &str) -> bool {
    let base = url.split(['#', '?']).next().unwrap_or(url);
    let Ok(parsed) = url::Url::parse(base) else {
        return false;
    };
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    if host == "accounts.spotify.com" || host == "challenge.spotify.com" {
        return true;
    }
    if parsed.path().to_ascii_lowercase().contains("/login") {
        return true;
    }
    host == "accounts.google.com"
        || host == "facebook.com"
        || host.ends_with(".facebook.com")
        || host == "appleid.apple.com"
}
