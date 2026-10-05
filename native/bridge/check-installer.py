#!/usr/bin/env python3
"""Exercise the shipping installer with private fake signing/copy/CLI boundaries.

Run on macOS with: python3 -I native/bridge/check-installer.py
No actual signer, app, provider, permission, notification or audio operation runs.
The installer's absolute PlistBuddy reads only the constructed fixture plists.
"""
import hashlib
import json
import os
import plistlib
import re
import shlex
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPO = Path(__file__).resolve().parents[2]
INSTALLER = REPO / "scripts/install-bridge.sh"
REQUIREMENT = 'designated => identifier "{bundle_id}" and anchor FAKE'
IDENTITY = "FAKE installer regression identity; never a signing key"
DISPLAY_NAME = "OVRCR Installer Fixture"

# This dispatcher has no subprocess or real-tool fallback. Every executable
# wrapper passes its role, private configuration and actual path explicitly.
FAKE_TOOLS = r"""
import json
import os
import shutil
import sys
from pathlib import Path

role, config_path, executable = sys.argv[1:4]
args = sys.argv[4:]
config = json.loads(Path(config_path).read_text())
root = Path(config["root"])
source = Path(config["source"])
destination = Path(config["destination"])

def require(condition):
    if not condition:
        raise ValueError("unexpected fake-tool invocation")

def record(operation):
    require(len(args) <= 16 and all(len(arg) <= 1024 for arg in args))
    path = root / "calls.jsonl"
    prior = path.read_bytes() if path.exists() else b""
    require(len(prior) < 65536 and prior.count(b"\n") < 16)
    line = json.dumps({"operation": operation, "role": role, "args": args}).encode() + b"\n"
    require(len(line) <= 4096 and len(prior) + len(line) <= 65536)
    with path.open("ab") as file:
        file.write(line)

def bundle_kind(value):
    path = Path(value)
    require(path.is_absolute() and path == path.resolve())
    if path == destination:
        require(config["existing"])
        return "installed"
    require(path.name == "OVRCR Bridge.app")
    require(path.parent.parent == destination.parent)
    require(path.parent.name.startswith(".ovrcr-bridge-install."))
    require(path.parent.is_dir() and not path.parent.is_symlink())
    require(path.parent.stat().st_uid == os.getuid())
    return "staged"

try:
    require(root == root.resolve() and root.parent == Path("/private/tmp").resolve())
    require(root.name.startswith("ovrcr-installer-"))
    require(Path(config_path) == root / "fake-tools.json")
    require(source.parent == root and destination.parent == root / "home/Applications")
    if role == "ditto":
        require(Path(executable) == root / "bin/ditto")
        require(len(args) == 2 and Path(args[0]) == source)
        require(bundle_kind(args[1]) == "staged" and not Path(args[1]).exists())
        record("copy")
        shutil.copytree(source, args[1])
    elif role == "codesign":
        require(Path(executable) == root / "bin/codesign")
        require(args)
        kind = bundle_kind(args[-1])
        bundle = Path(args[-1])
        if args == ["--force", "--options", "runtime", "--timestamp",
                    "--entitlements", str(bundle / "Contents/OVRCRBridge.entitlements"),
                    "--sign", config["identity"], str(bundle)]:
            require(kind == "staged")
            record("sign")
        elif args == ["--verify", "--deep", "--strict", str(bundle)]:
            record("verify_" + kind)
        elif args == ["-d", "--entitlements", "-", str(bundle)]:
            require(kind == "staged")
            record("entitlements")
            sys.stdout.buffer.write((source / "Contents/OVRCRBridge.entitlements").read_bytes())
        elif args == ["-d", "-r-", str(bundle)]:
            record("requirement_" + kind)
            print("Executable=" + str(bundle / "Contents/MacOS/OVRCRBridge"), file=sys.stderr)
            requirement = config[kind + "_requirement"]
            if requirement:
                stream = sys.stdout if config["requirement_stream"] == "stdout" else sys.stderr
                print(requirement, file=stream)
        else:
            raise ValueError("unexpected fake codesign arguments")
    elif role in ("bridge", "cli"):
        path = Path(executable)
        require(path.name == ("OVRCRBridge" if role == "bridge" else "ovrcr"))
        require(path.parent.name == "MacOS" and path.parent.parent.name == "Contents")
        require(bundle_kind(str(path.parents[2])) == "staged")
        require(args == ["--check-contract", str(config["schema"]), str(config["wire"])])
        record(role + "_check_contract")
        print(json.dumps({"schema": config["schema"], "server_wire": config["wire"],
                          "status": "available"}))
    else:
        raise ValueError("unexpected fake tool role")
except (KeyError, ValueError, OSError):
    try:
        record("unexpected")
    except (KeyError, ValueError, OSError):
        pass
    print("Fake tool rejected unexpected invocation; no real-tool fallback", file=sys.stderr)
    raise SystemExit(64)
"""


def bundle_snapshot(bundle, include_inode=False):
    """Record bytes, modes and mtimes; reads do not change these attributes."""
    result = {}
    for path in [bundle, *sorted(bundle.rglob("*"))]:
        details = path.lstat()
        if path.is_symlink():
            raise AssertionError("Fixture bundle must contain no symlinks")
        entry = {
            "mode": stat.S_IMODE(details.st_mode),
            "mtime_ns": details.st_mtime_ns,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None,
        }
        if include_inode:
            entry["inode"] = details.st_ino
        result[str(path.relative_to(bundle))] = entry
    return result


class InstallerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if sys.platform != "darwin" or not Path("/usr/libexec/PlistBuddy").is_file():
            raise RuntimeError("Installer regression requires macOS's read-only PlistBuddy")
        # Refuse an installer that bypasses the intercepted bare tool commands.
        script = INSTALLER.read_text()
        for command in ("codesign", "ditto"):
            mentions = re.findall(r"\b" + command + r"\b", script)
            intercepted = re.findall(r"(?m)^\s*" + command + r"\s", script)
            if not mentions or len(mentions) != len(intercepted):
                raise RuntimeError("Installer no longer uses exclusively intercepted " + command)
        cls.schema = cls.version("bridge.rs", "BRIDGE_SCHEMA_VERSION")
        cls.wire = cls.version("codec.rs", "PROTOCOL_VERSION")

    @staticmethod
    def version(filename, name):
        source = (REPO / "crates/ovrcr-protocol/src" / filename).read_text()
        match = re.search(r"pub const " + name + r": u32 = (\d+);", source)
        if not match:
            raise RuntimeError("Missing current protocol constant " + name)
        return int(match[1])

    def setUp(self):
        previous_umask = os.umask(0o077)
        try:
            self.root = Path(tempfile.mkdtemp(prefix="ovrcr-installer-", dir="/private/tmp")).resolve()
        finally:
            os.umask(previous_umask)
        self.group_clear = True
        self.addCleanup(self.cleanup_fixture)
        self.assertEqual(stat.S_IMODE(self.root.stat().st_mode), 0o700)
        self.home = self.root / "home"
        self.temporary = self.root / "tmp"
        self.bin = self.root / "bin"
        for path in (self.home, self.temporary, self.bin):
            path.mkdir(mode=0o700)
        self.source = self.root / "Source.app"
        self.destination = self.home / "Applications/OVRCR Installer Fixture.app"
        suffix = self.root.name.rsplit("-", 1)[1].replace("_", "-")
        self.bundle_id = "dev.ovrcr.bridge.installer-fixture." + suffix
        self.config_path = self.root / "fake-tools.json"
        self.dispatcher = self.root / "fake-tools.py"
        self.dispatcher.write_text(FAKE_TOOLS)
        self.dispatcher.chmod(0o600)
        for role in ("codesign", "ditto"):
            self.write_wrapper(self.bin / role, role, "tool")
        self.bin.joinpath("python3").symlink_to(Path(sys.executable).resolve())
        # No system directory is on PATH: codesign/ditto can only be our fakes.
        for name, target in {
            "dirname": "/usr/bin/dirname", "mkdir": "/bin/mkdir",
            "mktemp": "/usr/bin/mktemp", "rm": "/bin/rm",
            "sed": "/usr/bin/sed", "cmp": "/usr/bin/cmp",
        }.items():
            self.bin.joinpath(name).symlink_to(target)
        self.environment = {
            "HOME": str(self.home), "TMPDIR": str(self.temporary) + "/",
            "PATH": str(self.bin), "LC_ALL": "C",
        }

    def cleanup_fixture(self):
        if not self.group_clear:
            print("Owned process group not confirmed gone; fixture retained: " + str(self.root),
                  flush=True)
            return
        shutil.rmtree(self.root)
        if self.root.exists():
            raise AssertionError("Owned fixture cleanup failed")
        print("Owned fixture removed: " + str(self.root), flush=True)

    def write_wrapper(self, path, role, revision):
        command = " ".join(shlex.quote(str(value)) for value in (
            Path(sys.executable).resolve(), "-I", self.dispatcher, role, self.config_path,
        ))
        path.write_text("#!/bin/sh\n# Harmless installer fixture " + revision +
                        '\nexec ' + command + ' "$0" "$@"\n')
        path.chmod(0o700)

    def create_bundle(self, bundle, revision):
        macos = bundle / "Contents/MacOS"
        resources = bundle / "Contents/Resources"
        macos.mkdir(parents=True, mode=0o700)
        resources.mkdir(mode=0o700)
        self.write_wrapper(macos / "OVRCRBridge", "bridge", revision)
        self.write_wrapper(macos / "ovrcr", "cli", revision)
        shutil.copyfile(REPO / "native/bridge/entitlements.plist",
                        bundle / "Contents/OVRCRBridge.entitlements")
        sounds = REPO / "research/notification-bridge/sounds"
        manifest = json.loads((sounds / "manifest.json").read_text())
        for tone in manifest["tones"]:
            shutil.copyfile(sounds / tone["file"], resources / tone["file"])
        shutil.copyfile(sounds / "manifest.json", resources / "NotificationSounds-manifest.json")
        shutil.copyfile(sounds / "LICENSE", resources / "NotificationSounds-LICENSE")
        info = {
            "CFBundleIdentifier": self.bundle_id, "CFBundleExecutable": "OVRCRBridge",
            "CFBundleDisplayName": DISPLAY_NAME, "CFBundleName": "Installer fixture " + revision,
            "CFBundlePackageType": "APPL", "CFBundleVersion": f"{self.schema}.{self.wire}",
            "LSUIElement": True, "OVRCRBridgeSchema": self.schema, "OVRCRServerWire": self.wire,
            "OVRCRCallbackSHA256": hashlib.sha256((macos / "ovrcr").read_bytes()).hexdigest(),
            "NSAppleEventsUsageDescription":
                "OVRCR can select the existing iTerm session hosting your current Dashboard "
                "after you explicitly set up iTerm focus.",
        }
        (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(info))

    def install(self, *, existing=True, stream="stderr", installed_requirement=REQUIREMENT,
                staged_requirement=REQUIREMENT, error=None):
        self.create_bundle(self.source, "new")
        source_before = bundle_snapshot(self.source, include_inode=True)
        if existing:
            self.create_bundle(self.destination, "old")
            installed_before = bundle_snapshot(self.destination, include_inode=True)
            self.assertNotEqual(bundle_snapshot(self.destination), bundle_snapshot(self.source))
        self.config_path.write_text(json.dumps({
            "root": str(self.root), "source": str(self.source), "destination": str(self.destination),
            "schema": self.schema, "wire": self.wire, "identity": IDENTITY, "existing": existing,
            "requirement_stream": stream,
            "installed_requirement": installed_requirement.format(bundle_id=self.bundle_id),
            "staged_requirement": staged_requirement.format(bundle_id=self.bundle_id),
        }))
        self.config_path.chmod(0o600)
        command = [
            "/bin/sh", str(INSTALLER), "--identity", IDENTITY,
            "--destination", str(self.destination), "--bundle-id", self.bundle_id,
            "--display-name", DISPLAY_NAME, str(self.source),
        ]
        process = subprocess.Popen(command, env=self.environment, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   start_new_session=True)
        self.group_clear = False
        print(f"{self._testMethodName}: root={self.root} pid={process.pid} pgid={process.pid}",
              flush=True)
        timed_out = False
        try:
            stdout, stderr = process.communicate(timeout=15)
        except subprocess.TimeoutExpired:
            timed_out = True
            # The unreaped child still owns this group ID. Never signal it after reap.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            stdout, stderr = process.communicate(timeout=5)
        processes = subprocess.run(["/bin/ps", "-axo", "pid=,pgid="], env=self.environment,
                                   capture_output=True, text=True, timeout=5, check=True)
        remaining = [line.split()[0] for line in processes.stdout.splitlines()
                     if len(line.split()) == 2 and line.split()[1] == str(process.pid)]
        self.group_clear = not remaining
        self.assertFalse(remaining, "Owned installer group still present: " + repr(remaining))
        self.assertFalse(timed_out, "Shipping installer timed out: " + repr((stdout, stderr)))
        self.assertLessEqual(len(stdout) + len(stderr), 65536)
        self.assertTrue((self.root / "calls.jsonl").is_file(), repr((stdout, stderr)))
        calls = [json.loads(line) for line in (self.root / "calls.jsonl").read_text().splitlines()]
        operations = [call["operation"] for call in calls]
        print(json.dumps({"case": self._testMethodName, "schema": self.schema, "wire": self.wire,
                          "returncode": process.returncode, "operations": operations,
                          "owned_group_members": remaining}), flush=True)
        expected = ["copy", "sign", "verify_staged", "entitlements", "bridge_check_contract"]
        if existing:
            expected += ["verify_installed", "requirement_installed", "requirement_staged"]
        self.assertEqual(operations, expected, repr(calls))
        self.assertEqual(bundle_snapshot(self.source, include_inode=True), source_before)
        self.assertEqual(process.returncode, 65 if error else 0, repr((stdout, stderr, calls)))
        self.assertEqual(stdout.count(b"Bundle metadata/resources valid:"), 2)
        if error:
            self.assertIn(error.encode(), stderr)
            self.assertNotIn(b"Installed ", stdout)
            self.assertEqual(bundle_snapshot(self.destination, include_inode=True), installed_before)
        else:
            self.assertIn(("Installed " + str(self.destination)).encode(), stdout)
            self.assertEqual(bundle_snapshot(self.destination), bundle_snapshot(self.source))
            if existing:
                self.assertNotEqual(self.destination.stat().st_ino, installed_before["."]["inode"])
        self.assertEqual(list(self.destination.parent.iterdir()), [self.destination])
        self.assertFalse((self.home / "Applications/OVRCR Bridge.app").exists())

    def test_fresh_install_uses_no_existing_requirement(self):
        self.install(existing=False)

    def test_matching_designated_requirement_on_stdout_updates(self):
        self.install(stream="stdout")

    def test_matching_designated_requirement_on_stderr_updates(self):
        self.install()

    def test_changed_designated_requirement_keeps_exact_old_bundle(self):
        self.install(staged_requirement=REQUIREMENT + " and certificate FAKE_OTHER",
                     error="Signing requirement changed; refusing update")

    def test_trailing_space_requirement_difference_keeps_exact_old_bundle(self):
        self.install(staged_requirement=REQUIREMENT + " ",
                     error="Signing requirement changed; refusing update")

    def test_empty_installed_requirement_keeps_exact_old_bundle(self):
        self.install(installed_requirement="", error="Cannot compare signing requirements")

    def test_empty_staged_requirement_keeps_exact_old_bundle(self):
        self.install(staged_requirement="", error="Cannot compare signing requirements")


if __name__ == "__main__":
    if not __debug__:
        raise SystemExit("Installer checks require assertions; invoke python3 -I without -O")
    unittest.main(verbosity=2)
