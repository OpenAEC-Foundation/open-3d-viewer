#!/usr/bin/env python3
"""The product video, made by driving the viewer through its own MCP server.

  uv run --with pillow --with imageio-ffmpeg python scripts/product_video.py [--revit FILE.rvt]

Starts a viewer window of its own (O3V_VIEWER, else the Revit build in revit/target/release,
else target/release/open-3d-viewer) in the theme Light and in English, connects
target/release/open-3d-viewer-mcp to it and plays the storyboard in `storyboard()`. After every
step it captures the whole window; each capture is held for a number of frames at 30 fps and
gets a caption. The result is target/video/open-3d-viewer.mp4 (1920x1200, H.264).

The models are the test models in test-models/. The Revit part needs the Revit build and a Revit
file with a door schedule, such as the Autodesk sample snowdon_arch_2025.rvt (--revit); without
it that part is left out. The window is closed at the end.
"""

import base64
import io
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target" / "video"
FRAMES = OUT / "frames"
FPS = 30
SIZE = (1920, 1200)
EXE = ".exe" if os.name == "nt" else ""
MCP = ROOT / "target" / "release" / f"open-3d-viewer-mcp{EXE}"
FONTS = ROOT / "assets" / "fonts"
ACCENT = (217, 119, 6)
NAVY = (15, 27, 45)

KAVEL = ROOT / "test-models" / "test-kavel.skp"
BATHROOM = ROOT / "test-models" / "bathroom-3d.skp"


def font(size, bold=False):
    return ImageFont.truetype(str(FONTS / ("Inter-SemiBold.ttf" if bold else "Inter-Regular.ttf")), size)


def viewer_exe():
    for candidate in [os.environ.get("O3V_VIEWER"), ROOT / "revit" / "target" / "release" / f"open-3d-viewer{EXE}",
                      ROOT / "target" / "release" / f"open-3d-viewer{EXE}"]:
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    sys.exit("build the viewer first: cargo build --release")


def announcement():
    base = os.environ.get("APPDATA") or os.path.join(os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config"))
    path = Path(base) / "OpenAEC" / "Open 3D Viewer Studio" / "remote.json"
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


class Viewer:
    """A viewer window of our own and the MCP server connected to it."""

    def __init__(self, exe):
        self.window = subprocess.Popen([str(exe), "--theme", "light", "--language", "en"])
        # the MCP server connects to the viewer that announced itself last: wait for ours
        start = time.time()
        while announcement().get("pid") != self.window.pid:
            if time.time() - start > 30:
                self.window.kill()
                sys.exit("the viewer did not start its remote control")
            time.sleep(0.3)
        self.mcp = subprocess.Popen([str(MCP)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8")
        self.id = 0
        self.rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "product-video", "version": "1"}})

    def rpc(self, method, params):
        self.id += 1
        self.mcp.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params}) + "\n")
        self.mcp.stdin.flush()
        return json.loads(self.mcp.stdout.readline())

    def __call__(self, tool, **args):
        """Calls a tool; returns its JSON answer, or the image for a screenshot."""
        result = self.rpc("tools/call", {"name": tool, "arguments": args}).get("result", {})
        for part in result.get("content", []):
            if part.get("type") == "image":
                return Image.open(io.BytesIO(base64.b64decode(part["data"]))).convert("RGB")
            if part.get("type") == "text":
                if result.get("isError"):
                    raise RuntimeError(f"{tool}: {part['text']}")
                try:
                    return json.loads(part["text"])
                except ValueError:
                    return part["text"]
        return None

    def close(self):
        self.mcp.stdin.close()
        self.mcp.wait(10)
        self.window.terminate()


class Film:
    """Frames on disk, each held for a number of video frames."""

    def __init__(self):
        shutil.rmtree(FRAMES, ignore_errors=True)
        FRAMES.mkdir(parents=True)
        self.clips = []

    def add(self, image, seconds, caption=None):
        image = image.resize(SIZE, Image.LANCZOS) if image.size != SIZE else image.copy()
        if caption:
            self.caption(image, caption)
        path = FRAMES / f"{len(self.clips):05d}.png"
        image.save(path)
        self.clips.append((path, seconds))

    @staticmethod
    def caption(image, text):
        draw = ImageDraw.Draw(image, "RGBA")
        f = font(40, bold=True)
        w = draw.textlength(text, font=f)
        x, y, pad = 48, SIZE[1] - 150, 24
        draw.rounded_rectangle((x, y, x + w + 2 * pad, y + 40 + 2 * pad), radius=14, fill=(15, 27, 45, 225))
        draw.rectangle((x, y + 14, x + 6, y + 40 + 2 * pad - 14), fill=ACCENT)
        draw.text((x + pad, y + pad - 4), text, font=f, fill="white")

    def card(self, title, lines, seconds):
        image = Image.new("RGB", SIZE, NAVY)
        draw = ImageDraw.Draw(image)
        draw.rectangle((160, 420, 172, 560), fill=ACCENT)
        draw.text((210, 410), title, font=font(84, bold=True), fill="white")
        for i, line in enumerate(lines):
            draw.text((212, 530 + i * 56), line, font=font(38), fill=(198, 208, 222))
        self.add(image, seconds)

    def still(self, path, seconds, caption):
        self.add(Image.open(path).convert("RGB"), seconds, caption)

    def encode(self, out):
        import imageio_ffmpeg

        listing = OUT / "frames.txt"
        with listing.open("w", encoding="utf-8") as f:
            for path, seconds in self.clips:
                f.write(f"file '{path.as_posix()}'\nduration {seconds:.4f}\n")
            f.write(f"file '{self.clips[-1][0].as_posix()}'\n")
        subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(listing),
                        "-vf", f"fps={FPS},format=yuv420p", "-c:v", "libx264", "-crf", "20", "-preset", "slow",
                        "-movflags", "+faststart", str(out)], check=True)


def storyboard(v, film, revit):
    frame = 1 / FPS
    shot = lambda seconds, caption=None: film.add(v("screenshot", area="window", max_size=SIZE[0]), seconds, caption)

    film.card("Open 3D Viewer Studio", ["View, measure and section building models,", "on the desktop and in the browser."], 3.0)

    # open a model and walk around it
    v("open_file", path=str(KAVEL))
    v("set_display", mode="edges")
    v("set_view", preset="iso")
    caption = "Open SketchUp, Tekla, glTF, OBJ, STL and PLY"
    shot(1.5, caption)
    for _ in range(90):
        v("navigate", orbit_yaw_deg=4)
        shot(frame, caption)
    shot(0.5, caption)

    # render modes and projection
    v("set_display", mode="wireframe")
    shot(1.2, "Solid, wireframe and edges")
    v("set_display", mode="edges")
    v("set_view", preset="top")
    v("set_camera", projection="orthographic")
    shot(1.4, "Standard views, perspective or orthographic")
    v("set_camera", projection="perspective")
    v("set_view", preset="iso")

    # section box, lowered from the roof
    world = v("get_state")["model"]["bounds"]
    top, bottom = world["max"][2], world["min"][2]
    v("section_box", reset=True)
    caption = "Cut it open with a section box"
    for i in range(40):
        v("section_box", max=[None, None, top - (top - bottom) * 0.62 * (i + 1) / 40])
        shot(frame * 2, caption)
    for _ in range(30):
        v("navigate", orbit_yaw_deg=-2, orbit_pitch_deg=0.5)
        shot(frame * 2, caption)
    shot(1.0, caption)
    v("section_box", enabled=False)
    v("set_view", preset="iso")

    # exploded view
    v("open_file", path=str(BATHROOM))
    v("set_display", mode="edges")
    v("set_view", preset="iso")
    v("explode", enabled=True, mode="elements", percent=[70, 70, 70])
    v("zoom_to", target="model")
    caption = "Pull it apart per storey or per element"
    for i in range(41):
        p = 70 * i / 40
        v("explode", percent=[p, p, p])
        shot(frame * 2, caption)
    shot(1.5, caption)
    v("explode", enabled=False)
    v("zoom_to", target="model")

    # measure, and an element's properties
    v("set_view", preset="front")
    state = v("get_state")["model"]["bounds"]
    lo, hi = state["min"], state["max"]
    y = lo[1]
    v("measure_distance", a=[lo[0], y, lo[2]], b=[lo[0], y, hi[2]])
    v("measure_distance", a=[lo[0], y, lo[2]], b=[hi[0], y, lo[2]])
    shot(2.5, "Measure distances, angles and areas")
    v("clear_measurements")

    # themes, from the README images
    themes = ROOT / "docs" / "images" / "readme" / "themes.png"
    if themes.exists():
        film.still(themes, 2.5, "Light, Dark, Blueprint and high contrast")

    # Revit and its schedules
    if revit:
        v("open_file", path=str(revit))
        for _ in range(120):
            if not v("list_schedules")["computing"]:
                break
            time.sleep(1)
        v("set_display", mode="edges")
        v("set_view", preset="iso")
        shot(2.0, "Revit models: levels, views and schedules")
        rows = v("get_schedule", schedule="Door Schedule", show=True, limit=5)["rows"]
        shot(2.0, "A schedule opens as a table under the 3D view")
        ids = next(r["elements"] for r in rows if r.get("elements"))
        # the doors are inside the building: a section box around them shows them in their wall
        v("select", ids=ids)
        v("section_box", fit="selection")
        v("zoom_to", target="selection")
        v("navigate", zoom=0.6)
        shot(3.0, "A row selects its doors")
        v("select", clear=True)

    film.card("Driven by AI agents", ["An MCP server lets Claude and other agents open,", "look around, select and measure.",
                                      "This video was made that way."], 3.5)
    film.card("Try it in your browser", ["open-3d-viewer.open-aec.com", "",
                                         "Free and open (LGPL-3.0), part of the OpenAEC Foundation."], 4.0)


def main():
    revit = None
    if "--revit" in sys.argv:
        revit = Path(sys.argv[sys.argv.index("--revit") + 1])
    if not MCP.exists():
        sys.exit("build the MCP server first: cargo build --release")
    film = Film()
    viewer = Viewer(viewer_exe())
    try:
        storyboard(viewer, film, revit)
    finally:
        viewer.close()
    out = OUT / "open-3d-viewer.mp4"
    film.encode(out)
    seconds = sum(s for _, s in film.clips)
    print(f"{out.relative_to(ROOT).as_posix()}: {seconds:.0f} s, {out.stat().st_size // 1024} kB")


if __name__ == "__main__":
    main()
