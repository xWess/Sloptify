//! Resolve `download.source` via the official `myspoti-ext` runner, then fetch the file.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::ExtError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub track_id: String,
    #[serde(default)]
    pub isrc: Option<String>,
    #[serde(default)]
    pub duration_ms: i64,
    /// `hi-res` | `cd` | `small`
    #[serde(default = "default_quality")]
    pub quality: String,
}

fn default_quality() -> String {
    "cd".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedFile {
    pub url: String,
    pub format: String,
    pub bit_depth: Option<i64>,
    pub sample_rate: Option<i64>,
    pub provider: Option<String>,
    pub isrc_matched: bool,
    pub size_hint: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedTrack {
    pub path: PathBuf,
    pub resolved: ResolvedFile,
    pub bytes: u64,
}

pub fn default_extension_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share/sloptify/extensions/audio.lossless.flac")
}

pub fn default_downloads_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".local/share/sloptify/downloads")
}

fn find_myspoti_ext() -> Result<PathBuf, ExtError> {
    if let Ok(p) = std::env::var("SLOPTIFY_EXT_CLI") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Ok(pb);
        }
    }
    let home_bin =
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/bin/myspoti-ext");
    if home_bin.is_file() {
        return Ok(home_bin);
    }
    which("myspoti-ext").ok_or_else(|| {
        ExtError::Msg(
            "myspoti-ext not found (set SLOPTIFY_EXT_CLI or install ~/.local/bin/myspoti-ext)".into(),
        )
    })
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Ask the installed Lossless FLAC extension for download candidates.
pub async fn resolve_download(
    extension_dir: &Path,
    req: &DownloadRequest,
) -> Result<Vec<ResolvedFile>, ExtError> {
    let cli = find_myspoti_ext()?;
    if !extension_dir.join("manifest.json").is_file() {
        return Err(ExtError::Msg(format!(
            "extension not installed at {}",
            extension_dir.display()
        )));
    }

    let mut args = vec![
        "run".into(),
        extension_dir.display().to_string(),
        "download-source".into(),
        "--title".into(),
        req.title.clone(),
        "--artist".into(),
        req.artist.clone(),
        "--quality".into(),
        req.quality.clone(),
    ];
    if !req.album.is_empty() {
        args.push("--album".into());
        args.push(req.album.clone());
    }
    if !req.track_id.is_empty() {
        args.push("--id".into());
        args.push(req.track_id.clone());
    }
    if let Some(isrc) = &req.isrc {
        if !isrc.is_empty() {
            args.push("--isrc".into());
            args.push(isrc.clone());
        }
    }
    if req.duration_ms > 0 {
        args.push("--duration".into());
        args.push(req.duration_ms.to_string());
    }

    let output = Command::new(&cli)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| ExtError::Msg(format!("myspoti-ext: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        let msg = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("myspoti-ext failed");
        return Err(ExtError::Msg(msg.to_string()));
    }

    let files = parse_cli_files(&stdout);
    if files.is_empty() {
        let combined = format!("{stdout}\n{stderr}");
        if combined.contains("no file") || combined.contains("— ") {
            return Err(ExtError::Msg("no downloadable file for this track".into()));
        }
        return Err(ExtError::Msg(format!(
            "could not parse myspoti-ext output:\n{}",
            stdout.chars().take(400).collect::<String>()
        )));
    }
    Ok(files)
}

fn parse_cli_files(stdout: &str) -> Vec<ResolvedFile> {
    let mut out = Vec::new();
    let lines: Vec<&str> = stdout.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        // e.g. "flac 24-bit 48000 Hz Lossless FLAC · NetEase Hi-Res isrcMatched=true size=40673029"
        if line.starts_with("http://") || line.starts_with("https://") {
            out.push(ResolvedFile {
                url: line.split_whitespace().next().unwrap_or(line).to_string(),
                format: guess_format(line),
                bit_depth: None,
                sample_rate: None,
                provider: None,
                isrc_matched: false,
                size_hint: None,
            });
            i += 1;
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.first().map(|p| matches!(*p, "flac" | "m4a" | "mp3" | "dash-flac")).unwrap_or(false)
            && i + 1 < lines.len()
        {
            let meta = line;
            let next = lines[i + 1].trim();
            if next.starts_with("http://") || next.starts_with("https://") {
                let url = next
                    .split_whitespace()
                    .next()
                    .unwrap_or(next)
                    .to_string();
                let format = parts[0].to_string();
                let bit_depth = parts
                    .iter()
                    .find(|p| p.ends_with("-bit"))
                    .and_then(|p| p.trim_end_matches("-bit").parse().ok());
                let sample_rate = parts
                    .iter()
                    .position(|p| *p == "Hz")
                    .and_then(|idx| parts.get(idx.saturating_sub(1)))
                    .and_then(|p| p.parse().ok());
                let isrc_matched = meta.contains("isrcMatched=true");
                let size_hint = meta
                    .split_whitespace()
                    .find_map(|p| p.strip_prefix("size=").and_then(|s| s.parse().ok()));
                let provider = {
                    let start = meta.find("-bit ").map(|i| i + 5).unwrap_or(0);
                    let end = meta.find(" isrcMatched=").unwrap_or(meta.len());
                    let mid = meta.get(start..end).unwrap_or("");
                    let mid = mid
                        .trim()
                        .trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ')
                        .trim_start_matches("Hz ")
                        .trim();
                    if mid.is_empty() {
                        None
                    } else {
                        Some(mid.to_string())
                    }
                };
                out.push(ResolvedFile {
                    url,
                    format,
                    bit_depth,
                    sample_rate,
                    provider,
                    isrc_matched,
                    size_hint,
                });
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn guess_format(url: &str) -> String {
    let lower = url.to_ascii_lowercase();
    if lower.contains(".flac") {
        "flac".into()
    } else if lower.contains(".m4a") || lower.contains(".mp4") {
        "m4a".into()
    } else if lower.contains(".mp3") {
        "mp3".into()
    } else {
        "flac".into()
    }
}

fn safe_filename(title: &str, artist: &str, format: &str) -> String {
    let raw = format!("{artist} - {title}.{format}");
    raw.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .chars()
        .take(180)
        .collect()
}

/// Resolve + download the best file into `out_dir`.
pub async fn download_track(
    extension_dir: &Path,
    out_dir: &Path,
    req: &DownloadRequest,
) -> Result<DownloadedTrack, ExtError> {
    let files = resolve_download(extension_dir, req).await?;
    let best = files
        .into_iter()
        .next()
        .ok_or_else(|| ExtError::Msg("no files".into()))?;

    std::fs::create_dir_all(out_dir)?;
    let name = safe_filename(&req.title, &req.artist, &best.format);
    let path = out_dir.join(name);

    let client = reqwest::Client::builder()
        .user_agent("Sloptify-Desktop/0.1 (myspoti-ext download)")
        .timeout(Duration::from_secs(180))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| ExtError::Msg(e.to_string()))?;

    let bytes = client
        .get(&best.url)
        .send()
        .await
        .map_err(|e| ExtError::Msg(format!("download: {e}")))?
        .error_for_status()
        .map_err(|e| ExtError::Msg(format!("download: {e}")))?
        .bytes()
        .await
        .map_err(|e| ExtError::Msg(format!("download body: {e}")))?;

    if bytes.len() < 64 {
        return Err(ExtError::Msg("downloaded file too small".into()));
    }
    // FLAC magic
    if best.format == "flac" && !bytes.starts_with(b"fLaC") {
        return Err(ExtError::Msg("response is not a FLAC file".into()));
    }

    let tmp = path.with_extension(format!(
        "{}.part",
        path.extension().and_then(|e| e.to_str()).unwrap_or("tmp")
    ));
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(ExtError::Io)?;
    tokio::fs::rename(&tmp, &path)
        .await
        .map_err(ExtError::Io)?;

    Ok(DownloadedTrack {
        bytes: bytes.len() as u64,
        path,
        resolved: best,
    })
}
