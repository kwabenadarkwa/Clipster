use anyhow::{anyhow, bail, Context, Result};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::process::Command;

const VIRAL_PROMPT: &str = include_str!("../prompts/viral_clips.md");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub start: String,
    pub end: String,
    pub title: String,
    pub reason: String,
    pub score: u8,
    #[serde(default)]
    pub preview_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipSettings {
    pub clip_count: u8,
    pub min_duration: u16,
    pub max_duration: u16,
    pub model: String,
    pub temperature: f32,
}

impl Default for ClipSettings {
    fn default() -> Self {
        Self {
            clip_count: 5,
            min_duration: 30,
            max_duration: 60,
            model: "deepseek-chat".to_string(),
            temperature: 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineProgress {
    pub stage: String,
    pub message: String,
    pub clips: Vec<Clip>,
}

fn fmt_ts(seconds: f64) -> String {
    let h = (seconds / 3600.0) as u64;
    let m = ((seconds % 3600.0) / 60.0) as u64;
    let s = (seconds % 60.0) as u64;
    let ms = ((seconds % 1.0) * 1000.0) as u64;
    format!("{:02}:{:02}:{:02}.{:03}", h, m, s, ms)
}

fn parse_ts(ts: &str) -> f64 {
    let parts: Vec<&str> = ts.trim().split(':').collect();
    match parts.len() {
        3 => {
            let h: f64 = parts[0].parse().unwrap_or(0.0);
            let m: f64 = parts[1].parse().unwrap_or(0.0);
            let s: f64 = parts[2].parse().unwrap_or(0.0);
            h * 3600.0 + m * 60.0 + s
        }
        2 => {
            let m: f64 = parts[0].parse().unwrap_or(0.0);
            let s: f64 = parts[1].parse().unwrap_or(0.0);
            m * 60.0 + s
        }
        _ => 0.0,
    }
}

pub async fn download_video(url: &str, work_dir: &Path) -> Result<PathBuf> {
    info!("yt-dlp video: {}", url);
    let output = Command::new("yt-dlp")
        .arg("--format")
        .arg("bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best")
        .arg("--merge-output-format")
        .arg("mp4")
        .arg("--output")
        .arg(work_dir.join("source.%(ext)s").to_string_lossy().as_ref())
        .arg("--no-playlist")
        .arg("--progress")
        .arg(url)
        .output()
        .await
        .context("yt-dlp failed to spawn")?;

    if !output.status.success() {
        bail!(
            "yt-dlp failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    debug!("yt-dlp video stdout: {}", String::from_utf8_lossy(&output.stdout));

    let video = work_dir.join("source.mp4");
    if !video.exists() {
        let mut fallback = work_dir.to_path_buf();
        if let Some(entry) = std::fs::read_dir(work_dir)
            .context("work dir unreadable")?
            .filter_map(|e| e.ok())
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("source.")
            })
        {
            fallback = entry.path();
        }
        if !fallback.exists() {
            bail!("downloaded video not found after yt-dlp");
        }
        return Ok(fallback);
    }
    Ok(video)
}

pub async fn download_subtitles(url: &str, work_dir: &Path) -> Result<Option<PathBuf>> {
    info!("yt-dlp subs: {}", url);
    let status = Command::new("yt-dlp")
        .arg("--write-auto-subs")
        .arg("--sub-langs")
        .arg("en")
        .arg("--skip-download")
        .arg("--sub-format")
        .arg("vtt")
        .arg("--output")
        .arg(work_dir.join("source.%(ext)s").to_string_lossy().as_ref())
        .arg("--no-playlist")
        .arg(url)
        .output()
        .await
        .context("yt-dlp sub fetch failed")?;

    if !status.status.success() {
        return Ok(None);
    }

    for entry in std::fs::read_dir(work_dir).context("work dir unreadable")? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.ends_with(".vtt") && name_str.starts_with("source.") {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

pub fn parse_vtt(path: &Path) -> Result<String> {
    let raw = std::fs::read_to_string(path).context("vtt unreadable")?;
    let mut out = String::new();
    let mut in_cue = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            in_cue = false;
            continue;
        }
        if trimmed.starts_with("WEBVTT") || trimmed.starts_with("NOTE") || trimmed.starts_with("Kind:")
            || trimmed.starts_with("Language:") || trimmed.starts_with("STYLE") || trimmed.starts_with("REGION")
        {
            continue;
        }
        if trimmed.contains("-->") {
            in_cue = true;
            let parts: Vec<&str> = trimmed.split("-->").collect();
            if parts.len() == 2 {
                let start = parts[0].trim();
                let end = parts[1].trim().split_whitespace().next().unwrap_or("");
                out.push_str(&format!("[{} --> {}] ", normalize_ts(start), normalize_ts(end)));
            }
            continue;
        }
        if in_cue {
            let clean = stripped(trimmed);
            if !clean.is_empty() {
                out.push_str(&clean);
                out.push(' ');
            }
        }
    }
    Ok(out)
}

fn normalize_ts(ts: &str) -> String {
    let ts = ts.replace(',', ".");
    let parts: Vec<&str> = ts.split(':').collect();
    match parts.len() {
        3 => format!("{}:{}:{}", parts[0], parts[1], parts[2]),
        2 => format!("00:{}:{}", parts[0], parts[1]),
        _ => ts.to_string(),
    }
}

fn stripped(line: &str) -> String {
    let mut s = line.to_string();
    while s.starts_with('<') {
        if let Some(end) = s.find('>') {
            s = s[end + 1..].to_string();
        } else {
            break;
        }
    }
    while s.ends_with('>') {
        if let Some(start) = s.rfind('<') {
            s = s[..start].to_string();
        } else {
            break;
        }
    }
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

pub async fn find_viral_clips(
    transcript: &str,
    api_key: &str,
    settings: &ClipSettings,
    custom_context: &str,
) -> Result<Vec<Clip>> {
    info!("deepseek API call | transcript={} chars", transcript.len());
    let client = reqwest::Client::new();
    let user_content = format!(
        "Runtime settings:\n- clip_count: {}\n- min_duration_seconds: {}\n- max_duration_seconds: {}\n\nUser direction for this video:\n{}\n\nTranscript:\n{}",
        settings.clip_count,
        settings.min_duration,
        settings.max_duration,
        if custom_context.trim().is_empty() { "No extra direction." } else { custom_context.trim() },
        transcript
    );
    let body = serde_json::json!({
        "model": settings.model,
        "messages": [
            {"role": "system", "content": VIRAL_PROMPT},
            {"role": "user", "content": user_content}
        ],
        "temperature": settings.temperature,
        "max_tokens": 4096,
        "response_format": {"type": "json_object"}
    });

    let resp = client
        .post("https://api.deepseek.com/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .context("DeepSeek request failed")?;

    let status = resp.status();
    let text = resp.text().await.context("response body unreadable")?;
    if !status.is_success() {
        warn!("deepseek HTTP {}: {}", status, &text[..text.len().min(500)]);
        bail!("DeepSeek API error {}: {}", status, text);
    }
    debug!("deepseek response: {} bytes", text.len());

    let resp_json: serde_json::Value =
        serde_json::from_str(&text).context("invalid JSON from DeepSeek")?;

    let content = resp_json["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| anyhow!("missing content in DeepSeek response"))?;

    let mut clips = extract_clips(content)?;
    clips.truncate(settings.clip_count as usize);
    Ok(clips)
}

fn extract_clips(content: &str) -> Result<Vec<Clip>> {
    let mut content = content.trim().to_string();
    if content.starts_with("```") {
        content = content
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
            .to_string();
    }

    let start = content.find('[').ok_or_else(|| anyhow!("no array in LLM output"))?;
    let end = content.rfind(']').ok_or_else(|| anyhow!("no closing bracket"))?;
    let slice = &content[start..=end];

    let clips: Vec<Clip> = serde_json::from_str(slice).context("LLM JSON parse failed")?;
    if clips.is_empty() {
        bail!("LLM returned zero clips");
    }
    let mut sorted = clips;
    sorted.sort_by(|a, b| b.score.cmp(&a.score));
    Ok(sorted)
}

pub async fn cut_clip(
    video_path: &Path,
    clip: &Clip,
    output_path: &Path,
) -> Result<()> {
    info!("ffmpeg: {} -> {} [{}, {}]", video_path.display(), output_path.display(), clip.start, clip.end);
    let start_sec = parse_ts(&clip.start);
    let end_sec = parse_ts(&clip.end);
    let dur = end_sec - start_sec;
    if dur < 5.0 || dur > 120.0 {
        bail!("clip duration out of range: {}s", dur);
    }
    let dur_str = fmt_ts(dur);

    let crop_arg = if let Some(crop) = face_crop(video_path, clip).await {
        info!("face crop: {}", crop);
        crop
    } else {
        let detect = Command::new("ffmpeg")
        .arg("-ss").arg(&clip.start)
        .arg("-t").arg(&dur_str)
        .arg("-i").arg(video_path.to_string_lossy().as_ref())
        .arg("-vf").arg("cropdetect=24:16:0")
        .arg("-f").arg("null")
        .arg("-")
        .output()
        .await
        .context("ffmpeg cropdetect spawn failed")?;

        let stderr = String::from_utf8_lossy(&detect.stderr);
        extract_last_crop(&stderr).unwrap_or_else(|| {
            warn!("ffmpeg: no cropdetect result, using center crop fallback");
            "crop=ih*9/16:ih".to_string()
        })
    };
    debug!("ffmpeg: crop={}", crop_arg);

    let status = Command::new("ffmpeg")
        .arg("-ss").arg(&clip.start)
        .arg("-t").arg(&dur_str)
        .arg("-i").arg(video_path.to_string_lossy().as_ref())
        .arg("-vf").arg(format!("{},scale=1080:1920:force_original_aspect_ratio=decrease,pad=1080:1920:(ow-iw)/2:(oh-ih)/2", crop_arg))
        .arg("-c:v").arg("libx264")
        .arg("-preset").arg("fast")
        .arg("-crf").arg("23")
        .arg("-pix_fmt").arg("yuv420p")
        .arg("-movflags").arg("+faststart")
        .arg("-c:a").arg("aac")
        .arg("-b:a").arg("128k")
        .arg("-y")
        .arg(output_path.to_string_lossy().as_ref())
        .output()
        .await
        .context("ffmpeg cut spawn failed")?;

    if !status.status.success() {
        bail!(
            "ffmpeg cut failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
    Ok(())
}

async fn face_crop(video_path: &Path, clip: &Clip) -> Option<String> {
    let script = face_crop_script()?;
    let python = face_crop_python().unwrap_or_else(|| PathBuf::from("python3"));
    let output = Command::new(python)
        .arg(script)
        .arg(video_path.to_string_lossy().as_ref())
        .arg(&clip.start)
        .arg(&clip.end)
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        debug!("face crop unavailable: {}", String::from_utf8_lossy(&output.stderr));
        return None;
    }
    let crop = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if crop.starts_with("crop=") {
        Some(crop)
    } else {
        None
    }
}

fn face_crop_python() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    [
        cwd.join(".venv/bin/python"),
        cwd.join("src-tauri/.venv/bin/python"),
    ]
    .into_iter()
    .find(|p| p.exists())
}

fn face_crop_script() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    [
        cwd.join("scripts/face_crop.py"),
        cwd.join("src-tauri/scripts/face_crop.py"),
    ]
    .into_iter()
    .find(|p| p.exists())
}

pub async fn make_preview_clip(
    video_path: &Path,
    clip: &Clip,
    output_path: &Path,
) -> Result<()> {
    info!("ffmpeg preview: {} -> {} [{}, {}]", video_path.display(), output_path.display(), clip.start, clip.end);
    let start_sec = parse_ts(&clip.start);
    let end_sec = parse_ts(&clip.end);
    let dur = end_sec - start_sec;
    if dur < 5.0 || dur > 300.0 {
        bail!("preview duration out of range: {}s", dur);
    }
    let dur_str = fmt_ts(dur);

    let status = Command::new("ffmpeg")
        .arg("-ss").arg(&clip.start)
        .arg("-t").arg(&dur_str)
        .arg("-i").arg(video_path.to_string_lossy().as_ref())
        .arg("-vf").arg("scale=-2:720")
        .arg("-c:v").arg("libx264")
        .arg("-preset").arg("ultrafast")
        .arg("-crf").arg("28")
        .arg("-pix_fmt").arg("yuv420p")
        .arg("-movflags").arg("+faststart")
        .arg("-c:a").arg("aac")
        .arg("-b:a").arg("96k")
        .arg("-y")
        .arg(output_path.to_string_lossy().as_ref())
        .output()
        .await
        .context("ffmpeg preview spawn failed")?;

    if !status.status.success() {
        bail!(
            "ffmpeg preview failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
    Ok(())
}

fn extract_last_crop(stderr: &str) -> Option<String> {
    let mut last: Option<String> = None;
    for line in stderr.lines() {
        if let Some(pos) = line.find("crop=") {
            let crop = &line[pos..];
            let crop = crop.split_whitespace().next()?;
            last = Some(crop.to_string());
        }
    }
    last
}

pub fn cleanup_work_dir(work_dir: &Path) -> Result<()> {
    if work_dir.exists() {
        std::fs::remove_dir_all(work_dir).context("work dir cleanup failed")?;
    }
    std::fs::create_dir_all(work_dir).context("work dir recreate failed")?;
    Ok(())
}
