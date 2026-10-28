"""Check that vendored format crate snapshots match their recorded file hashes."""
from pathlib import Path
import hashlib
import json
import sys

root = Path(__file__).resolve().parents[1]
entries = json.loads((root / "vendor/formats-lock.json").read_text(encoding="utf-8"))
errors = []
for name, entry in entries.items():
    directory = root / "vendor" / name
    for relative, expected in entry["files"].items():
        path = directory / relative
        if not path.is_file() or hashlib.sha256(path.read_bytes().replace(b"\r\n", b"\n")).hexdigest() != expected:
            errors.append(f"{name}/{relative}: differs from pinned snapshot")
    for path in directory.rglob("*"):
        relative = path.relative_to(directory).as_posix()
        if path.is_file() and "target" not in path.parts and path.name != "Cargo.lock" and relative not in entry["files"]:
            errors.append(f"{name}/{relative}: unrecorded snapshot file")
if errors:
    sys.exit("\n".join(errors))
print(f"Verified {len(entries)} format crate snapshots.")
