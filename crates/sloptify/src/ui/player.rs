//! Player bar + now playing sheet.

use dioxus::prelude::*;
use icons::{
    ChevronDown, Heart, ListMusic, Pause, Play, Repeat, Repeat1, Shuffle, SkipBack, SkipForward,
    Volume2,
};

use sloptify_spotify::ContextPage;

use super::helpers::{
    cassette_svg_shell, fmt_ms, now_playing, now_unix_ms, send, toggle_transport, trunc,
};
use super::nav_ops::open_context_nav;
use super::types::{Nav, PlayerSlice, Sheet};
use super::widgets::{Cover, SeekScrub};
use crate::components::ui::slider::Slider;
use crate::nav::NavStack;
use crate::session::{SessionHandle, UiCommand};

#[component]
pub fn PlayerBar(
    player: Signal<PlayerSlice>,
    seek_drag: Signal<Option<i64>>,
    vol_drag: Signal<Option<f64>>,
    mut sheet: Signal<Sheet>,
    tick: Signal<u64>,
) -> Element {
    let session = use_context::<SessionHandle>();
    // Do NOT subscribe the whole bar to `tick` — only the seek strip needs the clock.
    let p = player();
    let np = now_playing(&p.snapshot);
    let vol = vol_drag().unwrap_or(p.volume);
    let shuffle_on = p.snapshot.state.as_ref().map(|s| s.shuffle).unwrap_or(false);
    let repeat = p.snapshot.state.as_ref().map(|s| s.repeat).unwrap_or(0);
    let can_prev = np.can_skip_previous;
    let can_next = np.can_skip_next;

    rsx! {
        footer { class: "player",
            button {
                class: "flex min-w-0 items-center gap-3 text-left",
                onclick: move |_| sheet.set(Sheet::NowPlaying),
                Cover { url: np.image.clone(), class: "cover-soft size-12 shrink-0".to_string() }
                div { class: "min-w-0",
                    p { class: "truncate text-[0.9rem] font-semibold leading-tight", "{np.title}" }
                    p { class: "text-muted-foreground mt-0.5 truncate text-[0.78rem]",
                        if let Some(hint) = np.skip_hint.as_ref() {
                            "{hint}"
                        } else {
                            "{np.artist}"
                        }
                    }
                }
            }

            div { class: "flex min-w-0 flex-col items-center gap-1",
                div { class: "flex items-center gap-3",
                    button {
                        class: if shuffle_on { "ctrl-on p-1" } else { "text-muted-foreground hover:text-foreground p-1" },
                        title: "Shuffle",
                        onclick: { let session = session.clone(); move |_| send(&session, UiCommand::ToggleShuffle) },
                        Shuffle { class: "size-4" }
                    }
                    button {
                        class: if can_prev { "text-muted-foreground hover:text-foreground p-1" } else { "text-muted-foreground/40 p-1" },
                        title: if can_prev { "Previous" } else { "Can't go back" },
                        disabled: !can_prev,
                        onclick: { let session = session.clone(); move |_| send(&session, UiCommand::Previous) },
                        SkipBack { class: "size-[1.15rem]" }
                    }
                    button {
                        class: "imm-play flex size-10 items-center justify-center rounded-full shadow-md hover:scale-105",
                        onclick: {
                            let session = session.clone();
                            move |_| toggle_transport(player, &session)
                        },
                        if np.paused {
                            Play { class: "size-4 fill-current" }
                        } else {
                            Pause { class: "size-4 fill-current" }
                        }
                    }
                    button {
                        class: if can_next { "text-muted-foreground hover:text-foreground p-1" } else { "text-muted-foreground/40 p-1" },
                        title: if can_next { "Next" } else if np.is_ad { "Can't skip ad" } else { "Can't skip" },
                        disabled: !can_next,
                        onclick: { let session = session.clone(); move |_| send(&session, UiCommand::Next) },
                        SkipForward { class: "size-[1.15rem]" }
                    }
                    button {
                        class: if repeat > 0 { "ctrl-on p-1" } else { "text-muted-foreground hover:text-foreground p-1" },
                        title: "Repeat",
                        onclick: { let session = session.clone(); move |_| send(&session, UiCommand::CycleRepeat) },
                        if repeat == 2 {
                            Repeat1 { class: "size-4" }
                        } else {
                            Repeat { class: "size-4" }
                        }
                    }
                }
                BarSeek { player, seek_drag, tick }
            }

            div { class: "side-col flex items-center justify-end gap-2 text-muted-foreground",
                button {
                    class: if p.liked { "ctrl-on p-1" } else { "hover:text-foreground p-1" },
                    title: "Save",
                    onclick: { let session = session.clone(); move |_| send(&session, UiCommand::ToggleLike) },
                    Heart { class: if p.liked { "size-4 fill-current" } else { "size-4" } }
                }
                button {
                    class: "hover:text-foreground p-1",
                    title: "Queue",
                    onclick: {
                        let session = session.clone();
                        move |_| {
                            send(&session, UiCommand::RefreshQueue);
                            sheet.set(Sheet::Queue);
                        }
                    },
                    ListMusic { class: "size-4" }
                }
                Volume2 { class: "size-3.5 shrink-0 opacity-70" }
                div {
                    class: "apple-slider volume",
                    style: "--pct: {vol * 100.0}%",
                    div { class: "apple-slider-rail" }
                    div { class: "apple-slider-fill" }
                    div { class: "apple-slider-knob" }
                    Slider {
                        min: 0.0, max: 100.0, step: 0.25, value: vol * 100.0,
                        bind_value: vol_drag().is_none(),
                        oninput: {
                            let session = session.clone();
                            move |e: FormEvent| {
                                if let Ok(v) = e.value().parse::<f64>() {
                                    let level = (v / 100.0).clamp(0.0, 1.0);
                                    vol_drag.set(Some(level));
                                    send(&session, UiCommand::SetVolume { level });
                                }
                            }
                        },
                        onchange: move |_| { vol_drag.set(None); }
                    }
                }
            }
        }
    }
}

/// Shared timeline — player bar and fullscreen NP both use this (tick-driven SeekScrub).
#[component]
fn BarSeek(
    player: Signal<PlayerSlice>,
    mut seek_drag: Signal<Option<i64>>,
    tick: Signal<u64>,
    /// Extra layout for fullscreen NP (`np-tape-seek`).
    #[props(default)]
    wide: bool,
) -> Element {
    let session = use_context::<SessionHandle>();
    let _ = tick();
    let p = player();
    let np = now_playing(&p.snapshot);
    let live_pos = seek_drag().unwrap_or_else(|| {
        p.snapshot
            .state
            .as_ref()
            .map(|s| s.live_position_ms(now_unix_ms()))
            .unwrap_or(np.pos)
    });

    let dur = np.dur.max(0);
    let can_seek = np.can_seek;
    // Ads / blocked seek: show time left on the right so the timeline isn't a fake scrub target.
    let end_label = if np.is_ad || !can_seek {
        format!("-{}", fmt_ms((dur - live_pos).max(0)))
    } else {
        fmt_ms(np.dur)
    };
    let wrap_class = if wide {
        "bar-seek np-tape-seek"
    } else {
        "bar-seek"
    };

    rsx! {
        div { class: "{wrap_class}",
            span { class: "bar-seek-time", "{fmt_ms(live_pos)}" }
            SeekScrub {
                position_ms: live_pos,
                duration_ms: dur,
                drag: seek_drag,
                enabled: can_seek,
                on_commit: {
                    let session = session.clone();
                    move |ms: i64| {
                        // Optimistic playhead — ui_eq skips position, so patch the signal now.
                        player.with_mut(|p| {
                            if let Some(st) = p.snapshot.state.as_mut() {
                                st.position_ms = ms;
                                st.timestamp = Some(now_unix_ms());
                            }
                        });
                        send(&session, UiCommand::Seek { position_ms: ms });
                    }
                },
            }
            span {
                class: if can_seek { "bar-seek-time end" } else { "bar-seek-time end is-locked" },
                title: if let Some(h) = np.skip_hint.as_deref() { h } else { "" },
                "{end_label}"
            }
        }
    }
}

#[component]
pub fn NowPlayingSheet(
    player: Signal<PlayerSlice>,
    seek_drag: Signal<Option<i64>>,
    vol_drag: Signal<Option<f64>>,
    tick: Signal<u64>,
    mut sheet: Signal<Sheet>,
    mut stack: Signal<NavStack>,
    nav: Signal<Nav>,
    context: Option<ContextPage>,
) -> Element {
    let session = use_context::<SessionHandle>();

    // Install cassette SVG once (template stays on window), then run the hub spin loop.
    use_effect(move || {
        let svg = cassette_svg_shell()
            .replace('\\', "\\\\")
            .replace('`', "\\`")
            .replace("${", "\\${");
        let _ = document::eval(&format!(
            r#"(() => {{
              window.__sloptifyCassetteSvg = `{svg}`;
              const root = document.querySelector('.np-full');
              if (root) root.focus();
              if (window.__sloptifyNpPaint) return;

              let last = performance.now();
              let angS = 0;
              let angT = 0;
              let hubS = null;
              let hubT = null;
              let wrap = null;

              const bind = (el) => {{
                if (!wrap || !wrap.isConnected) wrap = el.querySelector('.cass-svg-wrap');
                if (wrap && !wrap.dataset.ready && window.__sloptifyCassetteSvg) {{
                  wrap.innerHTML = window.__sloptifyCassetteSvg;
                  wrap.dataset.ready = '1';
                  hubS = hubT = null;
                }}
                if (wrap) {{
                  const nt = wrap.getAttribute('data-title') || '';
                  const na = wrap.getAttribute('data-artist') || '';
                  if (wrap._t !== nt) {{
                    wrap._t = nt;
                    const t = wrap.querySelector('#cass-title');
                    if (t) t.textContent = nt;
                  }}
                  if (wrap._a !== na) {{
                    wrap._a = na;
                    const a = wrap.querySelector('#cass-artist');
                    if (a) a.textContent = na;
                  }}
                }}
                // Re-query when hubs were orphaned by an SVG remount.
                if (!hubS || !hubS.isConnected || !hubT || !hubT.isConnected) {{
                  hubS = el.querySelector('#hub-supply');
                  hubT = el.querySelector('#hub-take');
                }}
              }};

              const paint = (now) => {{
                const el = document.querySelector('.np-full.np-tape');
                if (!el) {{
                  window.__sloptifyNpRaf = 0;
                  window.__sloptifyNpPaint = null;
                  return;
                }}
                bind(el);
                const dt = Math.min(0.05, (now - last) / 1000);
                last = now;

                // Timeline is BarSeek (same as the player bar) — only spin hubs here.
                const playing = el.dataset.playing === '1';
                if (playing) {{
                  angS = (angS + dt * (360 / 4.4)) % 360;
                  angT = (angT + dt * (360 / 2.75)) % 360;
                  if (hubS) hubS.setAttribute('transform', 'rotate(' + angS.toFixed(1) + ' 95 109)');
                  if (hubT) hubT.setAttribute('transform', 'rotate(' + angT.toFixed(1) + ' 205 109)');
                }}

                window.__sloptifyNpRaf = requestAnimationFrame(paint);
              }};

              window.__sloptifyNpPaint = paint;
              window.__sloptifyNpRaf = requestAnimationFrame(paint);
            }})()"#,
            svg = svg,
        ));
    });

    use_drop(|| {
        let _ = document::eval(
            r#"(() => {
              if (window.__sloptifyNpRaf) cancelAnimationFrame(window.__sloptifyNpRaf);
              window.__sloptifyNpRaf = 0;
              window.__sloptifyNpPaint = null;
            })()"#,
        );
    });

    let p = player();
    let liked = p.liked;
    let np = now_playing(&p.snapshot);
    let live_pos = seek_drag().unwrap_or_else(|| {
        p.snapshot
            .state
            .as_ref()
            .map(|s| s.live_position_ms(now_unix_ms()))
            .unwrap_or(np.pos)
    });
    let vol = vol_drag().unwrap_or(p.volume);
    let shuffle_on = p.snapshot.state.as_ref().map(|s| s.shuffle).unwrap_or(false);
    let repeat = p.snapshot.state.as_ref().map(|s| s.repeat).unwrap_or(0);
    let can_prev = np.can_skip_previous;
    let can_next = np.can_skip_next;
    let seeking = seek_drag().is_some();
    let playing_flag = if np.paused || seeking { "0" } else { "1" };
    let seeking_flag = if seeking { "1" } else { "0" };
    let tape_title = trunc(&np.title, 26);
    let tape_artist = trunc(
        np.skip_hint.as_deref().unwrap_or(np.artist.as_str()),
        34,
    );

    // Sync JS clock whenever transport position / pause / seek changes.
    let sync_pos = live_pos;
    let sync_dur = np.dur;
    let sync_playing = playing_flag;
    use_effect(move || {
        let _ = (sync_pos, sync_dur, sync_playing);
        let _ = document::eval(
            &format!(
                r#"(() => {{
                  const el = document.querySelector('.np-full.np-tape');
                  if (!el) return;
                  el.dataset.pos = '{pos}';
                  el.dataset.dur = '{dur}';
                  el.dataset.playing = '{playing}';
                  el.dataset.base = String(Date.now());
                }})()"#,
                pos = sync_pos,
                dur = sync_dur.max(1),
                playing = sync_playing,
            ),
        );
    });

    rsx! {
        div {
            class: "np-full np-tape",
            tabindex: "0",
            autofocus: true,
            role: "dialog",
            aria_label: "Now playing",
            aria_modal: "true",
            "data-playing": "{playing_flag}",
            "data-seeking": "{seeking_flag}",
            "data-pos": "{live_pos}",
            "data-dur": "{np.dur}",
            onkeydown: {
                let session = session.clone();
                move |e: KeyboardEvent| {
                    match e.key() {
                        Key::Escape => {
                            e.prevent_default();
                            sheet.set(Sheet::None);
                        }
                        Key::Character(c) if c == " " => {
                            e.prevent_default();
                            toggle_transport(player, &session);
                        }
                        Key::ArrowLeft => {
                            e.prevent_default();
                            if can_prev {
                                send(&session, UiCommand::Previous);
                            }
                        }
                        Key::ArrowRight => {
                            e.prevent_default();
                            if can_next {
                                send(&session, UiCommand::Next);
                            }
                        }
                        _ => {}
                    }
                }
            },
            // Solid dusk field only — blurred cover was a major WebKit cost.
            div { class: "np-full-bg-fallback" }
            div { class: "np-full-scrim" }
            div { class: "np-full-body np-tape-body",
                header { class: "np-top",
                    button {
                        class: "np-icon-btn", title: "Close (Esc)",
                        onclick: move |_| sheet.set(Sheet::None),
                        ChevronDown { class: "size-5" }
                    }
                    div { class: "np-top-mid",
                        if np.context_uri.is_some() {
                            button {
                                class: "np-context-btn",
                                title: "Go to {np.context}",
                                onclick: {
                                    let session = session.clone();
                                    let ctx = context.clone();
                                    let uri = np.context_uri.clone();
                                    let title = np.context.clone();
                                    let image = np.image.clone();
                                    move |_| {
                                        if let Some(uri) = uri.clone() {
                                            open_context_nav(
                                                &session, nav(), &mut stack, ctx.as_ref(),
                                                uri, Some(title.clone()), image.clone(),
                                            );
                                            sheet.set(Sheet::None);
                                        }
                                    }
                                },
                                "{np.context}"
                            }
                        } else {
                            span { class: "np-context-btn muted", "Now playing" }
                        }
                    }
                    span { class: "np-side-a", if playing_flag == "1" { "SIDE A · RUN" } else { "SIDE A" } }
                    button {
                        class: "np-icon-btn", title: "Queue",
                        onclick: {
                            let session = session.clone();
                            move |_| {
                                send(&session, UiCommand::RefreshQueue);
                                sheet.set(Sheet::Queue);
                            }
                        },
                        ListMusic { class: "size-5" }
                    }
                }

                div { class: "np-tape-stage",
                    div {
                        class: "cass-svg-wrap",
                        "data-title": "{tape_title}",
                        "data-artist": "{tape_artist}",
                    }

                    Cover {
                        url: np.image.clone(),
                        class: "np-tape-cover cover-soft".to_string(),
                    }

                    h1 { class: "np-tape-title", "{np.title}" }
                    p { class: "np-tape-artist", "{np.artist}" }

                    div { class: "np-links np-tape-links",
                        if let Some(uri) = np.album_uri.clone() {
                            {
                                let name = np.album_name.clone().unwrap_or_else(|| "Album".to_string());
                                let image = np.image.clone();
                                rsx! {
                                    button {
                                        class: "np-link",
                                        onclick: {
                                            let session = session.clone();
                                            let ctx = context.clone();
                                            move |_| {
                                                open_context_nav(
                                                    &session, nav(), &mut stack, ctx.as_ref(),
                                                    uri.clone(), Some(name.clone()), image.clone(),
                                                );
                                                sheet.set(Sheet::None);
                                            }
                                        },
                                        "Album"
                                    }
                                }
                            }
                        }
                        if let Some(uri) = np.artist_uri.clone() {
                            {
                                let name = np.artist.clone();
                                rsx! {
                                    button {
                                        class: "np-link",
                                        onclick: {
                                            let session = session.clone();
                                            let ctx = context.clone();
                                            move |_| {
                                                open_context_nav(
                                                    &session, nav(), &mut stack, ctx.as_ref(),
                                                    uri.clone(), Some(name.clone()), None,
                                                );
                                                sheet.set(Sheet::None);
                                            }
                                        },
                                        "Artist"
                                    }
                                }
                            }
                        }
                    }

                    div { class: "np-controls np-tape-controls",
                        div { class: "np-ctrl-rail left",
                            button {
                                class: if liked { "np-ctrl on like" } else { "np-ctrl" },
                                title: "Like",
                                onclick: { let session = session.clone(); move |_| send(&session, UiCommand::ToggleLike) },
                                Heart { class: if liked { "size-4 fill-current" } else { "size-4" } }
                            }
                            button {
                                class: if shuffle_on { "np-ctrl on" } else { "np-ctrl" },
                                title: "Shuffle",
                                onclick: { let session = session.clone(); move |_| send(&session, UiCommand::ToggleShuffle) },
                                Shuffle { class: "size-4" }
                            }
                            button {
                                class: if can_prev { "np-ctrl" } else { "np-ctrl is-disabled" },
                                title: if can_prev { "Previous" } else { "Can't go back" },
                                disabled: !can_prev,
                                onclick: { let session = session.clone(); move |_| send(&session, UiCommand::Previous) },
                                SkipBack { class: "size-5" }
                            }
                        }
                        button {
                            class: "np-play",
                            title: if np.paused { "Play" } else { "Pause" },
                            onclick: {
                                let session = session.clone();
                                move |_| toggle_transport(player, &session)
                            },
                            if np.paused { Play { class: "size-6 fill-current" } }
                            else { Pause { class: "size-6 fill-current" } }
                        }
                        div { class: "np-ctrl-rail right",
                            button {
                                class: if can_next { "np-ctrl" } else { "np-ctrl is-disabled" },
                                title: if can_next { "Next" } else if np.is_ad { "Can't skip ad" } else { "Can't skip" },
                                disabled: !can_next,
                                onclick: { let session = session.clone(); move |_| send(&session, UiCommand::Next) },
                                SkipForward { class: "size-5" }
                            }
                            button {
                                class: if repeat > 0 { "np-ctrl on" } else { "np-ctrl" },
                                title: "Repeat",
                                onclick: { let session = session.clone(); move |_| send(&session, UiCommand::CycleRepeat) },
                                if repeat == 2 { Repeat1 { class: "size-4" } } else { Repeat { class: "size-4" } }
                            }
                        }
                    }

                    BarSeek {
                        player,
                        seek_drag,
                        tick,
                        wide: true,
                    }

                    div { class: "np-vol np-tape-vol",
                        Volume2 { class: "size-3.5 shrink-0 opacity-60" }
                        div {
                            class: "apple-slider volume",
                            style: "--pct: {vol * 100.0}%",
                            div { class: "apple-slider-rail" }
                            div { class: "apple-slider-fill" }
                            div { class: "apple-slider-knob" }
                            Slider {
                                min: 0.0, max: 100.0, step: 0.25, value: vol * 100.0,
                                bind_value: vol_drag().is_none(),
                                oninput: {
                                    let session = session.clone();
                                    move |e: FormEvent| {
                                        if let Ok(v) = e.value().parse::<f64>() {
                                            let level = (v / 100.0).clamp(0.0, 1.0);
                                            vol_drag.set(Some(level));
                                            send(&session, UiCommand::SetVolume { level });
                                        }
                                    }
                                },
                                onchange: move |_| { vol_drag.set(None); }
                            }
                        }
                    }
                }
            }
        }
    }
}
