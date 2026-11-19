//! Small shared widgets.

use dioxus::prelude::*;
use icons::{House, Library, Search};

use super::helpers::trunc;

#[component]
pub fn Cover(url: Option<String>, #[props(default)] class: String) -> Element {
    let class = if class.is_empty() {
        "cover-soft".to_string()
    } else {
        class
    };
    if let Some(url) = url.filter(|u| !u.is_empty()) {
        rsx! {
            img {
                class: "{class}",
                src: "{url}",
                alt: "",
                loading: "lazy",
                draggable: "false",
            }
        }
    } else {
        rsx! {
            div { class: "{class} bg-[var(--surface)]" }
        }
    }
}

#[component]
pub fn NavBtn(
    label: &'static str,
    icon: &'static str,
    active: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let class = if active { "nav-item on" } else { "nav-item" };
    rsx! {
        button {
            class: "{class}",
            onclick: move |e| onclick.call(e),
            match icon {
                "search" => rsx! { Search { class: "size-4" } },
                "library" => rsx! { Library { class: "size-4" } },
                _ => rsx! { House { class: "size-4" } },
            }
            "{label}"
        }
    }
}

#[component]
pub fn ProfileDisc(
    name: String,
    image: Option<String>,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let initial = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    rsx! {
        button {
            class: "profile-disc",
            title: "{name}",
            onclick: move |e| onclick.call(e),
            if let Some(url) = image.filter(|u| !u.is_empty()) {
                img { src: "{url}", alt: "{name}" }
            } else {
                span { "{initial}" }
            }
        }
    }
}

pub fn tab_class(on: bool) -> &'static str {
    if on {
        "flex h-full flex-col items-center justify-center gap-1 text-[0.72rem] font-semibold text-foreground"
    } else {
        "flex h-full flex-col items-center justify-center gap-1 text-[0.72rem] font-semibold text-muted-foreground"
    }
}

pub fn trunc_label(s: &str) -> String {
    trunc(s, 40)
}

fn seek_ms_at(client_x: f64, left: f64, width: f64, dur_ms: i64) -> i64 {
    let w = width.max(1.0);
    let ratio = ((client_x - left) / w).clamp(0.0, 1.0);
    let ms = (ratio * dur_ms.max(0) as f64).round() as i64;
    ms.clamp(0, dur_ms.max(0))
}

/// Apple-style seek scrubber — pointer math only (no `<input type=range>`).
/// Chromium chokes on range inputs whose `max` is duration-in-ms (hundreds of thousands of steps).
///
/// Geometry is kept live via ResizeObserver + window resize so moving the window
/// across monitors / DPI scales does not leave a stale width (broken seek).
#[component]
pub fn SeekScrub(
    position_ms: i64,
    duration_ms: i64,
    mut drag: Signal<Option<i64>>,
    on_commit: EventHandler<i64>,
    #[props(default = true)] enabled: bool,
) -> Element {
    let mut held = use_signal(|| false);
    // Client-space origin + width of the hit target (CSS px).
    let mut left_px = use_signal(|| 0.0_f64);
    let mut width_px = use_signal(|| 1.0_f64);
    let id = use_hook(|| {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        format!(
            "seek-scrub-{}",
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )
    });

    let dur = duration_ms.max(0);
    let display = drag().unwrap_or(position_ms).clamp(0, dur.max(0));
    let pct = if dur > 0 {
        (display as f64 / dur as f64 * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };
    let class = if !enabled {
        "apple-slider seek is-disabled"
    } else if held() || drag().is_some() {
        "apple-slider seek is-scrubbing"
    } else {
        "apple-slider seek"
    };

    rsx! {
        div {
            id: "{id}",
            class: "{class}",
            style: "--pct: {pct}%",
            role: "slider",
            tabindex: if enabled { "0" } else { "-1" },
            aria_valuemin: 0_i64,
            aria_valuemax: dur,
            aria_valuenow: display,
            aria_label: "Seek",
            aria_disabled: if enabled { "false" } else { "true" },
            onmounted: {
                let id = id.clone();
                move |_| {
                    let id = id.clone();
                    spawn(async move {
                        let mut eval = document::eval(&format!(
                            r#"(() => {{
                              const el = document.getElementById('{id}');
                              if (!el) return;
                              const send = () => {{
                                const r = el.getBoundingClientRect();
                                dioxus.send([r.left, Math.max(1, r.width)]);
                              }};
                              send();
                              if (el.__sloptifyRo) try {{ el.__sloptifyRo.disconnect(); }} catch (_) {{}}
                              const ro = new ResizeObserver(send);
                              ro.observe(el);
                              el.__sloptifyRo = ro;
                              const onWin = () => send();
                              window.addEventListener('resize', onWin);
                              if (window.visualViewport) {{
                                window.visualViewport.addEventListener('resize', onWin);
                                window.visualViewport.addEventListener('scroll', onWin);
                              }}
                              // Keep the channel open so resize events keep flowing.
                              return new Promise(() => {{}});
                            }})()"#
                        ));
                        while let Ok(pair) = eval.recv::<(f64, f64)>().await {
                            left_px.set(pair.0);
                            width_px.set(pair.1.max(1.0));
                        }
                    });
                }
            },
            onpointerdown: {
                let id = id.clone();
                move |e: Event<PointerData>| {
                    if !enabled {
                        return;
                    }
                    e.prevent_default();
                    held.set(true);
                    let pid = e.data().pointer_id();
                    let client_x = e.client_coordinates().x;
                    // Optimistic hit with last geometry (usually fresh via RO).
                    drag.set(Some(seek_ms_at(client_x, left_px(), width_px(), dur)));
                    let id_cap = id.clone();
                    let _ = document::eval(&format!(
                        r#"(() => {{
                          const el = document.getElementById('{id_cap}');
                          if (!el) return;
                          try {{ el.setPointerCapture({pid}); }} catch (_) {{}}
                        }})()"#
                    ));
                    // Re-measure immediately — critical after a monitor / DPI move.
                    spawn({
                        let id = id.clone();
                        async move {
                            let mut eval = document::eval(&format!(
                                r#"(() => {{
                                  const el = document.getElementById('{id}');
                                  if (!el) return [0, 1];
                                  const r = el.getBoundingClientRect();
                                  return [r.left, Math.max(1, r.width)];
                                }})()"#
                            ));
                            if let Ok(pair) = eval.recv::<(f64, f64)>().await {
                                left_px.set(pair.0);
                                width_px.set(pair.1.max(1.0));
                                drag.set(Some(seek_ms_at(client_x, pair.0, pair.1, dur)));
                            }
                        }
                    });
                }
            },
            onpointermove: move |e: Event<PointerData>| {
                if !enabled || !held() {
                    return;
                }
                drag.set(Some(seek_ms_at(
                    e.client_coordinates().x,
                    left_px(),
                    width_px(),
                    dur,
                )));
            },
            onpointerup: move |e: Event<PointerData>| {
                if !enabled || !held() {
                    return;
                }
                held.set(false);
                let ms = seek_ms_at(e.client_coordinates().x, left_px(), width_px(), dur);
                on_commit.call(ms);
                drag.set(None);
            },
            onpointercancel: move |_| {
                held.set(false);
                drag.set(None);
            },
            div { class: "apple-slider-rail" }
            div { class: "apple-slider-fill" }
            div { class: "apple-slider-knob" }
        }
    }
}
