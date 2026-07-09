#!/usr/bin/env bash
#
# Regenerate still PNG screenshots of individual duodiff screens (for the
# README / the GitHub Pages landing page). Reuses the demo harness — the same
# seed data (seed.py) as record.sh — so screenshots are fully reproducible.
#
# Pipeline per shot: drive the real TUI to one screen (shoot.py) -> render the
# captured frame to a GIF with agg -> extract that frame to a PNG with ffmpeg.
#
# Requirements: cargo, python3, agg, ffmpeg, and a monospace font with
# box-drawing + emoji glyphs.
#
# Tunables (env): FONT, FONT_SIZE, COLS, ROWS, OUT_DIR.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

FONT="${FONT:-JetBrains Mono,Apple Color Emoji,Courier}"
FONT_SIZE="${FONT_SIZE:-16}"
OUT_DIR="${OUT_DIR:-$REPO_ROOT/website}"

# Each shot drives scripts/demo/shots/<name>.json and writes $OUT_DIR/<name>.png.
SHOTS=("tree-view")

PY=(python3)

for tool in cargo agg ffmpeg; do
  command -v "$tool" >/dev/null 2>&1 || { echo "error: '$tool' not found on PATH" >&2; exit 1; }
done

echo "==> building duodiff (release)"
cargo build --release --manifest-path "$REPO_ROOT/Cargo.toml"

# Use target/ directory inside workspace to avoid sandboxing blocks on /tmp
WORKSPACE="${DUODIFF_DEMO_WORKSPACE:-$REPO_ROOT/target/duodiff-shots}"
python3 -c "import shutil; import os; shutil.rmtree('$WORKSPACE') if os.path.exists('$WORKSPACE') else None"
mkdir -p "$WORKSPACE"

export DUODIFF_DEMO_HOME="$WORKSPACE"
export DUODIFF_DEMO_BIN="$REPO_ROOT/target/release/duodiff"
export DUODIFF_DEMO_CAST="$WORKSPACE/shot.cast"
export DUODIFF_DEMO_COLS="${COLS:-100}"
export DUODIFF_DEMO_ROWS="${ROWS:-30}"

mkdir -p "$OUT_DIR"
for name in "${SHOTS[@]}"; do
  steps="$SCRIPT_DIR/shots/$name.json"
  [ -f "$steps" ] || { echo "error: missing storyboard $steps" >&2; exit 1; }

  echo "==> [$name] seeding comparison trees"
  "${PY[@]}" "$SCRIPT_DIR/seed.py" >/dev/null

  echo "==> [$name] driving TUI to the target screen"
  DUODIFF_DEMO_STEPS="$steps" "${PY[@]}" "$SCRIPT_DIR/shoot.py"

  gif="$WORKSPACE/$name.gif"
  echo "==> [$name] rendering frame"
  agg --font-family "$FONT" --font-size "$FONT_SIZE" "$DUODIFF_DEMO_CAST" "$gif" >/dev/null 2>&1

  echo "==> [$name] extracting still PNG"
  ffmpeg -y -i "$gif" -vf reverse -update 1 -frames:v 1 "$OUT_DIR/$name.png" \
    -loglevel error
done

echo "==> cleaning workspace"
python3 -c "import shutil; shutil.rmtree('$WORKSPACE')"

echo "==> done"
for name in "${SHOTS[@]}"; do
  echo "    $OUT_DIR/$name.png"
done
