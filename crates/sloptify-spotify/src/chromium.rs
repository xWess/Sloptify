use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::time::sleep;

/// Private session bus for the embedded engine so Chromium never registers
/// `org.mpris.MediaPlayer2.chromium.*` on the user's real D-Bus (OSD / playerctl).
struct PrivateDbus {
    child: Child,
    address: String,
}

/// How the embedded Spotify Web Player is launched (Sloptify Android WebView equivalent).
///
/// Hidden mode runs on a private Xvfb display — never a Hyprland window/workspace.
/// Visible mode (login only) uses the real Wayland session.
///
/// Process / window identity is **Sloptify**, not Chromium — we exec through a private
/// wrapper so `ps`, task switchers and window classes never say "chromium".
#[derive(Debug, Clone)]
pub struct ChromiumConfig {
    pub binary: PathBuf,
    pub user_data_dir: PathBuf,
    pub debugging_port: u16,
    /// Show the window (needed once for Spotify login). After that we hide it.
    pub visible: bool,
    /// Wipe GPU/code caches on spawn. Disable when flipping login↔hidden so the
    /// session cookies stay warm and the bridge comes back quickly.
    pub prune_caches: bool,
}

impl Default for ChromiumConfig {
    fn default() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let share = home.join(".local/share/sloptify");
        Self {
            binary: ensure_sloptify_player_wrapper(&share),
            user_data_dir: player_data_dir(&share),
            debugging_port: 9222,
            // Like Android's hidden WebView: never show unless login is required.
            visible: false,
            prune_caches: true,
        }
    }
}

fn player_data_dir(share: &Path) -> PathBuf {
    let player = share.join("player");
    let legacy = share.join("chromium");
    // One-time rename so old installs keep cookies without a "chromium" folder name.
    if !player.exists() && legacy.exists() {
        let _ = std::fs::rename(&legacy, &player);
    }
    player
}

/// Private binary named `sloptify-player` so process lists / `/proc/exe` never say `chromium`.
///
/// Chromium re-execs from `/proc/self/exe`, so a shell `exec -a` is useless — we need a
/// real on-disk binary whose path is `…/sloptify-player`, plus the sibling `.pak` / libs
/// Chromium loads relative to that path.
fn ensure_sloptify_player_wrapper(share: &Path) -> PathBuf {
    let real = find_engine_binary();
    let Some(engine_dir) = real.parent().map(Path::to_path_buf) else {
        return real;
    };

    // Best case: hardlink beside the system binary (same dir ⇒ resources Just Work).
    let system_alias = engine_dir.join("sloptify-player");
    if same_inode(&system_alias, &real) || try_hard_link(&real, &system_alias) {
        return system_alias;
    }

    // User-writable chrome tree: copy/hardlink binary + symlink every sibling resource.
    let tree = share.join("engine");
    let alias = tree.join("sloptify-player");
    let _ = std::fs::create_dir_all(&tree);

    // Drop leftover shell wrapper / bare bin/ copy from earlier builds.
    let legacy_bin = share.join("bin/sloptify-player");
    if legacy_bin.is_file() {
        let _ = std::fs::remove_file(&legacy_bin);
    }

    if !same_inode(&alias, &real) {
        let _ = std::fs::remove_file(&alias);
        if !try_hard_link(&real, &alias) {
            if let Err(err) = std::fs::copy(&real, &alias) {
                tracing::warn!(%err, "could not install sloptify-player — using engine binary");
                return real;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&alias, std::fs::Permissions::from_mode(0o755));
            }
        }
    }

    if let Ok(entries) = std::fs::read_dir(&engine_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == "chromium"
                || name_str == "chromium-browser"
                || name_str == "sloptify-player"
                || name_str == "chrome"
                || name_str == "google-chrome"
            {
                continue;
            }
            let dest = tree.join(&name);
            if dest.exists() {
                continue;
            }
            let src = entry.path();
            #[cfg(unix)]
            {
                let _ = std::os::unix::fs::symlink(&src, &dest);
            }
            #[cfg(not(unix))]
            {
                let _ = std::fs::copy(&src, &dest);
            }
        }
    }

    if alias.is_file() {
        alias
    } else {
        real
    }
}

fn same_inode(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let Ok(ma) = std::fs::metadata(a) else {
            return false;
        };
        let Ok(mb) = std::fs::metadata(b) else {
            return false;
        };
        ma.dev() == mb.dev() && ma.ino() == mb.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}

fn try_hard_link(src: &Path, dest: &Path) -> bool {
    #[cfg(unix)]
    {
        std::fs::hard_link(src, dest).is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = (src, dest);
        false
    }
}

fn find_engine_binary() -> PathBuf {
    // Prefer the real binary: Omarchy's /usr/bin/chromium wrapper injects
    // --ozone-platform=wayland from chromium-flags.conf, which would steal the
    // window onto Hyprland and defeat the private Xvfb display.
    for candidate in [
        "/usr/lib/chromium/chromium",
        "/usr/lib64/chromium/chromium",
    ] {
        let p = PathBuf::from(candidate);
        if p.is_file() {
            return p;
        }
    }
    for candidate in [
        "chromium",
        "chromium-browser",
        "google-chrome-stable",
        "google-chrome",
        "chrome",
    ] {
        if let Ok(path) = which(candidate) {
            return path;
        }
    }
    PathBuf::from("chromium")
}

fn which(name: &str) -> Result<PathBuf> {
    let out = Command::new("which")
        .arg(name)
        .output()
        .with_context(|| format!("which {name}"))?;
    if !out.status.success() {
        bail!("{name} not found");
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() {
        bail!("{name} not found");
    }
    Ok(PathBuf::from(path))
}

/// Offscreen framebuffer — audio/CDP only; keep small to cut GPU/compositor RAM.
const OFFSCREEN_W: u32 = 800;
const OFFSCREEN_H: u32 = 600;
const DISK_CACHE_BYTES: u32 = 32 * 1024 * 1024; // 32 MiB

fn find_free_display() -> u16 {
    for d in 90..120 {
        if !Path::new(&format!("/tmp/.X{d}-lock")).exists() {
            return d;
        }
    }
    99
}

fn start_xvfb(display: u16) -> Result<Child> {
    let screen = format!("{OFFSCREEN_W}x{OFFSCREEN_H}x24");
    let child = Command::new("Xvfb")
        .arg(format!(":{display}"))
        .args(["-screen", "0", &screen])
        .arg("-ac")
        .args(["-nolisten", "tcp"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to start Xvfb — install xorg-server-xvfb")?;
    // Brief settle so the lock + socket exist before the player connects.
    std::thread::sleep(Duration::from_millis(200));
    Ok(child)
}

/// Drop disposable Chromium caches (keeps cookies / login). Shrinks profile + restart RSS.
fn prune_player_caches(user_data_dir: &Path) {
    let default = user_data_dir.join("Default");
    for rel in [
        "Cache",
        "Code Cache",
        "GPUCache",
        "Service Worker/CacheStorage",
    ] {
        let _ = std::fs::remove_dir_all(default.join(rel));
    }
    for rel in ["DawnCache", "GrShaderCache", "ShaderCache"] {
        let _ = std::fs::remove_dir_all(user_data_dir.join(rel));
    }
}

/// Shared Chromium flags to cap process count / V8 / disk cache for the hidden engine.
fn apply_memory_flags(cmd: &mut Command) {
    cmd.arg("--renderer-process-limit=2")
        .arg("--process-per-site")
        .arg(format!("--disk-cache-size={DISK_CACHE_BYTES}"))
        .arg("--js-flags=--max-old-space-size=384")
        .arg("--num-raster-threads=2")
        .arg("--disable-background-networking")
        .arg("--disable-component-update")
        .arg("--disable-domain-reliability")
        // HardwareMediaKeyHandling: OS media keys belong to Sloptify's MPRIS, not Chromium.
        .arg("--disable-features=TranslateUI,IsolateOrigins,site-per-process,BackForwardCache,HardwareMediaKeyHandling")
        .arg("--force-gpu-mem-available-mb=128")
        .arg("--enable-low-end-device-mode");
}

/// Brand PipeWire / Pulse / desktop entry so nothing says "Chromium".
fn apply_branding_env(cmd: &mut Command) {
    cmd.env("CHROME_DESKTOP", "sloptify.desktop")
        .env("PULSE_PROP_application.name", "Sloptify")
        .env("PULSE_PROP_application.id", "app.sloptify.desktop")
        .env("PULSE_PROP_media.role", "music")
        .env(
            "PIPEWIRE_PROPS",
            "{ node.name=sloptify application.name=Sloptify media.role=music }",
        );
}

/// Private D-Bus session for the engine — Chromium's MPRIS stays off the user bus.
fn start_private_dbus(share: &Path) -> Result<PrivateDbus> {
    let runtime = share.join("runtime");
    std::fs::create_dir_all(&runtime)
        .with_context(|| format!("create {}", runtime.display()))?;
    let sock = runtime.join(format!("dbus-{}", std::process::id()));
    let _ = std::fs::remove_file(&sock);
    let address = format!("unix:path={}", sock.display());
    let child = Command::new("dbus-daemon")
        .args([
            "--session",
            "--nofork",
            "--nopidfile",
            &format!("--address={address}"),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to start private dbus-daemon for Sloptify Web Player")?;
    for _ in 0..50 {
        if sock.exists() {
            return Ok(PrivateDbus { child, address });
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut orphan = PrivateDbus { child, address };
    orphan.stop();
    bail!("private dbus socket did not appear at {}", sock.display());
}

impl PrivateDbus {
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(path) = self.address.strip_prefix("unix:path=") {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Kill any previous Sloptify player so we own the CDP port and display mode.
fn kill_stale_sloptify_player(config: &ChromiumConfig) {
    let marker = config.user_data_dir.display().to_string();
    let _ = Command::new("pkill")
        .args(["-f", &format!("user-data-dir={marker}")])
        .status();
    // Legacy profile path from earlier builds.
    let legacy = config
        .user_data_dir
        .parent()
        .map(|p| p.join("chromium"))
        .unwrap_or_default();
    if legacy != config.user_data_dir && legacy.exists() {
        let _ = Command::new("pkill")
            .args(["-f", &format!("user-data-dir={}", legacy.display())])
            .status();
    }
    let _ = Command::new("pkill").args(["-x", "sloptify-player"]).status();
    std::thread::sleep(Duration::from_millis(300));
}

pub struct ChromiumSession {
    config: ChromiumConfig,
    xvfb: Option<Child>,
    /// Isolates Chromium MPRIS from the real session bus.
    dbus: Option<PrivateDbus>,
    child: Option<Child>,
    pub cdp_url: String,
    pub port: u16,
    pub visible: bool,
}

impl ChromiumSession {
    /// Start a fresh Sloptify Web Player (kills any previous instance for this profile).
    pub async fn start(config: ChromiumConfig) -> Result<Self> {
        kill_stale_sloptify_player(&config);
        Self::spawn(config).await
    }

    /// Switch between Xvfb (hidden) and real Wayland (login). Reuses the same profile.
    pub async fn relaunch_as(&mut self, visible: bool) -> Result<()> {
        if self.visible == visible && cdp_alive(&self.cdp_url).await {
            return Ok(());
        }
        self.stop();
        let mut cfg = self.config.clone();
        cfg.visible = visible;
        // Keep caches/cookies warm across the login hide flip.
        cfg.prune_caches = false;
        // Always clear stale processes before flipping display mode.
        kill_stale_sloptify_player(&cfg);
        *self = Self::spawn(cfg).await?;
        Ok(())
    }

    async fn spawn(config: ChromiumConfig) -> Result<Self> {
        let port = config.debugging_port;
        let cdp_url = format!("http://127.0.0.1:{port}");

        std::fs::create_dir_all(&config.user_data_dir)
            .with_context(|| format!("create {}", config.user_data_dir.display()))?;
        if config.prune_caches {
            prune_player_caches(&config.user_data_dir);
        }

        let mut xvfb = None;
        let share = config
            .user_data_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        // Hidden engine: private D-Bus so Chromium never registers MPRIS as "Chromium".
        // Visible login: keep the real session bus (portals / SSO / input method).
        let mut dbus = if config.visible {
            None
        } else {
            Some(start_private_dbus(&share)?)
        };

        let mut cmd = Command::new(&config.binary);
        cmd.arg(format!("--remote-debugging-port={port}"))
            .arg(format!("--user-data-dir={}", config.user_data_dir.display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-sync")
            .arg("--disable-session-crashed-bubble")
            .arg("--hide-crash-restore-bubble")
            // Sloptify WebView gets a user gesture context; Xvfb has none — allow media start.
            .arg("--autoplay-policy=no-user-gesture-required");
        apply_memory_flags(&mut cmd);
        apply_branding_env(&mut cmd);
        if let Some(bus) = dbus.as_ref() {
            cmd.env("DBUS_SESSION_BUS_ADDRESS", &bus.address);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        if config.visible {
            // One Wayland app window. No --class/--name with --app= (dual surfaces on Hyprland).
            cmd.arg("--app=https://accounts.spotify.com/login")
                .arg("--window-size=1100,800")
                .arg("--window-position=120,80");
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                cmd.arg("--ozone-platform=wayland");
            }
            tracing::info!(
                binary = %config.binary.display(),
                port,
                "starting Sloptify Web Player (visible — login)"
            );
        } else {
            let xvfb_display = find_free_display();
            xvfb = Some(start_xvfb(xvfb_display)?);
            cmd.arg("--class=Sloptify")
                .arg("--name=Sloptify")
                .arg("--app=https://open.spotify.com/")
                .env("DISPLAY", format!(":{xvfb_display}"))
                .env_remove("WAYLAND_DISPLAY")
                // Drop wrapper-injected Wayland preference if a shell wrapper is used.
                .env("OZONE_PLATFORM", "x11")
                .arg("--ozone-platform=x11")
                .arg("--ozone-platform-hint=x11")
                .arg(format!("--window-size={OFFSCREEN_W},{OFFSCREEN_H}"));
            tracing::info!(
                binary = %config.binary.display(),
                port,
                xvfb_display,
                "starting Sloptify Web Player on Xvfb (hidden)"
            );
        }

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(err) => {
                if let Some(mut bus) = dbus.take() {
                    bus.stop();
                }
                if let Some(mut xv) = xvfb.take() {
                    let _ = xv.kill();
                    let _ = xv.wait();
                }
                return Err(err).with_context(|| {
                    format!(
                        "failed to start Sloptify Web Player at {} — install chromium (engine only)",
                        config.binary.display()
                    )
                });
            }
        };

        let mut session = Self {
            visible: config.visible,
            config,
            xvfb,
            dbus,
            child: Some(child),
            cdp_url: cdp_url.clone(),
            port,
        };

        for _ in 0..50 {
            if cdp_alive(&cdp_url).await {
                return Ok(session);
            }
            sleep(Duration::from_millis(200)).await;
        }
        session.stop();
        bail!("Sloptify Web Player CDP did not become ready at {cdp_url}");
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(mut xv) = self.xvfb.take() {
            let _ = xv.kill();
            let _ = xv.wait();
        }
        if let Some(mut bus) = self.dbus.take() {
            bus.stop();
        }
    }
}

impl Drop for ChromiumSession {
    fn drop(&mut self) {
        // Leave the Web Player + Xvfb + private dbus running so Widevine session
        // survives UI restarts. Call `stop()` explicitly only on sign-out / full quit.
        let _ = self.child.take();
        let _ = self.xvfb.take();
        let _ = self.dbus.take();
    }
}

async fn cdp_alive(cdp_url: &str) -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .ok();
    let Some(client) = client else {
        return false;
    };
    client
        .get(format!("{cdp_url}/json/version"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}
