# Sloptify (desktop)

GPL-3.0 Rust desktop client for your Spotify account, Arch-first.

Derived from [Sloptify / Myspoti](https://github.com/kota-zo/myspoti): Chromium runs the official Spotify Web Player (Widevine); Sloptify draws the UI and drives playback through CDP and the shared `player-bridge.js`. See [NOTICE](NOTICE) and [LICENSE](LICENSE).

## Why Chromium runs at all

Sloptify does **not** decode Spotify audio itself. Like the Android app’s hidden WebView, Chromium runs the official Web Player (Widevine DRM) on a private **Xvfb** display — no desktop window. You only see the Sloptify UI.

## Requirements

- Rust (stable)
- `chromium`, `xdotool`, `webkit2gtk-4.1`, `gtk3` (`pacman -S chromium xdotool webkit2gtk-4.1 gtk3`)

## Run

```bash
cargo run -p sloptify --release
# or after build:
./target/release/sloptify
```

1. Chromium + bridge start on Xvfb in the background.
2. Sign in once if needed (**Log in with Spotify**).
3. Home / Search / Library drive playback through CDP.

## Workspace

| Crate | Role |
|-------|------|
| `sloptify` | Dioxus + Rust/UI desktop shell |
| `sloptify-spotify` | Chromium CDP host + player bridge |
| `sloptify-ext` | Plugin marketplace + extension manifests (JS sandbox next) |

## Plugin marketplace

Account menu → **Plugin marketplace**. Curated packages install into
`~/.local/share/sloptify/plugins/<id>/` with `manifest.json` + `main.js`.

Contributors can:

1. Copy `crates/sloptify-ext/examples/plugin-hello/`
2. Change `id` / `name` / actions in the manifest
3. Drop the folder under `~/.local/share/sloptify/plugins/<id>/`
4. Open the marketplace and enable it

Host hooks (`player.trackChanged`, `ui.theme`, …) are declared in the manifest;
the in-process JS runtime is next. Enablement and discovery already work so
packages can ship against the stable folder shape.

## Install

- **Releases:** [AppImage](https://github.com/xWess/Sloptify/releases) (Linux) and Windows `.exe` / `.zip`
- **Arch (local):** `cd pkgbuild && makepkg -si`
- **AUR:** `aur/` PKGBUILD (submit / install once published)

Media keys and OSD use Sloptify’s own MPRIS identity — not Chromium.

## Disclaimer

Same spirit as upstream Sloptify: you use your own Spotify account; ads and ToS are Spotify’s. This app does not circumvent DRM.
