//! Sloptify desktop — GPL-3.0. Classic shell on Dioxus (Rust/UI stack) + CDP Web Player.

mod components;
#[cfg(target_os = "linux")]
mod mpris;
mod nav;
mod plugins;
mod session;
mod ui;

use dioxus::desktop::{Config, WindowBuilder};

fn main() {
    // Dioxus desktop embeds WebKit via wry — needs an X11/XWayland surface on Hyprland.
    prepare_x11_for_webview();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let window = WindowBuilder::new()
        .with_title("Sloptify")
        .with_inner_size(dioxus::desktop::LogicalSize::new(1380.0, 860.0))
        .with_min_inner_size(dioxus::desktop::LogicalSize::new(900.0, 600.0));

    let head = format!(
        r#"<meta name="color-scheme" content="dark" />
<link rel="preconnect" href="https://fonts.googleapis.com" />
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600&display=swap" rel="stylesheet" />
<style>{}</style>
<style>{}</style>
<style>{}</style>
<style>html,body,#main{{background:#000!important;color:#fff;margin:0;height:100%;overflow:hidden;font-family:"Inter",system-ui,"Segoe UI",sans-serif}}.dark{{color-scheme:dark;--background:#000;--primary:#fff;--primary-foreground:#000}}</style>"#,
        include_str!("../assets/tailwind.css"),
        include_str!("../assets/slider.css"),
        include_str!("../assets/shell.css"),
    );

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_window(window)
                // Kill default Window / View menu bar.
                .with_menu(None)
                .with_custom_head(head.into()),
        )
        .launch(ui::app);
}

/// wry/webkit2gtk only embeds into X11 windows. Under Hyprland that means XWayland.
fn prepare_x11_for_webview() {
    std::env::set_var("GDK_BACKEND", "x11");
    // Kill light GTK chrome on the embedded WebKit surface.
    if std::env::var_os("GTK_THEME").is_none() {
        std::env::set_var("GTK_THEME", "Adwaita:dark");
    }
    if std::env::var_os("GTK_APPLICATION_PREFER_DARK_THEME").is_none() {
        std::env::set_var("GTK_APPLICATION_PREFER_DARK_THEME", "1");
    }

    let current = std::env::var("DISPLAY").unwrap_or_default();
    let looks_private = current.is_empty()
        || current
            .trim_start_matches(':')
            .parse::<u16>()
            .map(|n| n >= 90)
            .unwrap_or(false);

    if looks_private {
        if let Some(real) = first_x11_display() {
            tracing::info!(%real, was = %current, "rebinding DISPLAY for Sloptify UI (XWayland)");
            std::env::set_var("DISPLAY", real);
        } else {
            std::env::set_var("DISPLAY", ":0");
        }
    }
}

fn first_x11_display() -> Option<String> {
    let dir = std::fs::read_dir("/tmp/.X11-unix").ok()?;
    let mut nums = Vec::new();
    for ent in dir.flatten() {
        let name = ent.file_name().to_string_lossy().into_owned();
        if let Some(rest) = name.strip_prefix('X') {
            if let Ok(n) = rest.trim_end_matches('_').parse::<u16>() {
                if n < 90 {
                    nums.push(n);
                }
            }
        }
    }
    nums.sort_unstable();
    nums.first().map(|n| format!(":{n}"))
}
