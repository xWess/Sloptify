//! Login URL rules for the Spotify accounts flow.

pub fn is_login_url(url: &str) -> bool {
    let base = url.split(['#', '?']).next().unwrap_or(url);
    let Ok(parsed) = url::Url::parse(base) else {
        return false;
    };
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let path = parsed.path().to_ascii_lowercase();
    // Post-auth "status" page is not a login form — treat separately.
    if host == "accounts.spotify.com" && is_accounts_post_auth_path(&path) {
        return false;
    }
    if host == "accounts.spotify.com" || host == "challenge.spotify.com" {
        return true;
    }
    if path.contains("/login") {
        return true;
    }
    host == "accounts.google.com"
        || host == "facebook.com"
        || host.ends_with(".facebook.com")
        || host == "appleid.apple.com"
}

/// Spotify finishes OAuth on accounts.spotify.com/.../status (and similar),
/// then the desktop --app window often sits there instead of opening the player.
pub fn is_accounts_post_auth_url(url: &str) -> bool {
    let base = url.split(['#', '?']).next().unwrap_or(url);
    let Ok(parsed) = url::Url::parse(base) else {
        return false;
    };
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    if host != "accounts.spotify.com" {
        return false;
    }
    is_accounts_post_auth_path(&parsed.path().to_ascii_lowercase())
}

fn is_accounts_post_auth_path(path: &str) -> bool {
    path.contains("/status")
        || path.contains("/login/ok")
        || path.contains("/continue")
        || path.contains("/auth-finish")
}

pub fn is_web_player_url(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.starts_with("https://open.spotify.com") && !is_login_url(&u)
}
