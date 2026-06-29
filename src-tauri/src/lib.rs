mod pipeline;

use pipeline::{Clip, ClipSettings, PipelineProgress};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};
use log::{info, warn, error};

fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 32 => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .replace("  ", " ")
        .replace(" ", "_")
        .chars()
        .take(80)
        .collect()
}

fn work_dir(app: &AppHandle) -> PathBuf {
    let mut p = app
        .path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    p.push("clipster-work");
    p
}

fn cache_url_path(work: &std::path::Path) -> PathBuf {
    work.join("url.txt")
}

fn read_cached_url(work: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(cache_url_path(work)).ok()
}

fn write_cached_url(work: &std::path::Path, url: &str) -> Result<(), String> {
    std::fs::write(cache_url_path(work), url).map_err(|e| e.to_string())
}

fn cached_video(work: &std::path::Path) -> Option<PathBuf> {
    std::fs::read_dir(work).ok()?.filter_map(|e| e.ok()).find_map(|e| {
        let path = e.path();
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        let name = path.file_name()?.to_string_lossy();
        if name.starts_with("source.") && matches!(ext.as_str(), "mp4" | "mkv" | "webm" | "mov" | "m4v") {
            Some(path)
        } else {
            None
        }
    })
}

fn cached_subs(work: &std::path::Path) -> Option<PathBuf> {
    std::fs::read_dir(work).ok()?.filter_map(|e| e.ok()).find_map(|e| {
        let path = e.path();
        let name = path.file_name()?.to_string_lossy();
        if name.starts_with("source.") && name.ends_with(".vtt") {
            Some(path)
        } else {
            None
        }
    })
}

fn emit(app: &AppHandle, stage: &str, message: &str) {
    let _ = app.emit(
        "pipeline-progress",
        PipelineProgress {
            stage: stage.to_string(),
            message: message.to_string(),
            clips: vec![],
        },
    );
}

fn emit_clips(app: &AppHandle, stage: &str, message: &str, clips: &[Clip]) {
    let _ = app.emit(
        "pipeline-progress",
        PipelineProgress {
            stage: stage.to_string(),
            message: message.to_string(),
            clips: clips.to_vec(),
        },
    );
}

#[tauri::command]
async fn run_pipeline(
    app: AppHandle,
    url: String,
    output_dir: String,
    force_download: Option<bool>,
    settings: Option<ClipSettings>,
    custom_context: Option<String>,
    cut_final: Option<bool>,
) -> Result<Vec<Clip>, String> {
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .map_err(|_| "DEEPSEEK_API_KEY not set. Add it to .env or export it.".to_string())?;

    let force_download = force_download.unwrap_or(true);
    let settings = settings.unwrap_or_default();
    let custom_context = custom_context.unwrap_or_default();
    let cut_final = cut_final.unwrap_or(false);
    info!("pipeline start | url={} | out={} | force_download={} | settings={:?}", url, output_dir, force_download, settings);

    let work = work_dir(&app);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    info!("work dir: {:?}", work);

    let cached_url_matches = read_cached_url(&work).as_deref() == Some(url.as_str());
    let can_reuse_cache = !force_download && cached_url_matches && cached_video(&work).is_some() && cached_subs(&work).is_some();

    let video = if can_reuse_cache {
        let video = cached_video(&work).expect("checked above");
        emit(&app, "cache", "Using cached source video. Re-analyzing only…");
        info!("cache hit: {:?}", video);
        video
    } else {
        emit(&app, "download", "Downloading video via yt-dlp…");
        info!("cache miss: refreshing work dir");
        pipeline::cleanup_work_dir(&work).map_err(|e| e.to_string())?;
        write_cached_url(&work, &url)?;
        info!("yt-dlp: downloading video");
        let video = pipeline::download_video(&url, &work)
            .await
            .map_err(|e| {
                error!("download failed: {e}");
                format!("download: {e}")
            })?;
        info!("video downloaded: {:?}", video);

        emit(&app, "subs", "Fetching auto-subs via yt-dlp…");
        info!("yt-dlp: fetching auto-subs");
        let subs = pipeline::download_subtitles(&url, &work)
            .await
            .map_err(|e| {
                error!("sub fetch failed: {e}");
                format!("subs: {e}")
            })?;

        match subs {
            Some(p) => info!("subs downloaded: {:?}", p),
            None => {
                warn!("no auto-subs available");
                emit(&app, "error", "No captions found for this video. Try a different video or add Whisper fallback later.");
                return Err("No captions found for this video. Clipster currently needs YouTube captions.".into());
            }
        }
        video
    };

    let subs_path = match cached_subs(&work) {
        Some(p) => {
            info!("subs found: {:?}", p);
            p
        }
        None => {
            warn!("no auto-subs available");
            emit(&app, "error", "No captions found for this video. Try a different video or add Whisper fallback later.");
            return Err("No captions found for this video. Clipster currently needs YouTube captions.".into());
        }
    };

    emit(&app, "transcript", "Parsing captions…");
    info!("parsing VTT: {:?}", subs_path);
    let transcript = pipeline::parse_vtt(&subs_path).map_err(|e| {
        error!("vtt parse failed: {e}");
        format!("vtt: {e}")
    })?;
    info!("transcript length: {} chars", transcript.len());

    if transcript.len() < 500 {
        warn!("transcript too short ({} chars), aborting", transcript.len());
        return Err("transcript too short — likely no speech detected".into());
    }

    emit(&app, "llm", "Sending transcript to DeepSeek for viral clip analysis…");
    info!("deepseek: sending transcript ({} chars) to api", transcript.len());
    let mut clips = pipeline::find_viral_clips(&transcript, &api_key, &settings, &custom_context)
        .await
        .map_err(|e| {
            error!("deepseek failed: {e}");
            format!("llm: {e}")
        })?;
    info!("deepseek returned {} clips", clips.len());
    for (i, c) in clips.iter().enumerate() {
        info!("  clip {}: [{} -> {}] score={} | {}", i + 1, c.start, c.end, c.score, c.title);
    }

    let previews_dir = work.join("previews");
    if previews_dir.exists() {
        std::fs::remove_dir_all(&previews_dir).ok();
    }
    std::fs::create_dir_all(&previews_dir).map_err(|e| e.to_string())?;
    emit(&app, "preview", "Building lightweight preview clips…");
    for (i, clip) in clips.iter_mut().enumerate() {
        let safe_title = sanitize_filename(&clip.title);
        let fname = if safe_title.is_empty() {
            format!("preview_{:02}.mp4", i + 1)
        } else {
            format!("preview_{:02}_{}.mp4", i + 1, safe_title)
        };
        let dest = previews_dir.join(fname);
        match pipeline::make_preview_clip(&video, clip, &dest).await {
            Ok(()) => clip.preview_path = Some(dest.to_string_lossy().to_string()),
            Err(e) => warn!("preview {} failed: {e}", i + 1),
        }
    }

    if !cut_final {
        emit_clips(&app, "preview", &format!("Found {} clips. Review previews, then cut.", clips.len()), &clips);
        return Ok(clips);
    }

    emit_clips(&app, "clips", &format!("Found {} viral clips. Cutting…", clips.len()), &clips);

    let out = PathBuf::from(&output_dir);
    std::fs::create_dir_all(&out).map_err(|e| {
        error!("output dir create failed: {e}");
        format!("output dir: {e}")
    })?;
    info!("output dir ready: {:?}", out);

    let total = clips.len();
    for (i, clip) in clips.iter().enumerate() {
        emit(
            &app,
            "cutting",
            &format!("Clipping {} of {}: {}", i + 1, total, clip.title),
        );
        info!("ffmpeg: cutting clip {} of {} [{} -> {}]", i + 1, total, clip.start, clip.end);
        let safe_title = sanitize_filename(&clip.title);
        let fname = if safe_title.is_empty() {
            format!("clip_{:02}_score{}.mp4", i + 1, clip.score)
        } else {
            format!("{}_{}.mp4", safe_title, clip.score)
        };
        let dest = out.join(&fname);
        match pipeline::cut_clip(&video, clip, &dest).await {
            Ok(()) => info!("  ffmpeg done: {:?}", dest),
            Err(e) => error!("  ffmpeg clip {} failed: {e}", i + 1),
        }
    }

    info!("pipeline complete | clips={} | out={}", clips.len(), output_dir);
    emit(&app, "done", &format!("All clips saved to {}", output_dir));
    Ok(clips)
}

#[tauri::command]
async fn cut_cached_clips(
    app: AppHandle,
    output_dir: String,
    clips: Vec<Clip>,
) -> Result<Vec<Clip>, String> {
    let work = work_dir(&app);
    let video = cached_video(&work).ok_or_else(|| "No cached source video found. Run analysis first.".to_string())?;
    let out = PathBuf::from(&output_dir);
    std::fs::create_dir_all(&out).map_err(|e| format!("output dir: {e}"))?;
    let total = clips.len();
    for (i, clip) in clips.iter().enumerate() {
        emit(&app, "cutting", &format!("Clipping {} of {}: {}", i + 1, total, clip.title));
        let safe_title = sanitize_filename(&clip.title);
        let fname = if safe_title.is_empty() {
            format!("clip_{:02}_score{}.mp4", i + 1, clip.score)
        } else {
            format!("{}_{}.mp4", safe_title, clip.score)
        };
        let dest = out.join(fname);
        match pipeline::cut_clip(&video, clip, &dest).await {
            Ok(()) => info!("ffmpeg done: {:?}", dest),
            Err(e) => error!("ffmpeg clip {} failed: {e}", i + 1),
        }
    }
    emit(&app, "done", &format!("All clips saved to {}", output_dir));
    Ok(clips)
}

#[tauri::command]
fn open_output_folder(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn load_env() {
    let _ = dotenvy::dotenv();
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .format_timestamp_secs()
    .init();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    load_env();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![run_pipeline, cut_cached_clips, open_output_folder])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
