"""Verify OVRCR.app Info.plist icon metadata, sealed AppIcon.icns bytes, ICNS slots, and refusal cases."""
import hashlib
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile

assert __debug__, "Bundle icon checks require assertions; invoke python3 -I without -O"

ROOT = Path(__file__).resolve().parents[3]
EXPECTED_ICON = ROOT / "native/AppIcon.icns"
EXPECTED_HASH = hashlib.sha256(EXPECTED_ICON.read_bytes()).hexdigest()
EXPECTED_SLOTS = {
    (16, 16, 72),
    (32, 32, 144),
    (32, 32, 72),
    (64, 64, 144),
    (128, 128, 72),
    (256, 256, 144),
    (256, 256, 72),
    (512, 512, 144),
    (512, 512, 72),
    (1024, 1024, 144),
}


def validate_app_icon(bundle: Path) -> None:
    assert bundle.is_dir() and not bundle.is_symlink(), f"Invalid bundle directory: {bundle}"
    info_path = bundle / "Contents/Info.plist"
    assert info_path.is_file() and not info_path.is_symlink()
    info = plistlib.loads(info_path.read_bytes())
    assert info.get("CFBundleIdentifier") == "com.ovrcr.dashboard"
    assert info.get("CFBundleIconFile") == "AppIcon"
    icon_path = bundle / "Contents/Resources/AppIcon.icns"
    assert icon_path.is_file() and not icon_path.is_symlink()
    assert hashlib.sha256(icon_path.read_bytes()).hexdigest() == EXPECTED_HASH


def verify_icns_representations(icon_path: Path) -> None:
    swift_source = """
import AppKit
import Foundation
import ImageIO

let url = URL(fileURLWithPath: CommandLine.arguments[1])
guard let src = CGImageSourceCreateWithURL(url as CFURL, nil) else {
    fputs("Cannot open ICNS via ImageIO\\n", stderr)
    exit(1)
}
let type = CGImageSourceGetType(src) as String?
guard type == "com.apple.icns" else {
    fputs("Unexpected UTType: \\(String(describing: type))\\n", stderr)
    exit(2)
}
let count = CGImageSourceGetCount(src)
for idx in 0..<count {
    guard let img = CGImageSourceCreateImageAtIndex(src, idx, nil) else {
        fputs("Unreadable ICNS slot \\(idx)\\n", stderr)
        exit(3)
    }
    let props = CGImageSourceCopyPropertiesAtIndex(src, idx, nil) as? [String: Any]
    let dpi = Int((props?[kCGImagePropertyDPIWidth as String] as? Double ?? 72.0).rounded())
    print("\\(img.width),\\(img.height),\\(dpi)")
}
guard let nsImage = NSImage(contentsOf: url), nsImage.isValid, nsImage.representations.count == count else {
    fputs("AppKit NSImage failed to load all ICNS representations\\n", stderr)
    exit(4)
}
"""
    with tempfile.TemporaryDirectory(prefix="ovrcr-icns-check-") as tmp:
        binary = Path(tmp) / "verify-icns"
        cache = Path(tmp) / "module-cache"
        subprocess.run(
            ["xcrun", "swiftc", "-module-cache-path", str(cache), "-", "-o", str(binary)],
            input=swift_source,
            text=True,
            check=True,
        )
        output = subprocess.check_output([str(binary), str(icon_path)], text=True)
    observed = {
        tuple(int(part) for part in line.split(","))
        for line in output.strip().splitlines()
        if line
    }
    assert observed == EXPECTED_SLOTS, f"Unexpected ICNS slots: {observed!r}"


def run_negative_checks(bundle: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="ovrcr-app-icon-neg-") as tmp:
        root = Path(tmp)
        for case in ("missing_key", "wrong_key", "missing_file", "changed_bytes", "symlink"):
            copy = root / f"{case}.app"
            os_contents = copy / "Contents"
            (os_contents / "Resources").mkdir(parents=True)
            shutil.copy2(bundle / "Contents/Info.plist", os_contents / "Info.plist")
            icon = os_contents / "Resources/AppIcon.icns"
            shutil.copy2(bundle / "Contents/Resources/AppIcon.icns", icon)
            info = plistlib.loads((os_contents / "Info.plist").read_bytes())
            if case == "missing_key":
                info.pop("CFBundleIconFile", None)
                (os_contents / "Info.plist").write_bytes(plistlib.dumps(info))
            elif case == "wrong_key":
                info["CFBundleIconFile"] = "OtherIcon"
                (os_contents / "Info.plist").write_bytes(plistlib.dumps(info))
            elif case == "missing_file":
                icon.unlink()
            elif case == "changed_bytes":
                icon.write_bytes(b"corrupted icns")
            elif case == "symlink":
                icon.unlink()
                icon.symlink_to(EXPECTED_ICON)
            try:
                validate_app_icon(copy)
            except AssertionError:
                pass
            else:
                raise AssertionError(f"Expected icon validation failure for {case}")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("Usage: check_bundle.py APP_BUNDLE")
    bundle = Path(sys.argv[1]).resolve()
    validate_app_icon(bundle)
    verify_icns_representations(bundle / "Contents/Resources/AppIcon.icns")
    run_negative_checks(bundle)
    print(f"OVRCR.app icon verified: 10 retina/1x ICNS representations, SHA256 {EXPECTED_HASH}.")


if __name__ == "__main__":
    main()
