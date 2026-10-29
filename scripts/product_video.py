#!/usr/bin/env python3
"""The product video, made by driving the viewer through its own MCP server.

  uv run --with pillow --with imageio-ffmpeg python scripts/product_video.py [--revit FILE.rvt]
         [--language en|nl] [--kit]

Starts a viewer window of its own (O3V_VIEWER, else the Revit build in revit/target/release,
else target/release/open-3d-viewer) in the theme Light, connects
target/release/open-3d-viewer-mcp to it and plays the storyboard in `storyboard()`, scene by
scene. After every step it captures the whole window; each capture is held for a number of
frames at 30 fps. The window is closed at the end.

Writes target/video/open-3d-viewer.mp4 (1920x1200, H.264) with the captions burned in. With
--kit it also writes the material for editing the video elsewhere into docs/video/: per scene a
clip without captions (clips/), its last frame (stills/) and storyboard.json with the order,
durations and the captions and card texts in English and Dutch (see docs/video/README.md).

The models are the test models in test-models/. The Revit part needs the Revit build and a Revit
file with a door schedule, such as the Autodesk sample snowdon_arch_2025.rvt (--revit); without
it those scenes are left out.
"""

import base64
import io
import json
import os
import shutil
import subprocess
import sys
import time
import tomllib
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target" / "video"
FRAMES = OUT / "frames"
KIT = ROOT / "docs" / "video"
FPS = 30
SIZE = (1920, 1200)
EXE = ".exe" if os.name == "nt" else ""
MCP = ROOT / "target" / "release" / f"open-3d-viewer-mcp{EXE}"
FONTS = ROOT / "assets" / "fonts"
ACCENT = (217, 119, 6)
NAVY = (15, 27, 45)

KAVEL = ROOT / "test-models" / "test-kavel.skp"
BATHROOM = ROOT / "test-models" / "bathroom-3d.skp"
WEB = "https://open-3d-viewer.open-aec.com"
REPO = "https://github.com/OpenAEC-Foundation/open-3d-viewer"


def font(size, bold=False):
    return ImageFont.truetype(str(FONTS / ("Inter-SemiBold.ttf" if bold else "Inter-Regular.ttf")), size)


def viewer_exe():
    for candidate in [os.environ.get("O3V_VIEWER"), ROOT / "revit" / "target" / "release" / f"open-3d-viewer{EXE}",
                      ROOT / "target" / "release" / f"open-3d-viewer{EXE}"]:
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    sys.exit("build the viewer first: cargo build --release")


def announcement():
    base = os.environ.get("APPDATA") or os.environ.get("XDG_CONFIG_HOME") or os.path.expanduser("~/.config")
    path = Path(base) / "OpenAEC" / "Open 3D Viewer Studio" / "remote.json"
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


class Viewer:
    """A viewer window of our own and the MCP server connected to it."""

    def __init__(self, exe, language):
        self.window = subprocess.Popen([str(exe), "--theme", "light", "--language", language])
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
    """The scenes: frames on disk without captions, each held for a number of seconds."""

    def __init__(self, language):
        shutil.rmtree(FRAMES, ignore_errors=True)
        FRAMES.mkdir(parents=True)
        self.language = language
        self.scenes = []

    def scene(self, id, kind, caption=None, **text):
        """Starts a scene; `caption` is {"en": ..., "nl": ...}, `text` holds card texts and notes."""
        self.scenes.append({"id": id, "kind": kind, "caption": caption, **text, "frames": []})

    def add(self, image, seconds):
        image = image.resize(SIZE, Image.LANCZOS) if image.size != SIZE else image.copy()
        n = sum(len(s["frames"]) for s in self.scenes)
        path = FRAMES / f"{n:05d}.png"
        image.save(path)
        self.scenes[-1]["frames"].append((path, seconds))

    def card(self, id, title, lines, seconds):
        """A text card in the OpenAEC colours; `title` and `lines` per language."""
        self.scene(id, "card", title=title, lines=lines)
        image = Image.new("RGB", SIZE, NAVY)
        draw = ImageDraw.Draw(image)
        draw.rectangle((160, 420, 172, 560), fill=ACCENT)
        draw.text((210, 410), title[self.language], font=font(84, bold=True), fill="white")
        for i, line in enumerate(lines[self.language]):
            draw.text((212, 530 + i * 56), line, font=font(38), fill=(198, 208, 222))
        self.add(image, seconds)

    @staticmethod
    def captioned(path, text):
        image = Image.open(path).convert("RGB")
        if text:
            draw = ImageDraw.Draw(image, "RGBA")
            f = font(40, bold=True)
            w = draw.textlength(text, font=f)
            x, y, pad = 48, SIZE[1] - 150, 24
            draw.rounded_rectangle((x, y, x + w + 2 * pad, y + 40 + 2 * pad), radius=14, fill=(15, 27, 45, 225))
            draw.rectangle((x, y + 14, x + 6, y + 40 + 2 * pad - 14), fill=ACCENT)
            draw.text((x + pad, y + pad - 4), text, font=f, fill="white")
        return image

    @staticmethod
    def encode(clips, out, crf=20):
        """Encodes (png, seconds) pairs as H.264 at FPS."""
        import imageio_ffmpeg

        listing = out.with_suffix(".txt")
        with listing.open("w", encoding="utf-8") as f:
            for path, seconds in clips:
                f.write(f"file '{path.as_posix()}'\nduration {seconds:.4f}\n")
            f.write(f"file '{clips[-1][0].as_posix()}'\n")
        subprocess.run([imageio_ffmpeg.get_ffmpeg_exe(), "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(listing),
                        "-vf", f"fps={FPS},format=yuv420p", "-c:v", "libx264", "-crf", str(crf), "-preset", "slow",
                        "-movflags", "+faststart", str(out)], check=True)
        listing.unlink()

    def video(self, out):
        """The whole film with the captions burned in."""
        clips = []
        for s in self.scenes:
            text = (s["caption"] or {}).get(self.language)
            for path, seconds in s["frames"]:
                if text:
                    captioned = path.with_name(path.stem + "-c.png")
                    self.captioned(path, text).save(captioned)
                    path = captioned
                clips.append((path, seconds))
        self.encode(clips, out)

    def kit(self):
        """Clips without captions, stills and storyboard.json in docs/video/."""
        for folder in ("clips", "stills"):
            shutil.rmtree(KIT / folder, ignore_errors=True)
            (KIT / folder).mkdir(parents=True)
        version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
        scenes, start = [], 0.0
        for i, s in enumerate(self.scenes, 1):
            name = f"{i:02d}-{s['id']}"
            duration = sum(seconds for _, seconds in s["frames"])
            still = KIT / "stills" / f"{name}.jpg"
            Image.open(s["frames"][-1][0]).convert("RGB").save(still, quality=90, optimize=True)
            entry = {"id": s["id"], "kind": s["kind"], "start_s": round(start, 2), "duration_s": round(duration, 2),
                     "still": still.relative_to(KIT).as_posix()}
            if len(s["frames"]) > 1:
                clip = KIT / "clips" / f"{name}.mp4"
                self.encode(s["frames"], clip, crf=23)
                entry["clip"] = clip.relative_to(KIT).as_posix()
            for key in ("caption", "title", "lines", "notes"):
                if s.get(key):
                    entry[key] = s[key]
            scenes.append(entry)
            start += duration
        storyboard = {
            "product": "Open 3D Viewer Studio",
            "version": version,
            "tagline": {"en": "View, measure and section building models, on the desktop and in the browser.",
                        "nl": "Bouwmodellen bekijken, meten en doorsnijden, op de desktop en in de browser."},
            "interface_language": self.language,
            "size": list(SIZE),
            "fps": FPS,
            "duration_s": round(start, 2),
            "links": {"web": WEB, "download": f"{REPO}/releases/latest", "repository": REPO},
            "license": "LGPL-3.0-or-later",
            "brand": {
                "accent": "#D97706",
                "navy": "#0F1B2D",
                "fonts": ["assets/fonts/Inter-Regular.ttf", "assets/fonts/Inter-SemiBold.ttf"],
                "logo": "assets/logo.svg",
                "app_icon": "assets/icons/icon.png",
            },
            "scenes": scenes,
        }
        (KIT / "storyboard.json").write_text(json.dumps(storyboard, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def storyboard(v, film, revit):
    frame = 1 / FPS
    shot = lambda seconds: film.add(v("screenshot", area="window", max_size=SIZE[0]), seconds)

    film.card("title", {"en": "Open 3D Viewer Studio", "nl": "Open 3D Viewer Studio"},
              {"en": ["View, measure and section building models,", "on the desktop and in the browser."],
               "nl": ["Bouwmodellen bekijken, meten en doorsnijden,", "op de desktop en in de browser."]}, 3.0)

    # open a model and walk around it
    v("open_file", path=str(KAVEL))
    v("set_display", mode="edges")
    v("set_view", preset="iso")
    film.scene("open-and-orbit", "clip", {"en": "Open SketchUp, Tekla, glTF, OBJ, STL and PLY", "nl": "Open SketchUp, Tekla, glTF, OBJ, STL en PLY"},
               notes="test-models/test-kavel.skp, one turn around the model")
    shot(1.5)
    for _ in range(90):
        v("navigate", orbit_yaw_deg=4)
        shot(frame)
    shot(0.5)

    # render modes and projection
    film.scene("render-modes", "still", {"en": "Solid, wireframe and edges", "nl": "Volledig, draadmodel en randen"})
    v("set_display", mode="wireframe")
    shot(1.2)
    film.scene("views", "still", {"en": "Standard views, perspective or orthographic", "nl": "Standaardaanzichten, perspectief of orthografisch"},
               notes="top view, orthographic")
    v("set_display", mode="edges")
    v("set_view", preset="top")
    v("set_camera", projection="orthographic")
    shot(1.4)
    v("set_camera", projection="perspective")
    v("set_view", preset="iso")

    # section box, lowered from the roof
    film.scene("section-box", "clip", {"en": "Cut it open with a section box", "nl": "Snijd het open met een doorsnedekader"},
               notes="the top of the box goes down from the roof, then the camera turns")
    world = v("get_state")["model"]["bounds"]
    top, bottom = world["max"][2], world["min"][2]
    v("section_box", reset=True)
    for i in range(40):
        v("section_box", max=[None, None, top - (top - bottom) * 0.62 * (i + 1) / 40])
        shot(frame * 2)
    for _ in range(30):
        v("navigate", orbit_yaw_deg=-2, orbit_pitch_deg=0.5)
        shot(frame * 2)
    shot(1.0)
    v("section_box", enabled=False)
    v("set_view", preset="iso")

    # exploded view
    v("open_file", path=str(BATHROOM))
    v("set_display", mode="edges")
    v("set_view", preset="iso")
    v("explode", enabled=True, mode="elements", percent=[70, 70, 70])
    v("zoom_to", target="model")
    film.scene("explode", "clip", {"en": "Pull it apart per storey or per element", "nl": "Uit elkaar per verdieping of per element"},
               notes="test-models/bathroom-3d.skp, from 0 to 70 % per element")
    for i in range(41):
        p = 70 * i / 40
        v("explode", percent=[p, p, p])
        shot(frame * 2)
    shot(1.5)
    v("explode", enabled=False)
    v("zoom_to", target="model")

    # measure
    film.scene("measure", "still", {"en": "Measure distances, angles and areas", "nl": "Meet afstanden, hoeken en oppervlakten"},
               notes="height and width of the bathroom, front view")
    v("set_view", preset="front")
    bounds = v("get_state")["model"]["bounds"]
    lo, hi = bounds["min"], bounds["max"]
    y = lo[1]
    v("measure_distance", a=[lo[0], y, lo[2]], b=[lo[0], y, hi[2]])
    v("measure_distance", a=[lo[0], y, lo[2]], b=[hi[0], y, lo[2]])
    shot(2.5)
    v("clear_measurements")

    # themes, from the README images
    themes = ROOT / "docs" / "images" / "readme" / "themes.png"
    if themes.exists():
        film.scene("themes", "still", {"en": "Light, Dark, Blueprint and high contrast", "nl": "Licht, Donker, Blueprint en hoog contrast"},
                   notes="docs/images/readme/themes.png")
        film.add(Image.open(themes).convert("RGB"), 2.5)

    # Revit and its schedules
    if revit:
        v("open_file", path=str(revit))
        for _ in range(120):
            if not v("list_schedules")["computing"]:
                break
            time.sleep(1)
        v("set_display", mode="edges")
        v("set_view", preset="iso")
        film.scene("revit", "still", {"en": "Revit models: levels, views and schedules", "nl": "Revit-modellen: niveaus, aanzichten en staten"},
                   notes=f"{revit.name} (Autodesk sample), Revit build")
        shot(2.0)
        film.scene("schedule", "still", {"en": "A schedule opens as a table under the 3D view", "nl": "Een staat opent als tabel onder de 3D-weergave"},
                   notes="Door Schedule")
        rows = v("get_schedule", schedule="Door Schedule", show=True, limit=5)["rows"]
        shot(2.0)
        ids = next(r["elements"] for r in rows if r.get("elements"))
        # the doors are inside the building: a section box around them shows them in their wall
        film.scene("schedule-select", "still", {"en": "A row selects its doors", "nl": "Een rij selecteert de deuren"},
                   notes="the first row selected, shown in a section box")
        v("select", ids=ids)
        v("section_box", fit="selection")
        v("zoom_to", target="selection")
        v("navigate", zoom=0.6)
        shot(3.0)
        v("select", clear=True)

    film.card("mcp", {"en": "Driven by AI agents", "nl": "Bestuurd door AI-agents"},
              {"en": ["An MCP server lets Claude and other agents open,", "look around, select and measure.", "This video was made that way."],
               "nl": ["Via een MCP-server openen Claude en andere agents", "modellen, kijken rond, selecteren en meten.", "Deze video is zo gemaakt."]}, 3.5)
    film.card("end", {"en": "Try it in your browser", "nl": "Probeer het in je browser"},
              {"en": ["open-3d-viewer.open-aec.com", "", "Free and open (LGPL-3.0), part of the OpenAEC Foundation."],
               "nl": ["open-3d-viewer.open-aec.com", "", "Vrij en open (LGPL-3.0), onderdeel van de OpenAEC Foundation."]}, 4.0)


def main():
    revit = None
    if "--revit" in sys.argv:
        revit = Path(sys.argv[sys.argv.index("--revit") + 1])
    language = sys.argv[sys.argv.index("--language") + 1] if "--language" in sys.argv else "en"
    if not MCP.exists():
        sys.exit("build the MCP server first: cargo build --release")
    film = Film(language)
    viewer = Viewer(viewer_exe(), language)
    try:
        storyboard(viewer, film, revit)
    finally:
        viewer.close()
    out = OUT / "open-3d-viewer.mp4"
    film.video(out)
    seconds = sum(s for scene in film.scenes for _, s in scene["frames"])
    print(f"{out.relative_to(ROOT).as_posix()}: {seconds:.0f} s, {out.stat().st_size // 1024} kB")
    if "--kit" in sys.argv:
        film.kit()
        size = sum(p.stat().st_size for p in KIT.rglob("*") if p.is_file())
        print(f"docs/video/: {len(film.scenes)} scenes, {size // 1024} kB")


if __name__ == "__main__":
    main()
