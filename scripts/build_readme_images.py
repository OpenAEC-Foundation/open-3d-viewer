#!/usr/bin/env python3
"""The images of the README, rendered by the viewer itself.

  python scripts/build_readme_images.py           render and write docs/images/readme/*.png
  python scripts/build_readme_images.py --check   images present, small enough and
                                                  all used in README.md and README.nl.md

Rendering runs `open-3d-viewer --screenshot` (the whole window, headless, on the GPU) on the
test models into target/readme-renders/, so build first: `cargo build --release`. The schedule
image needs a Revit file with a door schedule, such as the Autodesk sample
snowdon_arch_2025.rvt: pass its path in O3V_SNOWDON. Without it that image is left as it is.

An image larger than MAX_BYTES is reduced to 256 colours without dithering, which keeps the
interface text sharp, and to fewer colours while it is still too large. Writing needs Pillow (`uv run --with pillow python scripts/...`);
--check does not.
"""

import os
import re
import struct
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs" / "images" / "readme"
RENDERS = ROOT / "target" / "readme-renders"
READMES = ["README.md", "README.nl.md"]
MAX_BYTES = 150 * 1024
WIDTH = 1440
EXE = ".exe" if os.name == "nt" else ""
VIEWER = ROOT / "target" / "release" / f"open-3d-viewer{EXE}"

KAVEL = "test-models/test-kavel.skp"
COMMON = ["--size", "1440x900", "--language", "en"]

# Render name -> (needs a Revit file, model, options).
SHOTS = {
    "kavel-light": (False, KAVEL, ["--mode", "edges"]),
    "kavel-dark": (False, KAVEL, ["--mode", "edges", "--theme", "dark"]),
    "kavel-blueprint": (False, KAVEL, ["--mode", "edges", "--theme", "blueprint"]),
    "kavel-high-contrast-dark": (False, KAVEL, ["--mode", "edges", "--theme", "high-contrast-dark"]),
    "file": (False, None, ["--page", "file", "--recent", KAVEL, "--recent", "test-models/bathroom-3d.skp",
                           "--recent", "test-models/solar-north-house.skp"]),
    "settings": (False, KAVEL, ["--page", "settings", "--theme", "blueprint"]),
    "shortcuts": (False, KAVEL, ["--page", "shortcuts"]),
    "explode": (False, "test-models/bathroom-3d.skp", ["--mode", "edges", "--explode", "60,60,60", "--explode-mode", "elements"]),
    "plan": (False, KAVEL, ["--mode", "wireframe", "--view", "top", "--ortho", "--theme", "dark"]),
    "schedule": (True, os.environ.get("O3V_SNOWDON"), ["--schedule", "Door Schedule", "--mode", "edges", "--theme", "dark"]),
}

# Output name -> one render, or four renders for a 2x2 mosaic.
IMAGES = {
    "hero.png": "kavel-light",
    "themes.png": ["kavel-light", "kavel-dark", "kavel-blueprint", "kavel-high-contrast-dark"],
    "file.png": "file",
    "settings.png": "settings",
    "shortcuts.png": "shortcuts",
    "explode.png": "explode",
    "plan.png": "plan",
    "schedule.png": "schedule",
}


def render():
    RENDERS.mkdir(parents=True, exist_ok=True)
    for name, (revit, model, options) in SHOTS.items():
        if not VIEWER.exists() or (revit and not (model and Path(model).exists())):
            print(f"skipped {name}: needs {VIEWER}" + (" and O3V_SNOWDON" if revit else ""))
            continue
        out = RENDERS / f"{name}.png"
        command = [str(VIEWER)] + ([model] if model else []) + ["--screenshot", str(out)] + COMMON + options
        subprocess.run(command, cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
        print(f"rendered {out.relative_to(ROOT).as_posix()}")


def build():
    from PIL import Image

    OUT.mkdir(parents=True, exist_ok=True)
    for name, source in IMAGES.items():
        sources = source if isinstance(source, list) else [source]
        if not all((RENDERS / f"{s}.png").exists() for s in sources):
            print(f"kept docs/images/readme/{name}: no render")
            continue
        tiles = [Image.open(RENDERS / f"{s}.png").convert("RGB") for s in sources]
        if len(tiles) == 4:
            w, h = tiles[0].size
            image = Image.new("RGB", (w, h), "white")
            gap = 16
            half = ((w - gap) // 2, (h - gap) // 2)
            for i, tile in enumerate(tiles):
                image.paste(tile.resize(half, Image.LANCZOS), ((i % 2) * (half[0] + gap), (i // 2) * (half[1] + gap)))
        else:
            image = tiles[0]
        if image.width > WIDTH:
            image = image.resize((WIDTH, round(image.height * WIDTH / image.width)), Image.LANCZOS)
        path = OUT / name
        image.save(path, optimize=True)
        for colours in (256, 192, 128, 96, 64):
            if path.stat().st_size <= MAX_BYTES:
                break
            image.quantize(colours, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).save(path, optimize=True)
        print(f"{path.relative_to(ROOT).as_posix()}  {path.stat().st_size // 1024} kB")


def png_size(path):
    head = path.read_bytes()[:24]
    if head[:8] != b"\x89PNG\r\n\x1a\n":
        return None
    return struct.unpack(">II", head[16:24])


def check():
    errors = []
    for name in IMAGES:
        path = OUT / name
        if not path.exists():
            errors.append(f"missing docs/images/readme/{name}; run this script without --check")
            continue
        size = png_size(path)
        if not size:
            errors.append(f"{name} is not a PNG")
        elif not 1280 <= size[0] <= 1600:
            errors.append(f"{name} is {size[0]} px wide, not 1280 to 1600")
        if path.stat().st_size > MAX_BYTES:
            errors.append(f"{name} is {path.stat().st_size // 1024} kB, more than {MAX_BYTES // 1024} kB")
    on_disk = {p.name for p in OUT.glob("*")} if OUT.exists() else set()
    for extra in sorted(on_disk - set(IMAGES)):
        errors.append(f"docs/images/readme/{extra} is not made by this script")
    for readme in READMES:
        text = (ROOT / readme).read_text(encoding="utf-8")
        used = set(re.findall(r"docs/images/readme/([\w.-]+)", text))
        for name in sorted(used - set(IMAGES)):
            errors.append(f"{readme} uses docs/images/readme/{name}, which does not exist")
        for name in sorted(set(IMAGES) - used):
            errors.append(f"{readme} does not use docs/images/readme/{name}")
    for error in errors:
        print(error)
    if not errors:
        print(f"{len(IMAGES)} README images present and used in {', '.join(READMES)}")
    return not errors


if __name__ == "__main__":
    if "--check" in sys.argv[1:]:
        sys.exit(0 if check() else 1)
    render()
    build()
