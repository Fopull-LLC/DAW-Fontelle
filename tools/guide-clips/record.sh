#!/bin/sh
# Records the guide's clips (docs/ux-routing-and-learning-plan.md step 7).
#
#   tools/guide-clips/record.sh build      the recording binary, once
#   tools/guide-clips/record.sh <name>...  record clips into assets/guide/
#   tools/guide-clips/record.sh all        every clip, settings last
#
# Needs a nested X server: `Xwayland :99 -geometry 1400x900 &`, Python with
# python-xlib and Pillow, ImageMagick. Work files go in $CLIPS_WORK.
set -e
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
W=${CLIPS_WORK:-/tmp/fontelle-clips}
export CLIPS_WORK=$W
mkdir -p "$W"
# A short home, so a path in a toast reads like one.
DEMO=/tmp/fontelle-demo
BIN=$W/fontelle-rec

running() { pgrep -x fontelle-rec >/dev/null; }

build() {
  # Vsync off: on the nested server a grab is a frame behind until the next
  # present, so a drag's result never showed. The source is put back.
  cd "$REPO"
  sed -i 's/wgpu::PresentMode::AutoVsync,/wgpu::PresentMode::AutoNoVsync,/' crates/fontelle-ui/src/app.rs
  cargo build -q --release -p fontelle-app || true
  sed -i 's/wgpu::PresentMode::AutoNoVsync,/wgpu::PresentMode::AutoVsync,/' crates/fontelle-ui/src/app.rs
  for p in $(pgrep -x fontelle-rec); do kill "$p"; done; sleep 1
  cp target/release/fontelle "$BIN"
  cargo build -q --release --example guide_clip_encode -p fontelle-ui
}

home() {
  # A clean home: default folders, nothing offered, nowhere to ask about.
  mkdir -p "$W/home/.config/fontelle" "$W/home/Music/Fontelle"
  ln -sfn "$W/home" "$DEMO"
  cat > "$W/home/.config/fontelle/settings.json" <<JSON
{"format_version":7,"soundfont_dirs":[],"projects_dir":"$DEMO/Music/Fontelle","recent_projects":[],"check_for_updates":false,"extensions_offered":true,"tour_offered":true}
JSON
}

fresh() {
  # Relaunch on a fresh tour song, its card closed.
  if running; then
    python3 "$HERE/close.py" >/dev/null; sleep 1.5
    running && python3 "$HERE/drive.py" m,600,500 s,0.3 c,495,386 s,1.5
    running && { for p in $(pgrep -x fontelle-rec); do kill "$p"; done; sleep 1; }
  fi
  env -u WAYLAND_DISPLAY -u XDG_CONFIG_HOME -u XDG_DATA_HOME DISPLAY=:99 HOME=$DEMO "$BIN" > "$W/fontelle.log" 2>&1 &
  sleep 6
  python3 "$HERE/drive.py" m,600,400 s,0.4 c,725,566 s,3.5 k,Escape s,0.8
}

setup() {
  case $1 in
    roll) python3 "$HERE/drive.py" c,450,213 s,0.8 w,800,520,-6 s,0.5 ;;
    # The rack and browser seam up, so the presets and the rack share a frame.
    browser) python3 "$HERE/drive.py" c,60,139 s,0.3 m,131,340 s,0.3 dn,131,328 m,131,300 m,131,250 m,131,215 s,0.2 up,131,215 s,0.5 ;;
  esac
  python3 "$HERE/drive.py" m,1390,890 s,0.5
}

record() {
  fresh
  setup "$1"
  python3 "$HERE/clip_$1.py" "$HERE" "$W/$1"
  "$REPO/target/release/examples/guide_clip_encode" "$W/$1" "$REPO/assets/guide/$1.png"
}

case $1 in
  build) build ;;
  all)
    home
    # Settings last: it opens the guide, which shows the others.
    for n in transport rack browser clips lanes roll mixer export; do record $n; done
    build
    record settings ;;
  *) home; for n in "$@"; do record "$n"; done ;;
esac
running && { python3 "$HERE/close.py" >/dev/null; sleep 1.5; for p in $(pgrep -x fontelle-rec); do kill "$p"; done; }
exit 0
