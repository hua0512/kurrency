#!/usr/bin/env bash
# Records the README's animated demo and screenshots (assets/screenshots/).
#
#   scripts/record-demo.sh [all|demo|stills|<still name>...]
#
# Linux with the niri compositor only; needs grim, wf-recorder, ffmpeg (with
# libwebp), python3 with Pillow, and bwrap. It runs a nested niri (a window
# on your desktop, pushed almost off-screen) whose output has scale 2, so
# everything is captured at 2x whatever your own display's scale, and your
# pointer and keyboard never reach the app. The app is a debug build (the
# demo and the fixture hooks exist only with debug assertions) with its
# dependencies optimised, so it animates smoothly, run against
# throwaway settings and a copy of scripts/demo-fixture/ (rates saved
# 2026-11-06 with the previous day and a year of USD/EUR and USD/GBP
# history), with the clock set to 2026-11-06 18:30 UTC (KURRENCY_NOW) and no
# network (bwrap --unshare-net), so every capture shows the same numbers.
# Your own settings and caches are never read or written.
#
# The demo is the KURRENCY_DEMO tour (crates/kurrency/src/app/demo.rs). It
# is recorded at 30 fps, trimmed to start and end on the same frame, so it
# loops seamlessly, and encoded as an animated WebP (demo.webp).
#
# KEEP_WORK=1 keeps the work directory (the frames and logs) for a look.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$ROOT/assets/screenshots
FIXTURE=$ROOT/scripts/demo-fixture
BIN=$ROOT/target/demo/kurrency
# 2026-11-06 18:30 UTC; the fixture's rates were fetched at 16:25.
NOW=1793989800
# The nested output, in its logical px (scale 2): tall enough for the
# window's natural height plus the 48px screen margins it keeps.
NEST_W=760
NEST_H=600
# Keep this many seconds of the still first and last frames (the loop seam).
HOLD=0.6
# Lossless (about 2 MB) keeps the text crisp; WEBP_QUALITY=80 encodes a
# smaller lossy file instead.
WEBP_QUALITY=${WEBP_QUALITY:-}

WORK=$(mktemp -d /tmp/kurrency-record.XXXXXX)
NEST_PID=
APP_PID=

cleanup() {
    [ -n "$APP_PID" ] && kill "$APP_PID" 2>/dev/null || true
    [ -n "$NEST_PID" ] && kill "$NEST_PID" 2>/dev/null || true
    wait 2>/dev/null || true
    [ -n "${KEEP_WORK:-}" ] && echo "record-demo: kept $WORK" >&2 || rm -rf "$WORK"
}
trap cleanup EXIT

for tool in niri grim wf-recorder ffmpeg python3 bwrap; do
    command -v "$tool" >/dev/null || { echo "record-demo: $tool is missing" >&2; exit 1; }
done
[ -n "${NIRI_SOCKET:-}" ] || { echo "record-demo: run this inside a niri session" >&2; exit 1; }

# Runs niri msg against the nested compositor.
nmsg() { NIRI_SOCKET=$NEST_IPC niri msg "$@"; }

start_nest() {
    cat > "$WORK/niri.kdl" <<'EOF'
output "winit" { scale 2.0; }
layout {
    gaps 0
    focus-ring { off; }
    border { off; }
    shadow { off; }
    background-color "#808080"
}
prefer-no-csd
animations { off; }
hotkey-overlay { skip-at-startup; }
cursor { hide-after-inactive-ms 100; }
window-rule {
    match app-id="dev.kurrency.Kurrency"
    open-floating true
}
EOF
    niri -c "$WORK/niri.kdl" > "$WORK/niri.log" 2>&1 &
    NEST_PID=$!
    for _ in $(seq 50); do
        grep -q "IPC listening on" "$WORK/niri.log" && break
        sleep 0.1
    done
    NEST_DISPLAY=$(sed -n 's/.*listening on Wayland socket: \(\S*\).*/\1/p' "$WORK/niri.log" | head -1)
    NEST_IPC=$(sed -n 's/.*IPC listening on: \(\S*\).*/\1/p' "$WORK/niri.log" | head -1)
    [ -n "$NEST_DISPLAY" ] && [ -n "$NEST_IPC" ] || { cat "$WORK/niri.log" >&2; exit 1; }
    # Float the nested niri's window on the host at the size that gives the
    # nested output NEST_W x NEST_H, and push it to the right edge.
    local scale id focused
    focused=$(niri msg -j focused-window | python3 -c "import json,sys; w=json.load(sys.stdin); print(w['id'] if w else '')")
    scale=$(niri msg -j focused-output | python3 -c "import json,sys; print(json.load(sys.stdin)['logical']['scale'])")
    id=$(niri msg -j windows | python3 -c "
import json,sys
print(next(w['id'] for w in json.load(sys.stdin) if w['pid']==$NEST_PID))")
    niri msg action toggle-window-floating --id "$id"
    niri msg action set-window-width --id "$id" "$(python3 -c "print(round($NEST_W*2/$scale))")"
    niri msg action set-window-height --id "$id" "$(python3 -c "print(round($NEST_H*2/$scale))")"
    niri msg action move-floating-window --id "$id" -x 100000 -y 0
    # Give the keyboard back, so typing elsewhere never reaches the app.
    [ -n "$focused" ] && niri msg action focus-window --id "$focused"
    sleep 1
    nmsg -j outputs | python3 -c "
import json,sys
o=next(iter(json.load(sys.stdin).values()))['logical']
assert (o['width'],o['height'])==($NEST_W,$NEST_H), o"
}

# run_app SETTINGS_JSON [VAR=value...]: starts the app in the nested niri.
run_app() {
    local settings=$1
    shift
    local config cache
    config=$(mktemp -d "$WORK/config.XXXXXX")
    cache=$(mktemp -d "$WORK/cache.XXXXXX")
    cp -r "$FIXTURE/cache/." "$cache/"
    printf '%s' "$settings" > "$config/settings.json"
    local fonts=()
    # The system's own Inter would shadow the bundled one.
    [ -d /usr/share/fonts/inter ] && fonts=(--tmpfs /usr/share/fonts/inter)
    env -u DISPLAY WAYLAND_DISPLAY="$NEST_DISPLAY" LANG="${APP_LANG:-en_US.UTF-8}" LC_ALL= \
        KURRENCY_CONFIG_DIR="$config" KURRENCY_CACHE_DIR="$cache" KURRENCY_NOW=$NOW \
        RUST_LOG=kurrency=info "$@" \
        bwrap --dev-bind / / --unshare-net "${fonts[@]}" "$BIN" > "$WORK/app.log" 2>&1 &
    APP_PID=$!
    for _ in $(seq 100); do
        nmsg -j windows | grep -q dev.kurrency.Kurrency && return 0
        sleep 0.1
    done
    cat "$WORK/app.log" >&2
    exit 1
}

stop_app() {
    kill "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
    APP_PID=
}

# The app window's rectangle in the nested output, "x,y wxh" (logical px).
geometry() {
    nmsg -j windows | python3 -c "
import json,sys
w=next(w for w in json.load(sys.stdin) if w['app_id']=='dev.kurrency.Kurrency')['layout']
x,y=w['tile_pos_in_workspace_view']; dx,dy=w['window_offset_in_tile']; ww,wh=w['window_size']
print(f'{round(x+dx)},{round(y+dy)} {ww}x{wh}')"
}

# geometry, once the window has stopped moving and resizing.
settled_geometry() {
    local before now
    now=$(geometry)
    for _ in $(seq 20); do
        before=$now
        sleep 0.3
        now=$(geometry)
        [ "$now" = "$before" ] && break
    done
    echo "$now"
}

# Losslessly recompresses PNGs and drops their metadata.
optimise_png() {
    if command -v oxipng >/dev/null; then
        oxipng -q -o 4 --strip all "$@"
    else
        python3 - "$@" <<'EOF'
import sys
from PIL import Image
for path in sys.argv[1:]:
    im = Image.open(path)
    im.load()
    if im.mode == "RGBA" and im.getextrema()[3] == (255, 255):
        im = im.convert("RGB")
    im.save(path, optimize=True)
EOF
    fi
}

LIGHT='"theme_mode":"light","language":"english"'
DARK='"theme_mode":"dark","language":"english"'
LIST='"currencies":["USD","EUR","JPY","GBP","CNY"],"source":"USD","amount":"100"'

# still NAME SETTINGS [VAR=value...]: one screenshot of the whole window.
still() {
    local name=$1 settings=$2
    shift 2
    run_app "{$settings}" KURRENCY_POINTER=away "$@"
    sleep "${WAIT:-4.5}"
    WAYLAND_DISPLAY=$NEST_DISPLAY grim -g "$(settled_geometry)" "$WORK/$name.png"
    stop_app
    if [ -n "${CROP:-}" ]; then
        python3 - "$WORK/$name.png" "$CROP" <<'EOF'
import sys
from PIL import Image
path, crop = sys.argv[1], [int(v) for v in sys.argv[2].split(",")]
Image.open(path).crop(crop).save(path)
EOF
    fi
    optimise_png "$WORK/$name.png"
    mv "$WORK/$name.png" "$OUT/$name.png"
    echo "record-demo: $name.png"
}

stills() {
    local want=("$@")
    wants() { [ ${#want[@]} -eq 0 ] || [[ " ${want[*]} " == *" $1 "* ]]; }
    wants main-light && still main-light "$LIST,$LIGHT"
    wants main-dark && still main-dark "$LIST,$DARK"
    wants editing && still editing "$LIST,$DARK" KURRENCY_EDIT='1.5k*3' KURRENCY_POINTER=row:1
    wants palette && still palette "$LIST,$LIGHT" KURRENCY_PALETTE='50 gbp'
    wants history && still history "$LIST,$DARK" KURRENCY_OPEN_DETAIL=EUR KURRENCY_POINTER=chart:0.64
    wants zh-cn && APP_LANG=zh_CN.UTF-8 still zh-cn "$LIST,\"theme_mode\":\"light\",\"language\":\"zh_hans\""
    # The main window in each of the four themes, side by side.
    if wants themes; then
        still theme-paper "$LIST,\"theme_mode\":\"light\",\"theme_light\":\"paper\",\"language\":\"english\""
        still theme-slate "$LIST,\"theme_mode\":\"light\",\"theme_light\":\"slate\",\"language\":\"english\""
        still theme-graphite "$LIST,\"theme_mode\":\"dark\",\"theme_dark\":\"graphite\",\"language\":\"english\""
        still theme-ink "$LIST,\"theme_mode\":\"dark\",\"theme_dark\":\"ink\",\"language\":\"english\""
        python3 - "$OUT" <<'PYEOF'
import sys
from PIL import Image
out = sys.argv[1]
names = ["paper", "slate", "graphite", "ink"]
ims = [Image.open(f"{out}/theme-{n}.png").convert("RGB") for n in names]
gap, w, h = 24, ims[0].width, max(i.height for i in ims)
sheet = Image.new("RGB", (w * 4 + gap * 3, h), (255, 255, 255))
for k, im in enumerate(ims):
    sheet.paste(im, (k * (w + gap), 0))
sheet = sheet.resize((sheet.width // 2, sheet.height // 2), Image.LANCZOS)
sheet.save(f"{out}/themes.png", optimize=True)
PYEOF
        rm -f "$OUT"/theme-{paper,slate,graphite,ink}.png
        echo "record-demo: themes.png"
    fi
    return 0
}

demo() {
    # The pointer leaves at 3.5s and the tour starts at 7s (its first change
    # at 7.9s); recording from 3.8s, the first change is the tour's.
    run_app "{$LIST,$LIGHT}" KURRENCY_POINTER=away KURRENCY_DEMO=1 KURRENCY_DEMO_DELAY_MS=7000
    sleep 3.8
    local raw=$WORK/demo.mkv
    # Every frame (-D), losslessly; wf-recorder runs a little behind.
    WAYLAND_DISPLAY=$NEST_DISPLAY wf-recorder -y -D -g "$(settled_geometry)" -r 30 \
        -c libx264rgb -x bgr0 -p crf=0 -p preset=ultrafast -f "$raw" > "$WORK/wf.log" 2>&1 &
    local rec=$!
    for _ in $(seq 400); do
        grep -q "kurrency-demo: end" "$WORK/app.log" && break
        sleep 0.1
    done
    grep -q "kurrency-demo: end" "$WORK/app.log" || { echo "record-demo: the tour did not finish" >&2; exit 1; }
    sleep 2.5
    kill -INT "$rec"
    wait "$rec" || true
    stop_app

    mkdir -p "$WORK/frames"
    ffmpeg -loglevel error -i "$raw" "$WORK/frames/%05d.png"
    # Trim to the tour, with HOLD seconds of its still first and last frames,
    # and check that the first and last frames match.
    python3 - "$WORK/frames" "$HOLD" > "$WORK/range" <<'EOF'
import os, sys
from PIL import Image, ImageChops
folder, hold = sys.argv[1], float(sys.argv[2])
names = sorted(os.listdir(folder))
frames = [Image.open(os.path.join(folder, n)).convert("RGB") for n in names]
def differs(a, b):
    return ImageChops.difference(a, b).getbbox() is not None
start = next(i for i in range(1, len(frames)) if differs(frames[i], frames[i - 1]))
last = len(frames) - 1
end = max(i for i in range(len(frames) - 1) if differs(frames[i], frames[last])) + 1
keep = round(hold * 30)
a, b = max(0, start - keep), min(len(frames) - 1, end + keep - 1)
seam = ImageChops.difference(frames[a], frames[b]).getbbox()
print(a, b)
print(f"record-demo: frames {a}..{b} of {len(frames)}; loop seam diff {seam}", file=sys.stderr)
EOF
    local a b
    read -r a b < "$WORK/range"
    local n=$((b - a + 1))
    ffmpeg -loglevel error -y -framerate 30 -start_number $((a + 1)) -i "$WORK/frames/%05d.png" \
        -frames:v "$n" -c:v libwebp_anim \
        $([ -n "$WEBP_QUALITY" ] && echo "-lossless 0 -q:v $WEBP_QUALITY -preset drawing" || echo "-lossless 1 -q:v 75") \
        -compression_level 6 -loop 0 "$OUT/demo.webp"
    echo "record-demo: demo.webp, $n frames ($(python3 -c "print(round($n/30,1))") s), $(du -k "$OUT/demo.webp" | cut -f1) KB"
}

# A debug build (debug assertions on) with optimised dependencies, in
# target/demo/ so the usual target/debug/ build is left alone.
cargo build -p kurrency --manifest-path "$ROOT/Cargo.toml" --profile demo \
    --config 'profile.demo.inherits="dev"' --config 'profile.demo.debug=false' \
    --config 'profile.demo.package."*".opt-level=2'
start_nest
case "${1:-all}" in
    all) stills; demo ;;
    demo) demo ;;
    stills) stills ;;
    *) stills "$@" ;;
esac
