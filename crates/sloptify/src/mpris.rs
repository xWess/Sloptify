//! Sloptify-owned MPRIS so Omarchy / Hyprland / playerctl show "Sloptify", not Chromium.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use mpris_server::{LoopStatus, Metadata, PlaybackStatus, Player, Time, TrackId, Volume};
use tokio::sync::{mpsc, watch};

use crate::session::{Phase, UiCommand, UiState};

/// Run on a dedicated current-thread runtime (mpris-server uses `LocalServer`).
pub fn spawn_mpris(
    cmd_tx: mpsc::UnboundedSender<UiCommand>,
    state_rx: watch::Receiver<UiState>,
) {
    std::thread::Builder::new()
        .name("sloptify-mpris".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    tracing::warn!(%err, "mpris: tokio runtime");
                    return;
                }
            };
            rt.block_on(async move {
                let local = tokio::task::LocalSet::new();
                local
                    .run_until(async move {
                        if let Err(err) = run_mpris(cmd_tx, state_rx).await {
                            tracing::warn!(%err, "mpris: stopped");
                        }
                    })
                    .await;
            });
        })
        .ok();
}

async fn run_mpris(
    cmd_tx: mpsc::UnboundedSender<UiCommand>,
    mut state_rx: watch::Receiver<UiState>,
) -> anyhow::Result<()> {
    let player = Player::builder("sloptify")
        .identity("Sloptify")
        .desktop_entry("sloptify")
        .can_raise(false)
        .can_quit(false)
        .can_play(true)
        .can_pause(true)
        .can_go_next(true)
        .can_go_previous(true)
        .can_seek(true)
        .can_control(true)
        .supported_uri_schemes(["spotify"])
        .build()
        .await?;

    let paused = Rc::new(Cell::new(true));

    {
        let tx = cmd_tx.clone();
        player.connect_next(move |_| {
            let _ = tx.send(UiCommand::Next);
        });
    }
    {
        let tx = cmd_tx.clone();
        player.connect_previous(move |_| {
            let _ = tx.send(UiCommand::Previous);
        });
    }
    {
        let tx = cmd_tx.clone();
        player.connect_play(move |_| {
            let _ = tx.send(UiCommand::Resume);
        });
    }
    {
        let tx = cmd_tx.clone();
        player.connect_pause(move |_| {
            let _ = tx.send(UiCommand::Pause);
        });
    }
    {
        let tx = cmd_tx.clone();
        let paused = paused.clone();
        player.connect_play_pause(move |_| {
            if paused.get() {
                let _ = tx.send(UiCommand::Resume);
            } else {
                let _ = tx.send(UiCommand::Pause);
            }
        });
    }
    {
        let tx = cmd_tx.clone();
        player.connect_stop(move |_| {
            let _ = tx.send(UiCommand::Pause);
        });
    }
    {
        let tx = cmd_tx.clone();
        player.connect_seek(move |p, offset| {
            let next = p.position() + offset;
            let _ = tx.send(UiCommand::Seek {
                position_ms: next.as_millis().max(0),
            });
        });
    }
    {
        let tx = cmd_tx.clone();
        player.connect_set_position(move |_, _track, position| {
            let _ = tx.send(UiCommand::Seek {
                position_ms: position.as_millis().max(0),
            });
        });
    }
    {
        let tx = cmd_tx.clone();
        player.connect_set_volume(move |_, volume: Volume| {
            let _ = tx.send(UiCommand::SetVolume {
                level: volume.clamp(0.0, 1.0),
            });
        });
    }
    {
        let tx = cmd_tx.clone();
        player.connect_set_shuffle(move |_, _shuffle| {
            let _ = tx.send(UiCommand::ToggleShuffle);
        });
    }
    {
        let tx = cmd_tx.clone();
        player.connect_set_loop_status(move |_, _status| {
            let _ = tx.send(UiCommand::CycleRepeat);
        });
    }

    tokio::task::spawn_local(player.run());
    tracing::info!("mpris: Identity=Sloptify on org.mpris.MediaPlayer2.sloptify");

    let mut last_sig = String::new();
    loop {
        let state = state_rx.borrow().clone();
        sync_player(&player, &state, &paused, &mut last_sig).await;
        tokio::select! {
            changed = state_rx.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(500)) => {}
        }
    }
    Ok(())
}

async fn sync_player(
    player: &Player,
    state: &UiState,
    paused_cell: &Rc<Cell<bool>>,
    last_sig: &mut String,
) {
    let ready = state.phase == Phase::Ready && state.bridge_ready;
    let ps = state.snapshot.state.as_ref();
    let is_paused = ps.map(|s| s.is_paused).unwrap_or(true);
    paused_cell.set(is_paused);

    let status = if !ready || ps.and_then(|s| s.track.as_ref()).is_none() {
        PlaybackStatus::Stopped
    } else if is_paused {
        PlaybackStatus::Paused
    } else {
        PlaybackStatus::Playing
    };
    let _ = player.set_playback_status(status).await;

    let can_next = ps.map(|s| s.restrictions.can_skip_next).unwrap_or(false);
    let can_prev = ps.map(|s| s.restrictions.can_skip_previous).unwrap_or(false);
    let can_seek = ps.map(|s| s.restrictions.can_seek).unwrap_or(false);
    let _ = player.set_can_go_next(ready && can_next).await;
    let _ = player.set_can_go_previous(ready && can_prev).await;
    let _ = player.set_can_seek(ready && can_seek).await;
    let _ = player.set_can_play(ready).await;
    let _ = player.set_can_pause(ready).await;

    if let Some(s) = ps {
        let _ = player.set_shuffle(s.shuffle).await;
        let loop_status = match s.repeat {
            1 => LoopStatus::Playlist,
            2 => LoopStatus::Track,
            _ => LoopStatus::None,
        };
        let _ = player.set_loop_status(loop_status).await;
        player.set_position(Time::from_millis(s.position_ms.max(0)));
    }

    let _ = player
        .set_volume(state.volume.clamp(0.0, 1.0))
        .await;

    let sig = metadata_sig(ps);
    if sig != *last_sig {
        *last_sig = sig;
        let meta = build_metadata(ps);
        let _ = player.set_metadata(meta).await;
    }
}

fn metadata_sig(ps: Option<&sloptify_spotify::PlayerState>) -> String {
    match ps.and_then(|s| s.track.as_ref()) {
        Some(t) => format!(
            "{}|{}|{}",
            t.uri.as_deref().unwrap_or(""),
            t.name.as_deref().unwrap_or(""),
            t.duration_ms.unwrap_or(ps.map(|s| s.duration_ms).unwrap_or(0))
        ),
        None => String::new(),
    }
}

fn build_metadata(ps: Option<&sloptify_spotify::PlayerState>) -> Metadata {
    let Some(s) = ps else {
        return Metadata::new();
    };
    let Some(track) = s.track.as_ref() else {
        return Metadata::new();
    };

    let uri = track
        .uri
        .clone()
        .unwrap_or_else(|| "spotify:track:unknown".into());
    let path = format!(
        "/org/sloptify/track/{}",
        uri.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
    );
    let track_id = TrackId::try_from(path.as_str()).unwrap_or(TrackId::NO_TRACK);

    let mut b = Metadata::builder().trackid(track_id);
    if let Some(name) = track.name.as_ref() {
        b = b.title(name.clone());
    }
    if let Some(artists) = track.artists.as_ref() {
        let names: Vec<String> = artists.iter().filter_map(|a| a.name.clone()).collect();
        if !names.is_empty() {
            b = b.artist(names);
        }
    }
    if let Some(album) = track.album.as_ref().and_then(|a| a.name.clone()) {
        b = b.album(album);
    }
    if let Some(img) = track.image.as_ref() {
        b = b.art_url(img.clone());
    }
    let len_ms = track.duration_ms.unwrap_or(s.duration_ms).max(0);
    if len_ms > 0 {
        b = b.length(Time::from_millis(len_ms));
    }
    let spotify_id = uri.rsplit(':').next().unwrap_or("unknown");
    b = b.url(format!("https://open.spotify.com/track/{spotify_id}"));
    b.build()
}
