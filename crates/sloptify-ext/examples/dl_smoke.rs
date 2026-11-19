#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ext = sloptify_ext::default_extension_dir();
    let out = sloptify_ext::default_downloads_dir();
    let req = sloptify_ext::DownloadRequest {
        title: "Moonshine".into(),
        artist: "The Favors".into(),
        album: "The Dream".into(),
        track_id: "2XZ86GmAnSXwreUsCP9JGR".into(),
        isrc: None,
        duration_ms: 186599,
        quality: "cd".into(),
    };
    println!("resolving via {} …", ext.display());
    let done = sloptify_ext::download_track(&ext, &out, &req).await?;
    println!(
        "OK {} bytes → {} ({:?})",
        done.bytes,
        done.path.display(),
        done.resolved.provider
    );
    Ok(())
}
