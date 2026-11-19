//! Session boot mirrors Sloptify Android: hidden Web Player always on, login only when needed.

use sloptify_spotify::{
    is_login_url, Bridge, BridgeSnapshot, ChromiumConfig, ChromiumSession, CdpHost, ContextPage,
    AdWatch, HomeFeed, HomeItem, Player, SpotifyCatalog, StallWatch, UserProfile, DEFAULT_CDP_URL,
};
use serde::Serialize;
use tokio::sync::{mpsc, watch};

#[derive(Debug, Clone)]
pub enum UiCommand {
    ShowLogin,
    ContinueAfterLogin,
    HidePlayer,
    RefreshHome { facet: String },
    LoadLibrary { filter: String },
    PlayUri { uri: String },
    PlayInContext {
        context_uri: String,
        track_uri: String,
        track_uid: Option<String>,
        index: Option<i64>,
    },
    OpenContext {
        uri: String,
        title: Option<String>,
        image: Option<String>,
    },
    LoadMoreContext,
    CloseContext,
    QueueAdd { uri: String },
    QueuePlayNext { uri: String },
    QueueSkip {
        uri: String,
        uid: Option<String>,
    },
    ToggleLikeUri { uri: String },
    CopyLink { uri: String },
    SongRadio {
        uri: String,
        title: Option<String>,
    },
    SignOut,
    Search { query: String },
    Pause,
    Resume,
    Next,
    Previous,
    Seek { position_ms: i64 },
    SetVolume { level: f64 },
    ToggleShuffle,
    CycleRepeat,
    ToggleLike,
    SyncLiked,
    RefreshQueue,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Phase {
    Starting,
    NeedsLogin,
    Ready,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    pub phase: Phase,
    pub status: String,
    pub bridge_ready: bool,
    pub login_visible: bool,
    pub snapshot: BridgeSnapshot,
    pub home: HomeFeed,
    pub search_results: Vec<HomeItem>,
    pub library: Vec<HomeItem>,
    pub context: Option<ContextPage>,
    pub profile: UserProfile,
    pub volume: f64,
    pub liked: bool,
    pub log: Vec<String>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            phase: Phase::Starting,
            status: "Starting…".into(),
            bridge_ready: false,
            login_visible: false,
            snapshot: BridgeSnapshot::default(),
            home: HomeFeed::default(),
            search_results: Vec::new(),
            library: Vec::new(),
            context: None,
            profile: UserProfile::default(),
            volume: 0.8,
            liked: false,
            log: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct SessionHandle {
    pub cmd_tx: mpsc::UnboundedSender<UiCommand>,
    pub state_rx: watch::Receiver<UiState>,
}

fn push(tx: &watch::Sender<UiState>, f: impl FnOnce(&mut UiState)) {
    let mut s = tx.borrow().clone();
    f(&mut s);
    let _ = tx.send(s);
}

fn push_if_changed(tx: &watch::Sender<UiState>, f: impl FnOnce(&mut UiState) -> bool) {
    let mut s = tx.borrow().clone();
    if f(&mut s) {
        let _ = tx.send(s);
    }
}

fn log_line(tx: &watch::Sender<UiState>, line: impl Into<String>) {
    let line = line.into();
    tracing::info!(%line, "session");
    push(tx, |s| {
        s.log.push(line);
        if s.log.len() > 80 {
            s.log.drain(0..s.log.len() - 80);
        }
    });
}

async fn attach(cdp: &str) -> anyhow::Result<Player> {
    let (host, messages) = CdpHost::connect(cdp).await?;
    let (bridge, _) = Bridge::attach(host, messages).await;
    Ok(Player::new(bridge))
}

async fn reattach(cdp: &str) -> anyhow::Result<Player> {
    let mut last = anyhow::anyhow!("reattach failed");
    for _ in 0..10 {
        match attach(cdp).await {
            Ok(p) => return Ok(p),
            Err(err) => {
                last = err;
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        }
    }
    Err(last)
}

async fn boot_ready(
    player: &Player,
    catalog: &mut Option<SpotifyCatalog>,
    state_tx: &watch::Sender<UiState>,
) -> anyhow::Result<()> {
    player.ensure_ready().await?;
    // Never mark Ready on a guest shell — that was the "empty account" after a
    // login that lost cookies when Chromium was killed too early.
    match player.token_anonymous().await {
        Ok(Some(true)) => {
            anyhow::bail!("Spotify session is still anonymous");
        }
        Ok(None) => {
            anyhow::bail!("Spotify token unavailable");
        }
        Ok(Some(false)) => {}
        Err(err) => anyhow::bail!("Spotify token check failed: {err}"),
    }
    *catalog = Some(SpotifyCatalog::new(player.bridge()));
    if let Some(cat) = catalog {
        match cat.home("").await {
            Ok(feed) => {
                push(state_tx, |s| {
                    s.home = feed;
                });
            }
            Err(err) => log_line(state_tx, format!("home: {err}")),
        }
        match cat.library("").await {
            Ok(items) => push(state_tx, |s| s.library = items),
            Err(err) => log_line(state_tx, format!("library: {err}")),
        }
        match cat.me().await {
            Ok(profile) => push(state_tx, |s| s.profile = profile),
            Err(err) => log_line(state_tx, format!("profile: {err}")),
        }
    }
    if let Ok(vol) = player.get_volume().await {
        push(state_tx, |s| s.volume = vol.clamp(0.0, 1.0));
    }
    let _ = player.refresh_queue().await;
    push(state_tx, |s| {
        s.phase = Phase::Ready;
        s.bridge_ready = true;
        s.login_visible = false;
        s.status = String::new();
    });
    Ok(())
}

pub fn spawn_session() -> SessionHandle {
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<UiCommand>();
    let (state_tx, state_rx) = watch::channel(UiState::default());

    #[cfg(target_os = "linux")]
    crate::mpris::spawn_mpris(cmd_tx.clone(), state_rx.clone());

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio");
        rt.block_on(async move {
            let mut chromium: Option<ChromiumSession> = None;
            let mut player: Option<Player> = None;
            let mut catalog: Option<SpotifyCatalog> = None;
            let mut cdp_url = DEFAULT_CDP_URL.to_string();
            let mut awaiting_login = false;
            let mut login_signed_ticks: u8 = 0;
            let mut stall_watch = StallWatch::default();
            let mut ad_watch = AdWatch::default();
            // Fast while playing so ads are silenced before they are heard; idle stays cheap.
            let mut tick_ms: u64 = 750;
            let mut last_play_at = std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(10))
                .unwrap_or_else(std::time::Instant::now);

            push(&state_tx, |s| {
                s.phase = Phase::Starting;
                s.status = "Connecting to Spotify…".into();
            });
            match ChromiumSession::start(ChromiumConfig::default()).await {
                Ok(session) => {
                    cdp_url = session.cdp_url.clone();
                    chromium = Some(session);
                    log_line(&state_tx, "Web Player on Xvfb (no desktop window)");
                }
                Err(err) => {
                    log_line(&state_tx, format!("start: {err} — trying existing CDP"));
                }
            }

            match attach(&cdp_url).await {
                Ok(p) => {
                    if let Err(err) = p.bridge().inject().await {
                        log_line(&state_tx, format!("inject: {err}"));
                    }
                    match p.ensure_ready().await {
                        Ok(()) => {
                            let anonymous = p.token_anonymous().await.ok().flatten();
                            let url = p.page_url().await.unwrap_or_default();
                            if anonymous == Some(true) || is_login_url(&url) {
                                push(&state_tx, |s| {
                                    s.phase = Phase::NeedsLogin;
                                    s.status =
                                        "Sign in once with Spotify — then it stays signed in.".into();
                                });
                                log_line(&state_tx, "Login required");
                            } else if let Err(err) = boot_ready(&p, &mut catalog, &state_tx).await {
                                log_line(&state_tx, format!("boot polish: {err}"));
                                push(&state_tx, |s| {
                                    s.phase = Phase::Ready;
                                    s.bridge_ready = true;
                                });
                            }
                            player = Some(p);
                        }
                        Err(err) => {
                            let url = p.page_url().await.unwrap_or_default();
                            if is_login_url(&url) || url.is_empty() {
                                push(&state_tx, |s| {
                                    s.phase = Phase::NeedsLogin;
                                    s.status =
                                        "Sign in once with Spotify — then it stays signed in.".into();
                                });
                            } else {
                                push(&state_tx, |s| {
                                    s.phase = Phase::Error;
                                    s.status = format!("Could not reach Spotify player: {err}");
                                });
                            }
                            player = Some(p);
                            log_line(&state_tx, format!("ready wait: {err}"));
                        }
                    }
                }
                Err(err) => {
                    push(&state_tx, |s| {
                        s.phase = Phase::Error;
                        s.status = format!("Could not start Web Player: {err}");
                    });
                }
            }

            loop {
                let cmd = tokio::select! {
                    c = cmd_rx.recv() => c,
                    // Snapshot + ad/stall watchdogs. ~200ms while playing so ads are
                    // silenced before audio leaves the element; 750ms when idle.
                    _ = tokio::time::sleep(std::time::Duration::from_millis(tick_ms)) => {
                        let mut signed_in = false;
                        if let Some(p) = &player {
                            let snap = p.snapshot().await;
                            tick_ms = match snap.state.as_ref() {
                                Some(st) if !st.is_paused => {
                                    let now_ms = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|d| d.as_millis() as i64)
                                        .unwrap_or(0);
                                    let pos = st.live_position_ms(now_ms);
                                    let dur = st.duration_ms.max(1);
                                    let hot = st.track.as_ref().is_some_and(|t| t.is_ad())
                                        || pos + 4_000 >= dur;
                                    if hot { 150 } else { 350 }
                                }
                                _ => 750,
                            };
                            // Ignore playhead-only ticks — PlayerBar interpolates locally.
                            // Pushing every position sample re-rendered the whole shell.
                            push_if_changed(&state_tx, |s| {
                                let changed =
                                    s.bridge_ready != snap.ready || !s.snapshot.ui_eq(&snap);
                                if changed {
                                    s.bridge_ready = snap.ready;
                                    s.snapshot = snap.clone();
                                }
                                changed
                            });
                            if state_tx.borrow().phase == Phase::Ready && snap.ready {
                                if let Err(err) = p.watch_ads(&mut ad_watch).await {
                                    tracing::warn!(?err, "ad watchdog");
                                }
                                if let Err(err) = p.watch_stalls(&mut stall_watch).await {
                                    tracing::warn!(?err, "stall watchdog");
                                }
                            }
                            if awaiting_login {
                                tick_ms = 200;
                                let _ = p.bridge().inject().await;
                                let url = p.page_url().await.unwrap_or_default();
                                let on_player = url.starts_with("https://open.spotify.com")
                                    && !is_login_url(&url);
                                // Need the real web player + a non-anonymous token.
                                // accounts.spotify.com has no bridge services.
                                signed_in = on_player
                                    && matches!(p.token_anonymous().await, Ok(Some(false)));
                                if signed_in && login_signed_ticks == 0 {
                                    log_line(
                                        &state_tx,
                                        "Login detected — waiting for session to settle…",
                                    );
                                    push(&state_tx, |s| {
                                        s.status =
                                            "Login detected. Finishing sign-in…".into();
                                    });
                                }
                            }
                        }
                        if signed_in {
                            // ~3s settle so Chromium flushes cookies before we kill it.
                            // Killing mid-redirect was coming back as an empty guest account.
                            login_signed_ticks = login_signed_ticks.saturating_add(1);
                            if login_signed_ticks < 15 {
                                continue;
                            }
                            if let Some(p) = player.take() {
                                let _ = p
                                    .bridge()
                                    .navigate("https://open.spotify.com/")
                                    .await;
                                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                                // Graceful close flushes the profile; hard kill often did not.
                                let _ = p.bridge().browser_close().await;
                                tokio::time::sleep(std::time::Duration::from_millis(700)).await;
                            }
                            catalog = None;
                            if let Some(session) = chromium.as_mut() {
                                match session.relaunch_as(false).await {
                                    Ok(()) => {
                                        cdp_url = session.cdp_url.clone();
                                        log_line(
                                            &state_tx,
                                            "Signed in — Web Player back on Xvfb",
                                        );
                                    }
                                    Err(err) => {
                                        log_line(&state_tx, format!("hide relaunch: {err}"))
                                    }
                                }
                            }
                            match reattach(&cdp_url).await {
                                Ok(p2) => match boot_ready(&p2, &mut catalog, &state_tx).await {
                                    Ok(()) => {
                                        awaiting_login = false;
                                        login_signed_ticks = 0;
                                        player = Some(p2);
                                    }
                                    Err(err) => {
                                        log_line(
                                            &state_tx,
                                            format!(
                                                "session not ready after login ({err}) — keeping login window"
                                            ),
                                        );
                                        // Cookies likely missed disk: show login again.
                                        player = None;
                                        if let Some(session) = chromium.as_mut() {
                                            if session.relaunch_as(true).await.is_ok() {
                                                cdp_url = session.cdp_url.clone();
                                            }
                                        }
                                        match reattach(&cdp_url).await {
                                            Ok(p3) => {
                                                let _ = p3.bridge().inject().await;
                                                player = Some(p3);
                                            }
                                            Err(e) => {
                                                log_line(&state_tx, format!("reattach: {e}"))
                                            }
                                        }
                                        awaiting_login = true;
                                        login_signed_ticks = 0;
                                        push(&state_tx, |s| {
                                            s.phase = Phase::NeedsLogin;
                                            s.login_visible = true;
                                            s.status = "Sign-in did not stick. Please log in again in the Spotify window.".into();
                                        });
                                    }
                                },
                                Err(err) => {
                                    log_line(&state_tx, format!("reattach: {err}"));
                                    awaiting_login = false;
                                    login_signed_ticks = 0;
                                }
                            }
                        } else if awaiting_login {
                            login_signed_ticks = 0;
                        }
                        continue;
                    }
                };
                let Some(cmd) = cmd else { break };

                match cmd {
                    UiCommand::Shutdown => break,

                    UiCommand::ShowLogin => {
                        if state_tx.borrow().phase != Phase::NeedsLogin {
                            log_line(&state_tx, "Already signed in — keeping player on Xvfb");
                        } else {
                            player = None;
                            catalog = None;
                            let shown = if let Some(session) = chromium.as_mut() {
                                match session.relaunch_as(true).await {
                                    Ok(()) => {
                                        cdp_url = session.cdp_url.clone();
                                        true
                                    }
                                    Err(err) => {
                                        log_line(&state_tx, format!("login relaunch: {err}"));
                                        false
                                    }
                                }
                            } else {
                                match ChromiumSession::start(ChromiumConfig {
                                    visible: true,
                                    ..ChromiumConfig::default()
                                })
                                .await
                                {
                                    Ok(session) => {
                                        cdp_url = session.cdp_url.clone();
                                        chromium = Some(session);
                                        true
                                    }
                                    Err(err) => {
                                        log_line(&state_tx, format!("login start: {err}"));
                                        false
                                    }
                                }
                            };
                            if shown {
                                match reattach(&cdp_url).await {
                                    Ok(p2) => {
                                        // Chromium already opened accounts.spotify.com via --app.
                                        // Do not navigate again (that used to open a second window
                                        // when CDP attached by creating open.spotify.com).
                                        let _ = p2.bridge().inject().await;
                                        player = Some(p2);
                                        awaiting_login = true;
                                        login_signed_ticks = 0;
                                        push(&state_tx, |s| {
                                            s.login_visible = true;
                                            s.status = "Sign in in the Spotify window. Sloptify hides it when you are done.".into();
                                        });
                                        log_line(&state_tx, "Login window open");
                                    }
                                    Err(err) => {
                                        push(&state_tx, |s| {
                                            s.phase = Phase::Error;
                                            s.status = format!("{err}");
                                        });
                                    }
                                }
                            }
                        }
                    }

                    UiCommand::ContinueAfterLogin => {
                        player = None;
                        catalog = None;
                        if let Some(session) = chromium.as_mut() {
                            if let Err(err) = session.relaunch_as(false).await {
                                log_line(&state_tx, format!("hide relaunch: {err}"));
                            } else {
                                cdp_url = session.cdp_url.clone();
                            }
                        }
                        match reattach(&cdp_url).await {
                            Ok(p2) => {
                                match boot_ready(&p2, &mut catalog, &state_tx).await {
                                    Ok(()) => {},
                                    Err(err) => push(&state_tx, |s| {
                                        s.phase = Phase::NeedsLogin;
                                        s.status = format!("{err}");
                                    }),
                                }
                                player = Some(p2);
                                awaiting_login = false;
                            }
                            Err(err) => push(&state_tx, |s| {
                                s.phase = Phase::Error;
                                s.status = format!("{err}");
                            }),
                        }
                    }

                    UiCommand::HidePlayer => {
                        player = None;
                        catalog = None;
                        if let Some(session) = chromium.as_mut() {
                            if let Err(err) = session.relaunch_as(false).await {
                                log_line(&state_tx, format!("hide: {err}"));
                            } else {
                                cdp_url = session.cdp_url.clone();
                            }
                        }
                        match reattach(&cdp_url).await {
                            Ok(p2) => {
                                player = Some(p2);
                                push(&state_tx, |s| s.login_visible = false);
                            }
                            Err(err) => log_line(&state_tx, format!("hide reattach: {err}")),
                        }
                    }

                    UiCommand::RefreshHome { facet } => {
                        if let Some(cat) = &catalog {
                            match cat.home(&facet).await {
                                Ok(feed) => push(&state_tx, |s| s.home = feed),
                                Err(err) => log_line(&state_tx, format!("home: {err}")),
                            }
                        }
                    }

                    UiCommand::LoadLibrary { filter } => {
                        if let Some(cat) = &catalog {
                            match cat.library(&filter).await {
                                Ok(items) => push(&state_tx, |s| s.library = items),
                                Err(err) => log_line(&state_tx, format!("library: {err}")),
                            }
                        }
                    }

                    UiCommand::PlayUri { uri } => {
                        // Debounce triple-fires from the shell.
                        if last_play_at.elapsed() < std::time::Duration::from_millis(400) {
                            continue;
                        }
                        last_play_at = std::time::Instant::now();
                        if let Some(cat) = &catalog {
                            if let Err(err) = cat.play_uri(&uri, None).await {
                                log_line(&state_tx, format!("play: {err}"));
                            } else if let Some(p) = &player {
                                let _ = p.refresh_queue().await;
                            }
                        }
                    }

                    UiCommand::PlayInContext {
                        context_uri,
                        track_uri,
                        track_uid,
                        index,
                    } => {
                        if last_play_at.elapsed() < std::time::Duration::from_millis(400) {
                            continue;
                        }
                        last_play_at = std::time::Instant::now();
                        if let Some(cat) = &catalog {
                            if let Err(err) = cat
                                .play_skip(
                                    &context_uri,
                                    Some(&track_uri),
                                    track_uid.as_deref(),
                                    index,
                                )
                                .await
                            {
                                log_line(&state_tx, format!("play: {err}"));
                            } else if let Some(p) = &player {
                                let _ = p.refresh_queue().await;
                            }
                        }
                    }

                    UiCommand::OpenContext { uri, title, image } => {
                        let hint_title = title
                            .clone()
                            .filter(|t| !t.is_empty() && !t.starts_with("spotify:"))
                            .unwrap_or_else(|| "Loading…".into());
                        let kind = sloptify_spotify::ContextKind::from_uri(&uri);
                        push(&state_tx, |s| {
                            s.context = Some(ContextPage {
                                uri: uri.clone(),
                                kind,
                                title: hint_title,
                                image: image.clone(),
                                loading: true,
                                ..ContextPage::default()
                            });
                        });
                        if let Some(cat) = &catalog {
                            match cat.context_page(&uri, 0, 50).await {
                                Ok(mut page) => {
                                    if page.title.is_empty()
                                        || page.title == "Playlist"
                                        || page.title == "Album"
                                        || page.title == "Artist"
                                    {
                                        if let Some(t) = title.clone().filter(|t| {
                                            !t.is_empty() && !t.starts_with("spotify:")
                                        }) {
                                            page.title = t;
                                        }
                                    }
                                    // Keep the cover the user clicked — GQL often returns a
                                    // different/first-track image that makes the hero "change".
                                    if let Some(img) =
                                        image.clone().filter(|s| !s.is_empty())
                                    {
                                        page.image = Some(img);
                                    }
                                    push(&state_tx, |s| s.context = Some(page));
                                }
                                Err(err) => push(&state_tx, |s| {
                                    s.context = Some(ContextPage {
                                        uri: uri.clone(),
                                        kind,
                                        title: title.unwrap_or_else(|| "Couldn’t open".into()),
                                        image,
                                        error: err.to_string(),
                                        loading: false,
                                        ..ContextPage::default()
                                    });
                                }),
                            }
                        }
                    }

                    UiCommand::LoadMoreContext => {
                        let (uri, offset) = {
                            let s = state_tx.borrow();
                            match s.context.as_ref() {
                                Some(c) if c.next_offset.is_some() && !c.loading => {
                                    (c.uri.clone(), c.next_offset.unwrap())
                                }
                                _ => continue,
                            }
                        };
                        // Mark loading without cloning the whole track list into a second push later.
                        push_if_changed(&state_tx, |s| {
                            if let Some(c) = s.context.as_mut() {
                                if !c.loading {
                                    c.loading = true;
                                    return true;
                                }
                            }
                            false
                        });
                        if let Some(cat) = &catalog {
                            match cat.context_page(&uri, offset, 50).await {
                                Ok(more) => push(&state_tx, |s| {
                                    if let Some(c) = s.context.as_mut() {
                                        c.tracks.extend(more.tracks);
                                        c.total = more.total;
                                        c.next_offset = more.next_offset;
                                        c.loading = false;
                                    }
                                }),
                                Err(err) => {
                                    push(&state_tx, |s| {
                                        if let Some(c) = s.context.as_mut() {
                                            c.loading = false;
                                            c.error = err.to_string();
                                        }
                                    });
                                }
                            }
                        }
                    }

                    UiCommand::CloseContext => {
                        push(&state_tx, |s| s.context = None);
                    }

                    UiCommand::QueueAdd { uri } => {
                        if let Some(p) = &player {
                            if let Err(err) = p.queue_add(&uri).await {
                                log_line(&state_tx, format!("queue: {err}"));
                            } else {
                                let _ = p.refresh_queue().await;
                                let snap = p.snapshot().await;
                                push_if_changed(&state_tx, |s| {
                                    let changed = !s.snapshot.ui_eq(&snap);
                                    if changed {
                                        s.snapshot = snap;
                                    }
                                    changed
                                });
                                log_line(&state_tx, "Added to queue");
                            }
                        }
                    }

                    UiCommand::QueuePlayNext { uri } => {
                        if let Some(p) = &player {
                            if let Err(err) = p.queue_play_next(&uri).await {
                                log_line(&state_tx, format!("play next: {err}"));
                            } else {
                                let _ = p.refresh_queue().await;
                                let snap = p.snapshot().await;
                                push_if_changed(&state_tx, |s| {
                                    let changed = !s.snapshot.ui_eq(&snap);
                                    if changed {
                                        s.snapshot = snap;
                                    }
                                    changed
                                });
                                log_line(&state_tx, "Playing next");
                            }
                        }
                    }

                    UiCommand::ToggleLikeUri { uri } => {
                        if let Some(p) = &player {
                            match p.library_contains(&uri).await {
                                Ok(liked) => {
                                    let res = if liked {
                                        p.library_remove(&uri).await
                                    } else {
                                        p.library_add(&uri).await
                                    };
                                    match res {
                                        Ok(()) => {
                                            if liked {
                                                log_line(&state_tx, "Removed from Liked Songs");
                                            } else {
                                                log_line(&state_tx, "Added to Liked Songs");
                                            }
                                            if state_tx
                                                .borrow()
                                                .snapshot
                                                .state
                                                .as_ref()
                                                .and_then(|s| s.track.as_ref())
                                                .and_then(|t| t.uri.as_ref())
                                                == Some(&uri)
                                            {
                                                push(&state_tx, |s| s.liked = !liked);
                                            }
                                        }
                                        Err(err) => log_line(&state_tx, format!("like: {err}")),
                                    }
                                }
                                Err(err) => log_line(&state_tx, format!("like: {err}")),
                            }
                        }
                    }

                    UiCommand::CopyLink { uri } => {
                        if let Some(url) = spotify_web_url(&uri) {
                            match copy_to_clipboard(&url) {
                                Ok(()) => log_line(&state_tx, "Link copied"),
                                Err(err) => log_line(&state_tx, format!("copy: {err}")),
                            }
                        } else {
                            log_line(&state_tx, "Nothing to copy");
                        }
                    }

                    UiCommand::SongRadio { uri, title } => {
                        if let Some(p) = &player {
                            match p.radio_from_seed(&uri).await {
                                Ok(Some(playlist_uri)) => {
                                    let radio_title = title
                                        .filter(|t| !t.is_empty())
                                        .map(|t| format!("{t} Radio"))
                                        .unwrap_or_else(|| "Song Radio".into());
                                    push(&state_tx, |s| {
                                        s.context = Some(ContextPage {
                                            uri: playlist_uri.clone(),
                                            kind: sloptify_spotify::ContextKind::Playlist,
                                            title: radio_title.clone(),
                                            loading: true,
                                            ..ContextPage::default()
                                        });
                                    });
                                    if let Some(cat) = &catalog {
                                        match cat.context_page(&playlist_uri, 0, 50).await {
                                            Ok(mut page) => {
                                                if page.title.is_empty()
                                                    || page.title == "Playlist"
                                                {
                                                    page.title = radio_title;
                                                }
                                                push(&state_tx, |s| s.context = Some(page));
                                            }
                                            Err(err) => log_line(
                                                &state_tx,
                                                format!("song radio page: {err}"),
                                            ),
                                        }
                                        if let Err(err) =
                                            cat.play_uri(&playlist_uri, None).await
                                        {
                                            log_line(
                                                &state_tx,
                                                format!("song radio play: {err}"),
                                            );
                                        }
                                    }
                                }
                                Ok(None) => log_line(&state_tx, "Song radio unavailable"),
                                Err(err) => {
                                    log_line(&state_tx, format!("song radio: {err}"))
                                }
                            }
                        }
                    }

                    UiCommand::SignOut => {
                        log_line(&state_tx, "Signing out…");
                        player = None;
                        catalog = None;
                        if let Some(mut session) = chromium.take() {
                            session.stop();
                        }
                        clear_spotify_login();
                        push(&state_tx, |s| {
                            *s = UiState {
                                phase: Phase::NeedsLogin,
                                status: "Signed out — log in with Spotify again.".into(),
                                ..UiState::default()
                            };
                        });
                    }

                    UiCommand::QueueSkip { uri, uid } => {
                        if let Some(p) = &player {
                            if let Err(err) = p.queue_skip_to(&uri, uid.as_deref()).await {
                                log_line(&state_tx, format!("queue skip: {err}"));
                            } else {
                                let _ = p.refresh_queue().await;
                                let snap = p.snapshot().await;
                                push_if_changed(&state_tx, |s| {
                                    let changed = !s.snapshot.ui_eq(&snap);
                                    if changed {
                                        s.snapshot = snap;
                                    }
                                    changed
                                });
                            }
                        }
                    }

                    UiCommand::Search { query } => {
                        if let Some(cat) = &catalog {
                            match cat.search(&query).await {
                                Ok(items) => push(&state_tx, |s| s.search_results = items),
                                Err(err) => log_line(&state_tx, format!("search: {err}")),
                            }
                        }
                    }

                    UiCommand::Pause => {
                        // Optimistic chrome state — don't wait on CDP for the icon.
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0);
                        push(&state_tx, |s| {
                            if let Some(st) = s.snapshot.state.as_mut() {
                                st.position_ms = st.live_position_ms(now);
                                st.timestamp = Some(now);
                                st.is_paused = true;
                            }
                        });
                        if let Some(p) = &player {
                            if let Err(err) = p.pause().await {
                                log_line(&state_tx, format!("pause: {err}"));
                            }
                        }
                    }
                    UiCommand::Resume => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0);
                        push(&state_tx, |s| {
                            if let Some(st) = s.snapshot.state.as_mut() {
                                st.is_paused = false;
                                st.timestamp = Some(now);
                            }
                        });
                        if let Some(p) = &player {
                            let snap = p.snapshot().await;
                            let track = snap.state.as_ref().and_then(|s| s.track.as_ref());
                            let has_real = track.map(|t| t.is_playable_music()).unwrap_or(false);
                            if !has_real {
                                if let Some(cat) = &catalog {
                                    let ctx = snap
                                        .state
                                        .as_ref()
                                        .and_then(|s| s.context_uri.clone())
                                        .filter(|c| !c.contains(":ad:"));
                                    let result = if let Some(ctx) = ctx {
                                        cat.play_uri(&ctx, None).await
                                    } else {
                                        cat.play_liked_or_resume().await
                                    };
                                    if let Err(err) = result {
                                        log_line(&state_tx, format!("play: {err}"));
                                    }
                                }
                            } else if let Err(err) = p.resume().await {
                                log_line(&state_tx, format!("resume: {err}"));
                                if let Some(cat) = &catalog {
                                    if let Err(err) = cat.play_liked_or_resume().await {
                                        log_line(&state_tx, format!("play: {err}"));
                                    }
                                }
                            }
                        }
                    }
                    UiCommand::Next => {
                        if let Some(p) = &player {
                            let _ = p.next().await;
                        }
                    }
                    UiCommand::Previous => {
                        if let Some(p) = &player {
                            let _ = p.previous().await;
                        }
                    }
                    UiCommand::Seek { position_ms } => {
                        if let Some(p) = &player {
                            let position_ms = position_ms.max(0);
                            tracing::info!(position_ms, "ui seek");
                            if let Err(err) = p.seek(position_ms).await {
                                tracing::warn!(?err, position_ms, "seek failed");
                                log_line(&state_tx, format!("seek: {err}"));
                            } else {
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis() as i64)
                                    .unwrap_or(0);
                                push(&state_tx, |s| {
                                    if let Some(st) = s.snapshot.state.as_mut() {
                                        st.position_ms = position_ms;
                                        st.timestamp = Some(now);
                                    }
                                });
                            }
                        }
                    }
                    UiCommand::SetVolume { level } => {
                        let level = level.clamp(0.0, 1.0);
                        push(&state_tx, |s| s.volume = level);
                        if let Some(p) = &player {
                            if let Err(err) = p.set_volume(level).await {
                                log_line(&state_tx, format!("volume: {err}"));
                            }
                        }
                    }
                    UiCommand::ToggleShuffle => {
                        if let Some(p) = &player {
                            let on = !p
                                .snapshot()
                                .await
                                .state
                                .as_ref()
                                .map(|s| s.shuffle)
                                .unwrap_or(false);
                            if let Err(err) = p.set_shuffle(on).await {
                                log_line(&state_tx, format!("shuffle: {err}"));
                            }
                        }
                    }
                    UiCommand::CycleRepeat => {
                        if let Some(p) = &player {
                            let cur = p
                                .snapshot()
                                .await
                                .state
                                .as_ref()
                                .map(|s| s.repeat)
                                .unwrap_or(0);
                            let next_mode = match cur {
                                0 => 1,
                                1 => 2,
                                _ => 0,
                            };
                            if let Err(err) = p.set_repeat(next_mode).await {
                                log_line(&state_tx, format!("repeat: {err}"));
                            }
                        }
                    }
                    UiCommand::ToggleLike => {
                        if let Some(p) = &player {
                            let uri = p
                                .snapshot()
                                .await
                                .state
                                .as_ref()
                                .and_then(|s| s.track.as_ref())
                                .and_then(|t| t.uri.clone());
                            if let Some(uri) = uri {
                                let liked = state_tx.borrow().liked;
                                let result = if liked {
                                    p.library_remove(&uri).await
                                } else {
                                    p.library_add(&uri).await
                                };
                                match result {
                                    Ok(()) => push(&state_tx, |s| s.liked = !liked),
                                    Err(err) => log_line(&state_tx, format!("like: {err}")),
                                }
                            }
                        }
                    }
                    UiCommand::SyncLiked => {
                        if let Some(p) = &player {
                            let uri = p
                                .snapshot()
                                .await
                                .state
                                .as_ref()
                                .and_then(|s| s.track.as_ref())
                                .and_then(|t| t.uri.clone());
                            if let Some(uri) = uri {
                                match p.library_contains(&uri).await {
                                    Ok(liked) => push_if_changed(&state_tx, |s| {
                                        let changed = s.liked != liked;
                                        s.liked = liked;
                                        changed
                                    }),
                                    Err(_) => {}
                                }
                            }
                        }
                    }
                    UiCommand::RefreshQueue => {
                        if let Some(p) = &player {
                            let _ = p.refresh_queue().await;
                            let snap = p.snapshot().await;
                            push_if_changed(&state_tx, |s| {
                                let changed = !s.snapshot.ui_eq(&snap);
                                if changed {
                                    s.snapshot = snap;
                                }
                                changed
                            });
                        }
                    }
                }
            }
        });
    });

    SessionHandle { cmd_tx, state_rx }
}

fn spotify_web_url(uri: &str) -> Option<String> {
    let mut parts = uri.split(':');
    let scheme = parts.next()?;
    if scheme != "spotify" {
        return None;
    }
    let kind = parts.next()?;
    let id = parts.next()?;
    if kind == "user" {
        // spotify:user:name[:playlist:id]
        let rest: Vec<_> = std::iter::once(id).chain(parts).collect();
        if rest.len() >= 3 && rest[1] == "playlist" {
            return Some(format!("https://open.spotify.com/playlist/{}", rest[2]));
        }
        return Some(format!("https://open.spotify.com/user/{}", rest[0]));
    }
    if kind == "collection" && id == "tracks" {
        return Some("https://open.spotify.com/collection/tracks".into());
    }
    Some(format!("https://open.spotify.com/{kind}/{id}"))
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    for (bin, args) in [
        ("wl-copy", vec![] as Vec<&str>),
        ("xclip", vec!["-selection", "clipboard"]),
        ("xsel", vec!["--clipboard", "--input"]),
    ] {
        if std::process::Command::new(bin)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()?;
                Ok(())
            })
            .is_ok()
        {
            return Ok(());
        }
    }
    Err("no clipboard tool (wl-copy / xclip / xsel)".into())
}

fn clear_spotify_login() {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let share = home.join(".local/share/sloptify");
    // Drop cookies / login tokens so the next launch requires Spotify sign-in.
    for root in [share.join("player"), share.join("chromium")] {
        for rel in [
            "Default/Cookies",
            "Default/Cookies-journal",
            "Default/Login Data",
            "Default/Login Data-journal",
            "Default/Network/Cookies",
            "Default/Network/Cookies-journal",
            "Default/Web Data",
            "Default/Web Data-journal",
        ] {
            let _ = std::fs::remove_file(root.join(rel));
        }
    }
}
