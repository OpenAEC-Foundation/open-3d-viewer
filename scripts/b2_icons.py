"""Copy the B2 icons the viewer uses from an OpenAEC Blueprint checkout.

The Blueprint's `openaec-icons` crate has one feature per app and none for the viewer yet, and
taking a whole feature would add hundreds of icons the viewer never shows. So the viewer keeps
the masters it uses, byte for byte, in `assets/b2/16` and `assets/b2/24`; `src/b2.rs` lists them.

    python scripts/b2_icons.py ../OpenAEC-Blueprint          copy the icons in the list
    python scripts/b2_icons.py ../OpenAEC-Blueprint --check  fail when a copy differs or is missing
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def ids():
    source = (ROOT / "src" / "b2.rs").read_text(encoding="utf-8")
    block = source.split("icons! {", 1)[1].split("}", 1)[0]
    return re.findall(r'"([a-z0-9.\-]+)"', block)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if len(args) != 1:
        sys.exit(__doc__)
    check = "--check" in sys.argv
    assets = pathlib.Path(args[0]) / "crates" / "openaec-icons" / "assets"
    wanted = ids()
    bad = []
    for size in ("16", "24"):
        target = ROOT / "assets" / "b2" / size
        target.mkdir(parents=True, exist_ok=True)
        for id in wanted:
            master = (assets / size / f"{id}.svgz").read_bytes()
            copy = target / f"{id}.svgz"
            if check:
                if not copy.exists() or copy.read_bytes() != master:
                    bad.append(f"{size}/{id}")
            else:
                copy.write_bytes(master)
        if not check:
            for old in target.glob("*.svgz"):
                if old.stem not in wanted:
                    old.unlink()
    if bad:
        sys.exit("differs from the Blueprint: " + ", ".join(bad))
    print(f"{len(wanted)} icons, 16 and 24 px: {'match' if check else 'copied'}")


if __name__ == "__main__":
    main()
