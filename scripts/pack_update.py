"""Packs a signed installer into an `.alloyupdate` file.

    python3 scripts/pack_update.py --installer path/to/Alloy_0.2.0_x64-setup.exe \
        --version 0.2.0 --platform windows-x86_64 --kind nsis --notes CHANGELOG.md --out dist-update/

Signs the installer and manifest.json through `pnpm tauri signer sign`, which reads
TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD from the environment.
An existing `<installer>.sig` is reused.
"""

import argparse
import datetime
import hashlib
import json
import pathlib
import re
import subprocess
import sys
import zipfile


def notes_for(changelog: pathlib.Path, version: str) -> str:
    if not changelog.exists():
        return ""
    text = changelog.read_text(encoding="utf-8")
    m = re.search(rf"^## {re.escape(version)}\b.*?$(.*?)(?=^## |\Z)", text, re.S | re.M)
    return (m.group(1) if m else "").strip()


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--installer", required=True)
    ap.add_argument("--version", required=True)
    ap.add_argument("--platform", required=True)
    ap.add_argument("--kind", required=True, choices=["nsis", "appimage"])
    ap.add_argument("--notes", default="CHANGELOG.md")
    ap.add_argument("--out", default="dist-update")
    a = ap.parse_args()

    installer = pathlib.Path(a.installer)
    sig = installer.with_name(installer.name + ".sig")
    shell = sys.platform == "win32"
    if not sig.exists():
        subprocess.run(["pnpm", "tauri", "signer", "sign", str(installer)], check=True, shell=shell)

    out = pathlib.Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    manifest = {
        "product": "alloy",
        "version": a.version.lstrip("v"),
        "platform": a.platform,
        "kind": a.kind,
        "file": installer.name,
        "sha256": hashlib.sha256(installer.read_bytes()).hexdigest(),
        "notes": notes_for(pathlib.Path(a.notes), a.version.lstrip("v")),
        "date": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
    }
    manifest_path = out / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")

    subprocess.run(["pnpm", "tauri", "signer", "sign", str(manifest_path)], check=True, shell=shell)

    target = out / f"Alloy_{manifest['version']}_{a.platform}.alloyupdate"
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as z:
        z.write(manifest_path, "manifest.json")
        z.write(out / "manifest.json.sig", "manifest.json.sig")
        z.write(installer, installer.name)
        z.write(sig, sig.name)
    print(target)
    return 0


if __name__ == "__main__":
    sys.exit(main())
