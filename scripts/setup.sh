#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

log() {
  printf '\n==> %s\n' "$1"
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1
}

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This setup script currently supports macOS only." >&2
  exit 1
fi

log "Checking Homebrew"
if ! need_cmd brew; then
  echo "Homebrew missing. Install it first: https://brew.sh" >&2
  exit 1
fi

log "Installing system tools"
brew install ffmpeg yt-dlp python@3.12 || true

log "Checking Rust"
if ! need_cmd cargo; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi
# shellcheck disable=SC1091
source "$HOME/.cargo/env"

if ! grep -q 'cargo/env' "$HOME/.zshrc" 2>/dev/null; then
  log "Adding Rust to ~/.zshrc"
  {
    echo ''
    echo '# Rust toolchain'
    echo 'source "$HOME/.cargo/env"'
  } >> "$HOME/.zshrc"
fi

log "Installing frontend dependencies"
corepack enable >/dev/null 2>&1 || true
pnpm install

log "Setting up MediaPipe Python venv"
PY312="/opt/homebrew/bin/python3.12"
if [[ ! -x "$PY312" ]]; then
  PY312="$(brew --prefix python@3.12)/bin/python3.12"
fi
"$PY312" -m venv src-tauri/.venv
src-tauri/.venv/bin/python -m pip install --upgrade pip
src-tauri/.venv/bin/python -m pip install mediapipe numpy

log "Creating env template"
if [[ ! -f src-tauri/.env ]]; then
  cp src-tauri/.env.example src-tauri/.env
fi

cat <<'EOF'

Setup complete.

Next step:
  1. Edit src-tauri/.env
  2. Replace your-key-here with your DeepSeek API key
  3. Run: RUST_LOG=debug pnpm tauri dev

EOF
