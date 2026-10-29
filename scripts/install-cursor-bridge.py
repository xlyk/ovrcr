#!/usr/bin/env python3
"""Install the pinned official archive without executing it or reading credentials."""
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile

VERSION = "1.0.40"
PINS = {
    "arm64": "56f7f68a43ce73daa8d93c60b35b5301feb6a351e1dabfefe9873d80d56931cb",
    "x86_64": "cc1e1779e9768fc0db09a93d98dce51498e507ee26a24ae04c0dd21b21700ea6",
}


def checksum(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(65536), b""):
            digest.update(chunk)
    return digest.hexdigest()


def main():
    if len(sys.argv) > 2:
        raise ValueError("Usage: install-cursor-bridge.py [OFFICIAL_ARCHIVE]")
    machine = platform.machine()
    if sys.platform != "darwin" or machine not in PINS:
        raise ValueError("Cursor cloud requires macOS arm64 or x86_64")
    home = Path(os.environ["HOME"])
    if not home.is_absolute():
        raise ValueError("HOME must be absolute")
    base = home / ".local/lib/ovrcr-cursor-sdk"
    destination = base / VERSION
    for path in (base, destination):
        if path.is_symlink():
            raise ValueError(f"Refusing symlink installation: {path}")
    manifest = dict(sdk=VERSION, bridge="1.0.0", protocol="sdk.v1", sha256=PINS[machine])
    if destination.exists() and (not (destination / "artifact.json").is_file()
                                 or json.loads((destination / "artifact.json").read_text()) != manifest):
        raise ValueError(f"Refusing unmanaged installation: {destination}")
    installed = destination / "bridge.tar.gz"
    if len(sys.argv) == 1 and installed.is_file():
        if checksum(installed) == PINS[machine]:
            print(f"bridge_archive = {json.dumps(str(installed))}")
            return
    base.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".install-", dir=base) as temporary:
        stage = Path(temporary) / "current"
        stage.mkdir()
        archive = stage / "bridge.tar.gz"
        if len(sys.argv) == 2:
            shutil.copyfile(sys.argv[1], archive)
        else:
            architecture = "arm64" if machine == "arm64" else "x64"
            url = f"https://github.com/cursor/sdk-bridge/releases/download/v{VERSION}/cursor-sdk-bridge-standalone-darwin-{architecture}.tar.gz"
            subprocess.run(["curl", "--fail", "--location", "--proto", "=https", "--proto-redir", "=https", "--max-time", "180", "--max-filesize", "268435456", "--output", str(archive), url], check=True)
        if checksum(archive) != PINS[machine]:
            raise ValueError("pinned SDK archive checksum mismatch; installation unchanged")
        (stage / "artifact.json").write_text(json.dumps(manifest) + "\n")
        previous = Path(temporary) / "previous"
        if destination.exists():
            destination.rename(previous)
        try:
            stage.rename(destination)
        except OSError:
            if previous.exists():
                previous.rename(destination)
            raise
    print(f"bridge_archive = {json.dumps(str(destination / 'bridge.tar.gz'))}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(f"Cursor SDK installation failed: {error}")
