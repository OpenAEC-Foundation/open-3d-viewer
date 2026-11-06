#!/usr/bin/env bash
# Runs the Linux build the way OpenAEC Linux, the streamed SuperCloud environment, runs it
# (OpenAEC-Blueprint#61): no GPU (Mesa's software Vulkan and OpenGL), no FUSE, no D-Bus session,
# no system tray, notification service or keyring, under the labwc compositor without a screen,
# with OPENAEC_BEHEERD=1 and OPENAEC_MAP set. It checks that the app keeps running, that the window
# draws, and that the 3D view draws a model, and it takes screenshots of what would be streamed.
# Then it drives the viewer as the SuperCloud's assistant does (route B of the Blueprint's
# docs/supercloud-koppeling.md): MCP over HTTP with OPENAEC_MCP_POORT and OPENAEC_MCP_GEHEIM, the
# tools viewer3d_*, open by a path in the work folder and save the IFC next to it.
#
#   packaging/linux/headless-test.sh APP MODEL OUT
#
# APP is the AppImage (run extracted with --appimage-extract, as without FUSE) or the
# open-3d-viewer program. MODEL is a small model that must show in the 3D view. OUT receives
# empty.png (no file open), model.png (MODEL open), memory-mb.txt (memory in use with no file open),
# mcp-*.json (the MCP answers) and the logs. Needs labwc, grim, Mesa (mesa-vulkan-drivers,
# libegl-mesa0), curl and python3.
set -euo pipefail

app=$(realpath "$1")
model=$(realpath "$2")
out=$(realpath -m "$3")
mkdir -p "$out"
work=$(mktemp -d)
pids=()
cleanup() {
    for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
    wait 2>/dev/null || true
    rm -rf "$work"
}
trap cleanup EXIT

if [[ "$app" == *.AppImage ]]; then
    chmod +x "$app"
    (cd "$work" && "$app" --appimage-extract >/dev/null)
    program="$work/squashfs-root/AppRun"
else
    program="$app"
fi
[[ -x "$program" ]] || { echo "::error::$program is not a program"; exit 1; }

# A new user in a session without a desktop: own home and runtime folder, nothing from the runner.
unset DISPLAY WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS
export HOME="$work/home" XDG_RUNTIME_DIR="$work/run"
export XDG_CONFIG_HOME="$HOME/.config"
mkdir -p "$HOME" "$XDG_CONFIG_HOME/labwc" "$work/openaec"
mkdir -m 700 "$XDG_RUNTIME_DIR"
export OPENAEC_BEHEERD=1 OPENAEC_MAP="$work/openaec"
# Nothing but software rendering: Mesa's lavapipe (Vulkan) and llvmpipe (OpenGL).
export LIBGL_ALWAYS_SOFTWARE=1

# labwc on a virtual output, every window maximized as in the SuperCloud
cat > "$XDG_CONFIG_HOME/labwc/rc.xml" <<'EOF'
<?xml version="1.0"?>
<labwc_config>
  <windowRules>
    <windowRule identifier="*">
      <action name="Maximize"/>
    </windowRule>
  </windowRules>
</labwc_config>
EOF
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 labwc > "$out/labwc.log" 2>&1 &
pids+=($!)
for _ in $(seq 50); do
    socket=$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -type s -name 'wayland-*' -printf '%f\n' | head -1 || true)
    [[ -n "$socket" ]] && break
    sleep 0.2
done
[[ -n "${socket:-}" ]] || { cat "$out/labwc.log"; echo "::error::labwc did not start"; exit 1; }
export WAYLAND_DISPLAY="$socket"
command -v vulkaninfo >/dev/null && vulkaninfo --summary 2>/dev/null | grep -E 'deviceName|driverName' | tee "$out/vulkan.txt" || true

# Starts the app (with the arguments given), lets it settle and takes a screenshot as NAME.
shot() {
    local name=$1; shift
    "$program" "$@" > "$out/app-$name.log" 2>&1 &
    local pid=$!
    pids+=("$pid")
    sleep 15
    if ! kill -0 "$pid" 2>/dev/null; then
        cat "$out/app-$name.log"
        echo "::error::the app stopped ($name)"
        exit 1
    fi
    if [[ "$name" == empty ]]; then
        # memory in use at rest, with no file open
        awk '/^VmRSS:/ { printf "%d\n", $2 / 1024 }' "/proc/$pid/status" > "$out/memory-mb.txt"
    fi
    grim "$out/$name.png"
    grim -t ppm "$work/$name.ppm"
    kill "$pid"
    wait "$pid" 2>/dev/null || true
}

shot empty
shot model "$model"

# The window drew (the empty screenshot is not one colour), and the 3D view drew the model (the
# middle of the window differs between the two). Without a working wgpu adapter iced falls back
# to its CPU renderer, which draws the window but not the 3D view, and the second check fails.
python3 -I - "$work/empty.ppm" "$work/model.ppm" <<'EOF'
import sys
from collections import Counter

def read_ppm(path):
    data = open(path, "rb").read()
    fields, pos = [], 0
    while len(fields) < 4:
        while data[pos:pos + 1].isspace():
            pos += 1
        start = pos
        while not data[pos:pos + 1].isspace():
            pos += 1
        fields.append(data[start:pos])
    assert fields[0] == b"P6" and fields[3] == b"255", "not an 8-bit PPM"
    w, h = int(fields[1]), int(fields[2])
    return w, h, data[pos + 1:]

w, h, empty = read_ppm(sys.argv[1])
w2, h2, model = read_ppm(sys.argv[2])
assert (w, h) == (w2, h2), "the screenshots differ in size"

pixels = Counter(empty[i:i + 3] for i in range(0, len(empty), 3))
other = 1 - pixels.most_common(1)[0][1] / (w * h)
print(f"screen {w}x{h}; empty window: {other:.1%} of the pixels differ from the most common colour")
if other < 0.01:
    sys.exit("::error::the window did not draw (the screenshot is one colour)")

changed = total = 0
for y in range(int(h * 0.35), int(h * 0.65)):
    for x in range(int(w * 0.35), int(w * 0.65)):
        i = (y * w + x) * 3
        total += 1
        if sum(abs(a - b) for a, b in zip(empty[i:i + 3], model[i:i + 3])) > 24:
            changed += 1
print(f"middle of the window: {changed / total:.1%} of the pixels changed with the model open")
# the cube fills most of the middle (about 85% changes); without the 3D view only the "No model
# open" text there goes away (under 10%)
if changed / total < 0.3:
    sys.exit("::error::the 3D view did not draw the model (no wgpu adapter?)")
EOF

# The assistant of the SuperCloud: the viewer serves MCP over HTTP itself.
name=$(basename "$model")
stem=${name%.*}
cp "$model" "$work/openaec/$name"
# a model outside the work folder, which would open if the viewer let it
mkdir -p "$work/outside"
cp "$model" "$work/outside/$name"
secret=$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')
port=${MCP_PORT:-39301}
OPENAEC_MCP_POORT=$port OPENAEC_MCP_GEHEIM=$secret "$program" > "$out/app-mcp.log" 2>&1 &
app_pid=$!
pids+=("$app_pid")
# mcp NAME BODY [curl options]: the HTTP status; the answer goes to mcp-NAME.json. A call that
# does not answer in two minutes fails rather than holding the job for the viewer's own time-out.
mcp() {
    local what=$1 body=$2; shift 2
    curl -sS --max-time 120 -o "$out/mcp-$what.json" -w '%{http_code}' -X POST "http://127.0.0.1:$port/mcp" \
        -H 'Content-Type: application/json' "$@" --data "$body" 2>/dev/null || true
}
auth=(-H "Authorization: Bearer $secret")
call() { echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"$1\",\"arguments\":$2}}"; }
up=
for _ in $(seq 60); do
    [[ "$(mcp ping '{"jsonrpc":"2.0","id":1,"method":"ping"}' "${auth[@]}")" == 200 ]] && { up=1; break; }
    sleep 0.5
done
if [[ -z "$up" ]]; then
    cat "$out/app-mcp.log"
    echo "::error::the MCP endpoint did not answer on port $port"
    exit 1
fi
codes=(
    "$(mcp tools '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' "${auth[@]}")"
    "$(mcp open "$(call viewer3d_open "{\"path\":\"$name\"}")" "${auth[@]}")"
    "$(mcp save "$(call viewer3d_opslaan '{}')" "${auth[@]}")"
    "$(mcp outside "$(call viewer3d_open "{\"path\":\"$work/outside/$name\"}")" "${auth[@]}")"
    "$(mcp outside-up "$(call viewer3d_open "{\"path\":\"../outside/$name\"}")" "${auth[@]}")"
    "$(mcp open-ifc "$(call viewer3d_open "{\"path\":\"$stem.ifc\"}")" "${auth[@]}")"
    "$(mcp save-ifc "$(call viewer3d_opslaan '{}')" "${auth[@]}")"
    "$(mcp origin '{}' "${auth[@]}" -H 'Origin: http://localhost')"
    "$(mcp nosecret '{}')"
)
python3 -I - "$out" "$work/openaec" "$stem" "${codes[@]}" <<'EOF'
import json, os, sys

out, folder, stem, *codes = sys.argv[1:]
expected = ["200"] * 7 + ["403", "401"]
if codes != expected:
    sys.exit(f"::error::MCP over HTTP answered {codes}, expected {expected}")
answer = lambda name: json.load(open(os.path.join(out, f"mcp-{name}.json")))["result"]
text = lambda name: answer(name)["content"][-1]["text"]
names = [t["name"] for t in answer("tools")["tools"]]
missing = {"viewer3d_open", "viewer3d_opslaan", "viewer3d_stand"} - set(names)
if missing or not all(n.startswith("viewer3d_") for n in names):
    sys.exit(f"::error::the tools are not the SuperCloud's: missing {missing}")
for step in ["open", "save", "open-ifc", "save-ifc"]:
    if answer(step)["isError"]:
        sys.exit(f"::error::{step}: {text(step)}")
# a model that would open, refused only because it lies outside the work folder
for step in ["outside", "outside-up"]:
    if not answer(step)["isError"] or "outside the work folder" not in text(step):
        sys.exit(f"::error::viewer3d_open opened a file outside the work folder ({step}): {text(step)}")
# saved next to the model as IFC; an open IFC is not replaced but saved beside it
for name in [f"{stem}.ifc", f"{stem}-viewer.ifc"]:
    ifc = os.path.join(folder, name)
    if not os.path.isfile(ifc) or not open(ifc, "rb").read(16).startswith(b"ISO-10303-21"):
        sys.exit(f"::error::viewer3d_opslaan did not write {name}")
print(f"MCP over HTTP: {len(names)} tools viewer3d_*, opened and saved {stem}.ifc and {stem}-viewer.ifc "
      f"in the work folder; browsers, missing secrets and paths outside refused")
EOF

# SIGTERM while a save runs (the SuperCloud ends the session): the save finishes or its temporary
# file goes, and the viewer stops by itself.
mcp stop-save "$(call viewer3d_opslaan '{"overwrite":true}')" "${auth[@]}" > /dev/null &
save_pid=$!
sleep 0.3
kill -TERM "$app_pid"
for _ in $(seq 50); do
    kill -0 "$app_pid" 2>/dev/null || break
    sleep 0.2
done
if kill -0 "$app_pid" 2>/dev/null; then
    cat "$out/app-mcp.log"
    echo "::error::the app did not stop within 10 s of SIGTERM"
    exit 1
fi
status=0
wait "$app_pid" || status=$?
wait "$save_pid" || true
left=$(find "$work/openaec" -name '*.part')
if [[ "$status" != 0 || -n "$left" ]]; then
    cat "$out/app-mcp.log"
    echo "::error::after SIGTERM: exit status $status, left behind: ${left:-nothing}"
    exit 1
fi
echo "SIGTERM during a save: stopped with status 0, no partial file left"
echo "memory at rest: $(cat "$out/memory-mb.txt") MB"
