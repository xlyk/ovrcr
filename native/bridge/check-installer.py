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
LOCAL_BUNDLE_ID = "com.ovrcr.bridge.local"
LOCAL_DISPLAY_NAME = "OVRCR Local"
LOCAL_APP_NAME = "OVRCR Bridge Local.app"

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
    require(path.name == config["app_name"])
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
        timestamp = "--timestamp=none" if config["local_development"] else "--timestamp"
        identity = "-" if config["local_development"] else config["identity"]
        if args == ["--force", "--options", "runtime", timestamp,
                    "--entitlements", str(bundle / "Contents/OVRCRBridge.entitlements"),
                    "--sign", identity, str(bundle)]:
            require(kind == "staged")
            record("sign")
            if config.get("competing_destination"):
                require(config["local_development"] and not destination.exists())
                destination.mkdir(mode=0o700)
                (root / "competing-destination.json").write_text(json.dumps({
                    "inode": destination.stat().st_ino,
                    "mode": destination.stat().st_mode,
                }))
                record("create_competing_destination")
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
        if re.search(r"\bsecurity\b", script):
            raise RuntimeError("Installer must never discover or query signing identities")
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
        self.display_name = DISPLAY_NAME
        self.local_development = False
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
            "CFBundleDisplayName": self.display_name, "CFBundleName": "Installer fixture " + revision,
            "CFBundlePackageType": "APPL", "CFBundleVersion": f"{self.schema}.{self.wire}",
            "LSUIElement": True, "OVRCRBridgeSchema": self.schema, "OVRCRServerWire": self.wire,
            "OVRCRCallbackSHA256": hashlib.sha256((macos / "ovrcr").read_bytes()).hexdigest(),
            "NSAppleEventsUsageDescription":
                "OVRCR can select the existing iTerm session hosting your current Dashboard "
                "after you explicitly set up iTerm focus.",
        }
        if self.local_development:
            info["OVRCRBridgeLocalDevelopment"] = True
        (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(info))

    def configure_tools(self, *, existing=False, stream="stderr", installed_requirement=REQUIREMENT,
                        staged_requirement=REQUIREMENT, competing_destination=False):
        self.config_path.write_text(json.dumps({
            "root": str(self.root), "source": str(self.source), "destination": str(self.destination),
            "schema": self.schema, "wire": self.wire, "identity": IDENTITY, "existing": existing,
            "app_name": LOCAL_APP_NAME if self.local_development else "OVRCR Bridge.app",
            "local_development": self.local_development,
            "competing_destination": competing_destination,
            "requirement_stream": stream,
            "installed_requirement": installed_requirement.format(bundle_id=self.bundle_id),
            "staged_requirement": staged_requirement.format(bundle_id=self.bundle_id),
        }))
        self.config_path.chmod(0o600)

    def run_installer(self, command):
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
        calls_path = self.root / "calls.jsonl"
        calls = [json.loads(line) for line in calls_path.read_text().splitlines()] if calls_path.exists() else []
        print(json.dumps({"case": self._testMethodName, "schema": self.schema, "wire": self.wire,
                          "returncode": process.returncode,
                          "operations": [call["operation"] for call in calls],
                          "owned_group_members": remaining}), flush=True)
        return process.returncode, stdout, stderr, calls

    def install(self, *, existing=True, stream="stderr", installed_requirement=REQUIREMENT,
                staged_requirement=REQUIREMENT, error=None):
        self.create_bundle(self.source, "new")
        source_before = bundle_snapshot(self.source, include_inode=True)
        if existing:
            self.create_bundle(self.destination, "old")
            installed_before = bundle_snapshot(self.destination, include_inode=True)
            self.assertNotEqual(bundle_snapshot(self.destination), bundle_snapshot(self.source))
        self.configure_tools(existing=existing, stream=stream,
                             installed_requirement=installed_requirement,
                             staged_requirement=staged_requirement)
        command = [
            "/bin/sh", str(INSTALLER), "--identity", IDENTITY,
            "--destination", str(self.destination), "--bundle-id", self.bundle_id,
            "--display-name", DISPLAY_NAME, str(self.source),
        ]
        returncode, stdout, stderr, calls = self.run_installer(command)
        self.assertTrue((self.root / "calls.jsonl").is_file(), repr((stdout, stderr)))
        operations = [call["operation"] for call in calls]
        expected = ["copy", "sign", "verify_staged", "entitlements", "bridge_check_contract"]
        if existing:
            expected += ["verify_installed", "requirement_installed", "requirement_staged"]
        self.assertEqual(operations, expected, repr(calls))
        self.assertEqual(bundle_snapshot(self.source, include_inode=True), source_before)
        self.assertEqual(returncode, 65 if error else 0, repr((stdout, stderr, calls)))
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

    def prepare_local(self):
        self.local_development = True
        self.bundle_id = LOCAL_BUNDLE_ID
        self.display_name = LOCAL_DISPLAY_NAME
        self.destination = self.home / "Applications" / LOCAL_APP_NAME
        self.create_bundle(self.source, "local-new")
        self.configure_tools()

    def local_command(self, *options):
        return ["/bin/sh", str(INSTALLER), "--local-development", "--destination",
                str(self.destination), *options, str(self.source)]

    def assert_refused_before_staging(self, command, *, returncode, message=None):
        source_before = bundle_snapshot(self.source, include_inode=True)
        result, stdout, stderr, calls = self.run_installer(command)
        self.assertEqual(result, returncode, repr((stdout, stderr, calls)))
        if message:
            self.assertIn(message.encode(), stderr)
        self.assertEqual(calls, [], "Refusal must precede copy, signing and contract execution")
        self.assertNotIn(b"Installed ", stdout)
        self.assertEqual(list(self.destination.parent.glob(".ovrcr-bridge-install.*")), [])
        self.assertEqual(bundle_snapshot(self.source, include_inode=True), source_before)

    def test_local_fresh_install_signs_only_enclosing_app_without_identity_or_timestamp(self):
        self.prepare_local()
        source_before = bundle_snapshot(self.source, include_inode=True)
        returncode, stdout, stderr, calls = self.run_installer(self.local_command())
        self.assertEqual(returncode, 0, repr((stdout, stderr, calls)))
        self.assertEqual([call["operation"] for call in calls],
                         ["copy", "sign", "verify_staged", "entitlements", "bridge_check_contract"])
        sign = next(call for call in calls if call["operation"] == "sign")
        staged = Path(sign["args"][-1])
        self.assertEqual(sign["args"], ["--force", "--options", "runtime", "--timestamp=none",
                                       "--entitlements", str(staged / "Contents/OVRCRBridge.entitlements"),
                                       "--sign", "-", str(staged)])
        self.assertEqual(staged.name, LOCAL_APP_NAME)
        self.assertNotIn("--deep", sign["args"], "Do not re-sign the sealed callback helper")
        self.assertEqual({call["role"] for call in calls}, {"ditto", "codesign", "bridge"})
        self.assertEqual(bundle_snapshot(self.source, include_inode=True), source_before)
        self.assertEqual(bundle_snapshot(self.destination), bundle_snapshot(self.source))
        self.assertEqual(stdout.count(b"Bundle metadata/resources valid:"), 2)
        self.assertIn(("Installed " + str(self.destination)).encode(), stdout)
        self.assertEqual(list(self.destination.parent.iterdir()), [self.destination])
        self.assertFalse((self.home / "Applications/OVRCR Bridge.app").exists())

    def existing_local_path_is_preserved(self, kind):
        self.prepare_local()
        self.destination.parent.mkdir(mode=0o700)
        if kind == "bundle":
            self.create_bundle(self.destination, "local-old")
        elif kind == "directory":
            self.destination.mkdir(mode=0o700)
        elif kind == "file":
            self.destination.write_bytes(b"existing local path must survive")
        elif kind == "fifo":
            os.mkfifo(self.destination, 0o600)
        elif kind == "symlink":
            self.destination.symlink_to(self.source)
        elif kind == "broken_symlink":
            self.destination.symlink_to(self.root / "missing-owned-target")
        else:
            raise AssertionError("Unknown existing-path fixture")
        before = self.destination.lstat()
        original = (bundle_snapshot(self.destination, include_inode=True) if kind == "bundle"
                    else self.destination.read_bytes() if kind == "file"
                    else os.readlink(self.destination) if self.destination.is_symlink() else None)
        self.assert_refused_before_staging(self.local_command(), returncode=65,
                                          message="requires a fresh destination")
        after = self.destination.lstat()
        self.assertEqual((after.st_ino, after.st_mode, after.st_mtime_ns, after.st_size),
                         (before.st_ino, before.st_mode, before.st_mtime_ns, before.st_size))
        if kind == "bundle":
            self.assertEqual(bundle_snapshot(self.destination, include_inode=True), original)
        elif kind == "file":
            self.assertEqual(self.destination.read_bytes(), original)
        elif self.destination.is_symlink():
            self.assertEqual(os.readlink(self.destination), original)
        self.assertEqual(list(self.destination.parent.iterdir()), [self.destination])

    def test_local_existing_valid_bundle_is_never_updated(self):
        self.existing_local_path_is_preserved("bundle")

    def test_local_existing_empty_directory_is_never_replaced(self):
        self.existing_local_path_is_preserved("directory")

    def test_local_existing_file_is_never_replaced(self):
        self.existing_local_path_is_preserved("file")

    def test_local_existing_fifo_is_never_replaced(self):
        self.existing_local_path_is_preserved("fifo")

    def test_local_existing_symlink_is_never_followed(self):
        self.existing_local_path_is_preserved("symlink")

    def test_local_existing_broken_symlink_is_never_replaced(self):
        self.existing_local_path_is_preserved("broken_symlink")

    def test_local_concurrent_empty_destination_is_preserved_and_stage_retained(self):
        self.prepare_local()
        self.configure_tools(competing_destination=True)
        source_before = bundle_snapshot(self.source, include_inode=True)
        returncode, stdout, stderr, calls = self.run_installer(self.local_command())
        self.assertEqual(returncode, 74, repr((stdout, stderr, calls)))
        self.assertEqual([call["operation"] for call in calls],
                         ["copy", "sign", "create_competing_destination", "verify_staged",
                          "entitlements", "bridge_check_contract"])
        competing = json.loads((self.root / "competing-destination.json").read_text())
        self.assertEqual(self.destination.stat().st_ino, competing["inode"])
        self.assertEqual(self.destination.stat().st_mode, competing["mode"])
        self.assertEqual(list(self.destination.iterdir()), [],
                         "An empty competing directory must survive; ordinary os.rename would replace it")
        sign = next(call for call in calls if call["operation"] == "sign")
        staged = Path(sign["args"][-1])
        self.assertTrue(staged.is_dir())
        self.assertEqual(staged.name, LOCAL_APP_NAME)
        self.assertEqual(staged.parent.parent, self.destination.parent)
        self.assertTrue(staged.parent.name.startswith(".ovrcr-bridge-install."))
        self.assertEqual(bundle_snapshot(staged), bundle_snapshot(self.source))
        self.assertEqual(bundle_snapshot(self.source, include_inode=True), source_before)
        self.assertIn(("Local staged application retained for inspection: " + str(staged)).encode(), stderr)
        self.assertIn(b"Local development publication refused; destination retained", stderr)
        self.assertNotIn(b"Installed ", stdout)
        self.assertEqual(set(self.destination.parent.iterdir()), {self.destination, staged.parent})
        print(json.dumps({"retained_stage": str(staged), "competing_inode": competing["inode"],
                          "competing_empty": True}), flush=True)

    def test_local_refuses_all_identity_and_metadata_overrides(self):
        self.prepare_local()
        for options in (("--identity", "-"), ("--identity", IDENTITY), ("--identity", ""),
                        ("--bundle-id", LOCAL_BUNDLE_ID), ("--display-name", LOCAL_DISPLAY_NAME)):
            with self.subTest(options=options):
                self.assert_refused_before_staging(self.local_command(*options), returncode=64,
                                                  message="refuses identity and metadata overrides")
                self.assertFalse(self.destination.exists())

    def test_local_refuses_production_destination_before_staging(self):
        self.prepare_local()
        self.destination = self.home / "Applications/OVRCR Bridge.app"
        self.destination.parent.mkdir(mode=0o700)
        self.create_bundle(self.destination, "production-path-owned-fixture")
        before = bundle_snapshot(self.destination, include_inode=True)
        self.assert_refused_before_staging(self.local_command(), returncode=64,
                                          message="destination must be OVRCR Bridge Local.app")
        self.assertEqual(bundle_snapshot(self.destination, include_inode=True), before)

    def test_local_requires_fixed_profile_and_exact_true_marker(self):
        self.prepare_local()
        info_path = self.source / "Contents/Info.plist"
        original = plistlib.loads(info_path.read_bytes())
        for field, value in (("OVRCRBridgeLocalDevelopment", None), ("OVRCRBridgeLocalDevelopment", False),
                             ("OVRCRBridgeLocalDevelopment", 1), ("OVRCRBridgeLocalDevelopment", "true"),
                             ("CFBundleIdentifier", "com.ovrcr.bridge"), ("CFBundleDisplayName", "OVRCR")):
            with self.subTest(field=field, value=value):
                changed = dict(original)
                if value is None:
                    changed.pop(field)
                else:
                    changed[field] = value
                info_path.write_bytes(plistlib.dumps(changed))
                self.assert_refused_before_staging(self.local_command(), returncode=1,
                                                  message="AssertionError")
                self.assertFalse(self.destination.exists())

    def test_local_refuses_production_artifact(self):
        self.prepare_local()
        info_path = self.source / "Contents/Info.plist"
        info = plistlib.loads(info_path.read_bytes())
        info.pop("OVRCRBridgeLocalDevelopment")
        info.update(CFBundleIdentifier="com.ovrcr.bridge", CFBundleDisplayName="OVRCR")
        info_path.write_bytes(plistlib.dumps(info))
        self.assert_refused_before_staging(self.local_command(), returncode=1, message="AssertionError")

    def test_production_refuses_local_artifact_even_with_matching_metadata_overrides(self):
        self.prepare_local()
        command = ["/bin/sh", str(INSTALLER), "--identity", IDENTITY,
                   "--destination", str(self.destination), "--bundle-id", LOCAL_BUNDLE_ID,
                   "--display-name", LOCAL_DISPLAY_NAME, str(self.source)]
        self.assert_refused_before_staging(command, returncode=1, message="AssertionError")

    def test_production_still_refuses_ad_hoc_identity(self):
        self.create_bundle(self.source, "production-new")
        self.configure_tools()
        command = ["/bin/sh", str(INSTALLER), "--identity", "-",
                   "--destination", str(self.destination), "--bundle-id", self.bundle_id,
                   "--display-name", DISPLAY_NAME, str(self.source)]
        self.assert_refused_before_staging(command, returncode=64,
                                          message="An explicit non-ad-hoc --identity is required")

    def test_local_refuses_changed_callback_helper_before_staging(self):
        self.prepare_local()
        (self.source / "Contents/MacOS/ovrcr").write_bytes(b"changed callback helper")
        self.assert_refused_before_staging(self.local_command(), returncode=1, message="AssertionError")

    def test_local_refuses_changed_sound_before_staging(self):
        self.prepare_local()
        (self.source / "Contents/Resources/ovrcr-tap-v1.wav").write_bytes(b"changed sound")
        self.assert_refused_before_staging(self.local_command(), returncode=1, message="AssertionError")


if __name__ == "__main__":
    if not __debug__:
        raise SystemExit("Installer checks require assertions; invoke python3 -I without -O")
    unittest.main(verbosity=2)
