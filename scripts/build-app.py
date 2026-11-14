"""Build the macOS Dashboard from this checkout and publish a complete app bundle."""
import argparse
import ctypes
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BUNDLE_ID = "com.ovrcr.dashboard"


def publish(stage, destination):
    # Both directories are on the same volume. Swap on update so Finder never
    # observes a partially replaced app and a failed rename preserves the old one.
    update = destination.exists()
    if destination.is_symlink():
        raise ValueError(f"Refusing to replace a symlink at {destination}")
    if update:
        with (destination / "Contents/Info.plist").open("rb") as info:
            if plistlib.load(info).get("CFBundleIdentifier") != BUNDLE_ID:
                raise ValueError(f"Refusing to replace a different app at {destination}")
    libc = ctypes.CDLL(None, use_errno=True)
    rename_swap, rename_excl = 2, 4  # Darwin sys/stdio.h
    if libc.renamex_np(os.fsencode(stage), os.fsencode(destination), rename_swap if update else rename_excl):
        raise OSError(ctypes.get_errno(), os.strerror(ctypes.get_errno()), destination)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--install", action="store_true", help="publish to ~/Applications")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("OVRCR.app requires macOS")

    target = (ROOT / os.environ.get("CARGO_TARGET_DIR", "target")).resolve()
    destination = (Path.home() / "Applications" if args.install else target) / "OVRCR.app"
    package = ROOT / "native/app"
    env = os.environ.copy()
    clt = Path("/Library/Developer/CommandLineTools")
    if clt.is_dir():
        env.setdefault("DEVELOPER_DIR", str(clt))
    subprocess.run(["cargo", "build", "--locked", "-p", "ovrcr", "--bin", "ovrcr", "--release",
                    "--target-dir", target], cwd=ROOT, check=True)
    cli = target / "release/ovrcr"
    swift = ["xcrun", "swift", "build", "--package-path", package, "--configuration", "release",
             "--only-use-versions-from-resolved-file"]
    subprocess.run([*swift, "--product", "OVRCRApp"], env=env, check=True)
    products = Path(subprocess.check_output([*swift, "--show-bin-path"], text=True, env=env).strip())
    payload = target / "startup-assets/ovrcr-startup"
    subprocess.run(["sh", ROOT / "scripts/package-startup.sh", payload, cli], check=True)

    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".ovrcr-build-", dir=destination.parent) as temp:
        app = Path(temp) / "OVRCR.app"
        contents = app / "Contents"
        bin_dir, resources = contents / "MacOS", contents / "Resources"
        bin_dir.mkdir(parents=True)
        resources.mkdir()
        shutil.copy2(products / "OVRCRApp", bin_dir / "OVRCRApp")
        shutil.copy2(cli, bin_dir / "ovrcr")
        shutil.copytree(payload, contents / "lib/ovrcr")
        for license_path in [ROOT / "LICENSE", ROOT / "THIRD_PARTY_NOTICES.md", package / "SwiftTerm-LICENSE"]:
            shutil.copy2(license_path, resources)
        with (contents / "Info.plist").open("wb") as info:
            plistlib.dump(dict(CFBundleExecutable="OVRCRApp", CFBundleIdentifier=BUNDLE_ID,
                               CFBundleName="OVRCR", CFBundlePackageType="APPL", CFBundleVersion="1",
                               CFBundleShortVersionString="0.1.0", LSMinimumSystemVersion="13.0",
                               NSHighResolutionCapable=True), info)
        subprocess.run(["codesign", "--force", "--sign", "-", app], check=True)
        subprocess.run(["codesign", "--verify", app], check=True)
        publish(app, destination)
    print(destination)


if __name__ == "__main__":
    main()
