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
cp "$model" "$work/openaec/$name"
secret=$(head -c 24 /dev/urandom | od -An -tx1 | tr -d ' \n')
port=${MCP_PORT:-39301}
OPENAEC_MCP_POORT=$port OPENAEC_MCP_GEHEIM=$secret "$program" > "$out/app-mcp.log" 2>&1 &
pids+=($!)
# mcp NAME BODY [curl options]: the HTTP status; the answer goes to mcp-NAME.json
mcp() {
    local what=$1 body=$2; shift 2
    curl -sS -o "$out/mcp-$what.json" -w '%{http_code}' -X POST "http://127.0.0.1:$port/mcp" \
        -H 'Content-Type: application/json' "$@" --data "$body" 2>/dev/null || true
}
auth=(-H "Authorization: Bearer $secret")
call() { echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"$1\",\"arguments\":$2}}"; }
for _ in $(seq 60); do
    [[ "$(mcp ping '{"jsonrpc":"2.0","id":1,"method":"ping"}' "${auth[@]}")" == 200 ]] && break
    sleep 0.5
done
codes=(
    "$(mcp tools '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' "${auth[@]}")"
    "$(mcp open "$(call viewer3d_open "{\"path\":\"$name\"}")" "${auth[@]}")"
    "$(mcp save "$(call viewer3d_opslaan '{}')" "${auth[@]}")"
    "$(mcp outside "$(call viewer3d_open '{"path":"/etc/hostname"}')" "${auth[@]}")"
    "$(mcp origin '{}' "${auth[@]}" -H 'Origin: http://localhost')"
    "$(mcp nosecret '{}')"
)
python3 -I - "$out" "$work/openaec/${name%.*}.ifc" "${codes[@]}" <<'EOF'
import json, os, sys

out, ifc, *codes = sys.argv[1:]
expected = ["200", "200", "200", "200", "403", "401"]
if codes != expected:
    sys.exit(f"::error::MCP over HTTP answered {codes}, expected {expected}")
answer = lambda name: json.load(open(os.path.join(out, f"mcp-{name}.json")))["result"]
names = [t["name"] for t in answer("tools")["tools"]]
missing = {"viewer3d_open", "viewer3d_opslaan", "viewer3d_stand"} - set(names)
if missing or not all(n.startswith("viewer3d_") for n in names):
    sys.exit(f"::error::the tools are not the SuperCloud's: missing {missing}")
for step in ["open", "save"]:
    if answer(step)["isError"]:
        sys.exit(f"::error::viewer3d_{step}: {answer(step)['content'][0]['text']}")
if not answer("outside")["isError"]:
    sys.exit("::error::viewer3d_open opened a file outside the work folder")
if not os.path.isfile(ifc) or not open(ifc, "rb").read(16).startswith(b"ISO-10303-21"):
    sys.exit(f"::error::viewer3d_opslaan did not write {ifc}")
print(f"MCP over HTTP: {len(names)} tools viewer3d_*, opened and saved {os.path.basename(ifc)} "
      f"({os.path.getsize(ifc)} bytes) in the work folder; browsers, missing secrets and paths outside refused")
EOF
echo "memory at rest: $(cat "$out/memory-mb.txt") MB"
