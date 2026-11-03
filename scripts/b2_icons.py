"""Copy the B2 icons the viewer uses from an OpenAEC Blueprint checkout.

The Blueprint's `openaec-icons` crate has one feature per app and none for the viewer yet, and
taking a whole feature would add hundreds of icons the viewer never shows. So the viewer keeps
the masters it uses, byte for byte, in `assets/b2/16` and `assets/b2/24`; `src/b2.rs` lists them.
Copying also writes `assets/b2/blueprint.sha256`: the Blueprint release and the SHA-256 of each
master, so the copies can be checked without the Blueprint, which is a private repository.

    python scripts/b2_icons.py ../OpenAEC-Blueprint          copy the icons in the list
    python scripts/b2_icons.py ../OpenAEC-Blueprint --check  fail when a copy differs from the Blueprint
    python scripts/b2_icons.py --check                       fail when a copy differs from blueprint.sha256
"""
import hashlib
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
B2 = ROOT / "assets" / "b2"
SUMS = B2 / "blueprint.sha256"
SIZES = ("16", "24")


def ids():
    source = (ROOT / "src" / "b2.rs").read_text(encoding="utf-8")
    block = source.split("icons! {", 1)[1].split("}", 1)[0]
    return re.findall(r'"([a-z0-9.\-]+)"', block)


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def release(checkout):
    """The Blueprint tag the checkout is on, or its commit."""
    for args in (["describe", "--tags", "--exact-match"], ["rev-parse", "HEAD"]):
        done = subprocess.run(["git", "-C", str(checkout), *args], capture_output=True, text=True)
        if done.returncode == 0:
            return done.stdout.strip()
    return "unknown"


def check_sums(wanted):
    """Every listed icon present and equal to the master recorded in blueprint.sha256."""
    if not SUMS.exists():
        sys.exit(f"missing {SUMS.relative_to(ROOT)}: copy the icons from a Blueprint checkout first")
    lines = SUMS.read_text(encoding="utf-8").splitlines()
    sums = dict(reversed(line.split(None, 1)) for line in lines if line and not line.startswith("#"))
    names = {f"{size}/{id}.svgz" for size in SIZES for id in wanted}
    bad = sorted(n for n in names if n not in sums or not (B2 / n).exists()
                 or sha256((B2 / n).read_bytes()) != sums[n])
    extra = sorted(set(sums) - names)
    if bad:
        sys.exit("differs from blueprint.sha256: " + ", ".join(bad))
    if extra:
        sys.exit("in blueprint.sha256 but not in src/b2.rs: " + ", ".join(extra))
    print(f"{len(wanted)} icons, 16 and 24 px: match {lines[0].lstrip('# ')}")


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    check = "--check" in sys.argv
    wanted = ids()
    if not args and check:
        return check_sums(wanted)
    if len(args) != 1:
        sys.exit(__doc__)
    assets = pathlib.Path(args[0]) / "crates" / "openaec-icons" / "assets"
    bad = []
    sums = []
    for size in SIZES:
        target = B2 / size
        target.mkdir(parents=True, exist_ok=True)
        for id in wanted:
            master = (assets / size / f"{id}.svgz").read_bytes()
            copy = target / f"{id}.svgz"
            sums.append(f"{sha256(master)}  {size}/{id}.svgz")
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
    if not check:
        header = f"# OpenAEC-Blueprint {release(args[0])}"
        SUMS.write_text("\n".join([header, *sums]) + "\n", encoding="utf-8", newline="\n")
    print(f"{len(wanted)} icons, 16 and 24 px: {'match' if check else 'copied'}")


if __name__ == "__main__":
    main()
