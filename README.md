# Clipster

Clipster is a local Tauri desktop app that takes a YouTube URL, downloads the video and captions, asks DeepSeek to find high-potential short-form clips, generates playable previews, then cuts the selected clips with ffmpeg.

## What It Does

| Step | Tool |
|---|---|
| Download YouTube video | `yt-dlp` |
| Fetch captions | YouTube auto-subs via `yt-dlp` |
| Pick viral moments | DeepSeek chat API |
| Generate previews | `ffmpeg` |
| Final 9:16 clips | `ffmpeg` + optional MediaPipe face crop |
| Desktop UI | Tauri + React + TypeScript |

## Requirements

| Requirement | Install |
|---|---|
| Node + pnpm | `corepack enable` or install pnpm directly |
| Rust | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| ffmpeg | `brew install ffmpeg` |
| yt-dlp | `brew install yt-dlp` |
| DeepSeek API key | Put in `src-tauri/.env` |
| Python 3.12 + MediaPipe | Optional, only needed for face crop |

## Setup

From the project root:

```bash
./scripts/setup.sh
```

Or through pnpm:

```bash
pnpm setup
```

The setup script installs/checks:

| Thing | Installed By Script |
|---|---|
| `ffmpeg` | Yes, via Homebrew |
| `yt-dlp` | Yes, via Homebrew |
| Rust | Yes, via rustup if missing |
| pnpm dependencies | Yes |
| Python 3.12 | Yes, via Homebrew |
| MediaPipe venv | Yes, at `src-tauri/.venv` |

Then edit `src-tauri/.env`:

```bash
DEEPSEEK_API_KEY=your-deepseek-key-here
```

That key is the only manual step.

## Run

Development mode:

```bash
RUST_LOG=debug pnpm tauri dev
```

Normal logging:

```bash
pnpm tauri dev
```

Build a desktop app bundle:

```bash
pnpm tauri build
```

## Basic Usage

| Step | Action |
|---|---|
| 1 | Paste a YouTube link. |
| 2 | Choose an output folder. |
| 3 | Optionally add AI direction for this specific video. |
| 4 | Adjust settings if needed. |
| 5 | Click **Find Viral Clips**. |
| 6 | Watch generated previews in the app. |
| 7 | Click **Cut reviewed clips** to export final MP4s. |
| 8 | Click **Open output folder**. |

## Configuration

Settings are in the app UI.

| Setting | Default | Notes |
|---|---:|---|
| Clips | `5` | Number of clips DeepSeek should return. |
| Min seconds | `30` | Minimum clip duration. |
| Max seconds | `60` | Maximum clip duration. |
| Model | `deepseek-chat` | DeepSeek model name. |
| Temperature | `0.3` | Lower = more deterministic clip picks. |
| AI direction | empty | Per-video instructions like: “Prioritize founder advice and controversial takes.” |

## AI Direction Examples

```text
Prioritize clips about fitness myths, metabolism, and surprising science. Avoid long setup.
```

```text
Find clips that would work for startup founders. Prefer contrarian business advice and quotable one-liners.
```

```text
Look for emotional stories, conflict, or moments where the speaker changes their mind.
```

## MediaPipe Face Crop

Final clips try MediaPipe face crop first. If MediaPipe is unavailable, Clipster falls back to ffmpeg `cropdetect`.

This repo expects an optional venv at:

```bash
src-tauri/.venv
```

Install it:

```bash
brew install python@3.12
/opt/homebrew/bin/python3.12 -m venv src-tauri/.venv
src-tauri/.venv/bin/python -m pip install --upgrade pip
src-tauri/.venv/bin/python -m pip install opencv-contrib-python numpy
```

If face crop is working, `RUST_LOG=debug` logs will show `face crop:` during final cutting.

## Cached Re-analysis

Clipster keeps the downloaded source video and captions in the app cache for the current URL. Use **Re-analyze cached video** to skip downloading and run the LLM again with different settings or AI direction.

The source video is not copied into your output folder. Only final clips are exported there.

## Troubleshooting

| Problem | Fix |
|---|---|
| `cargo metadata` not found | Run `source "$HOME/.cargo/env"`, then retry. |
| No captions found | Video has no YouTube captions. Pick another video or add Whisper fallback later. |
| Preview does not play | Restart dev server and re-run analysis so previews regenerate. |
| Final MP4 does not play | Re-cut clips. Output uses `yuv420p` + `faststart` for compatibility. |
| MediaPipe not used | Install Python 3.12 venv dependencies above. |
| DeepSeek error | Check `src-tauri/.env` has `DEEPSEEK_API_KEY=...`. |

## Useful Commands

| Command | Purpose |
|---|---|
| `pnpm tauri dev` | Run app locally. |
| `RUST_LOG=debug pnpm tauri dev` | Run with verbose terminal logs. |
| `pnpm exec tsc --noEmit` | Type-check frontend. |
| `cd src-tauri && cargo check` | Check Rust code. |
| `pnpm tauri build` | Build app bundle. |

## Notes

Clipster analyzes captions only. It does not watch the video. Visual-only moments can be missed unless the captions make the moment obvious.
