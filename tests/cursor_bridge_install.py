"""Exercise the real pinned-archive installer; never execute the SDK bridge."""
import hashlib
import json
import os
import platform
from pathlib import Path
import subprocess
import sys
import shutil
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
ARCHIVE = Path(os.environ["OVRCR_CURSOR_BRIDGE_ARCHIVE"])


class Installation(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.home = Path(self.temporary.name)
        self.destination = self.home / ".local/lib/ovrcr-cursor-sdk/1.0.40/bridge.tar.gz"

    def install(self, archive=ARCHIVE, **environment):
        return subprocess.run([sys.executable, "-I", str(ROOT / "scripts/install-cursor-bridge.py"), *([] if archive is None else [str(archive)])],
                              env={**os.environ, "HOME": str(self.home), **environment}, capture_output=True, text=True)

    def test_install_and_relocate_exact_official_archive_without_notification_collision(self):
        notification = self.home / ".local/lib/ovrcr/keep"
        notification.parent.mkdir(parents=True)
        notification.write_text("notification assets")
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(str(self.destination), result.stdout)
        self.assertEqual(hashlib.sha256(self.destination.read_bytes()).digest(), hashlib.sha256(ARCHIVE.read_bytes()).digest())
        relocated = self.home / "relocated"
        self.destination.parents[0].rename(relocated)
        self.assertEqual((relocated / "bridge.tar.gz").read_bytes(), ARCHIVE.read_bytes())
        self.assertEqual(notification.read_text(), "notification assets")
        self.assertEqual(self.install().returncode, 0)

    def test_reinstall_reuses_valid_pin_without_downloading(self):
        self.assertEqual(self.install().returncode, 0)
        result = self.install(None, PATH="/nonexistent")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("curl", result.stderr)

    def test_download_selects_fixed_official_host_archive(self):
        tools = self.home / "tools"
        tools.mkdir()
        arguments = self.home / "curl-arguments.json"
        curl = tools / "curl"
        curl.write_text(f"#!{sys.executable}\nimport json, shutil, sys\nfrom pathlib import Path\nPath({str(arguments)!r}).write_text(json.dumps(sys.argv[1:]))\nshutil.copyfile({str(ARCHIVE)!r}, sys.argv[sys.argv.index('--output') + 1])\n")
        curl.chmod(0o755)
        result = self.install(None, PATH=str(tools))
        self.assertEqual(result.returncode, 0, result.stderr)
        args = json.loads(arguments.read_text())
        arch = "arm64" if platform.machine() == "arm64" else "x64"
        self.assertEqual(args[-1], f"https://github.com/cursor/sdk-bridge/releases/download/v1.0.40/cursor-sdk-bridge-standalone-darwin-{arch}.tar.gz")
        self.assertEqual(args[args.index("--proto") + 1], "=https")
        self.assertEqual(args[args.index("--proto-redir") + 1], "=https")
        self.assertEqual(self.destination.read_bytes(), ARCHIVE.read_bytes())

    def test_missing_or_tampered_source_preserves_previous_installation(self):
        self.assertEqual(self.install().returncode, 0)
        original = self.destination.read_bytes()
        bad = self.home / "bad.tar.gz"
        for source in [self.home / "missing.tar.gz", bad]:
            if source == bad:
                bad.write_bytes(b"wrong architecture or altered artifact")
            result = self.install(source)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(self.destination.read_bytes(), original)

    def test_unmanaged_and_symlink_destination_refused(self):
        self.destination.parent.mkdir(parents=True)
        marker = self.destination.parent / "keep"
        marker.write_text("user content")
        self.assertNotEqual(self.install().returncode, 0)
        self.assertEqual(marker.read_text(), "user content")
        marker.unlink()
        self.destination.parent.rmdir()
        self.destination.parent.symlink_to(self.home, target_is_directory=True)
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("symlink", result.stderr)

    def test_relocated_cli_reports_passive_installed_and_configured_integrity(self):
        self.assertEqual(self.install().returncode, 0)
        executable = Path(os.environ["OVRCR_TEST_EXECUTABLE"])
        installed_cli = self.home / ".local/bin/ovrcr"
        installed_cli.parent.mkdir(parents=True)
        shutil.copyfile(executable, installed_cli)
        installed_cli.chmod(0o755)
        settings = self.home / "dashboard.toml"
        settings.write_text('[[cursor_cloud_accounts]]\nname="work"\nkeychain_service="unused"\nkeychain_account="unused"\nmodel="unused"\nrepositories=["https://github.com/acme/app"]\nowners=[]\nbridge_archive=' + json.dumps(str(self.destination)) + '\n')
        def doctor(command="doctor"):
            result = subprocess.run([str(installed_cli), "agent", command, "cursor-cloud", "--json" if command == "doctor" else "--print", "--settings", str(settings)],
                                    env={**os.environ, "HOME": str(self.home), "OVRCR_HOME": str(self.home / "config")},
                                    capture_output=True, text=True, timeout=3)
            self.assertEqual(result.returncode, 0, result.stderr)
            return json.loads(result.stdout)["bridge_dependency"]
        report = doctor()
        self.assertEqual(doctor("setup"), report)
        self.assertEqual(report["sdk_version"], "1.0.40")
        self.assertEqual(report["bridge_version"], "1.0.0")
        self.assertEqual(report["protocol"], "sdk.v1")
        self.assertEqual(report["installed_archive"]["path"], str(self.destination))
        self.assertEqual(report["installed_archive"]["status"], "verified")
        self.assertEqual(report["configured_archives"][0]["status"], "verified")
        self.destination.write_bytes(b"tampered")
        self.assertEqual(doctor()["installed_archive"]["status"], "invalid")
        self.destination.unlink()
        report = doctor()
        self.assertEqual(report["installed_archive"]["status"], "invalid")
        self.assertEqual(report["configured_archives"][0]["status"], "invalid")
        os.mkfifo(self.destination)
        self.assertEqual(doctor()["installed_archive"]["status"], "invalid")
        self.assertFalse(report["runtime_checked"])
        self.assertFalse(report["release_ready"])


if __name__ == "__main__":
    unittest.main()
