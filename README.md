# Sloptify

A desktop Spotify client for Linux and Windows. GPL-3.0.

Sloptify provides its own UI and controls. Playback runs through the official Spotify Web Player in a background Chromium process (Widevine). Sign in with your Spotify account.

## Install

**Releases:** [AppImage](https://github.com/xWess/Sloptify/releases) (Linux) and Windows `.exe` / `.zip`

**Arch (local package):**

```bash
cd pkgbuild
makepkg -si
```

An AUR `PKGBUILD` lives under `aur/`.

## Build from source

Dependencies (Arch):

```bash
pacman -S rust chromium xdotool webkit2gtk-4.1 gtk3
```

```bash
cargo run -p sloptify --release
```

On first launch, sign in if prompted. After that, the Web Player stays signed in in the background.

## Workspace

| Crate | Role |
|-------|------|
| `sloptify` | Desktop UI |
| `sloptify-spotify` | Web Player host (CDP) |
| `sloptify-ext` | Plugin marketplace |

## Plugins

Account menu → Plugin marketplace. Packages install to `~/.local/share/sloptify/plugins/<id>/` (`manifest.json` + `main.js`).

Example scaffold: `crates/sloptify-ext/examples/plugin-hello/`.

## License

GPL-3.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

You use your own Spotify account. Ads and terms are Spotify's. This app does not circumvent DRM.
