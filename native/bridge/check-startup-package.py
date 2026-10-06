#!/usr/bin/env python3
"""Exercise a relocated runtime payload; never install or launch the Bridge."""
import hashlib
import json
import os
import plistlib
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
from pathlib import Path

assert __debug__, "Run python3 -I without -O"
arguments = sys.argv[1:]
local_development = bool(arguments and arguments[0] == "--local-development")
if local_development:
    arguments = arguments[1:]
if len(arguments) != 1:
    raise SystemExit("Usage: check-startup-package.py [--local-development] PAYLOAD_DIRECTORY")
payload = Path(arguments[0]).resolve()
app_name = "OVRCR Bridge Local.app" if local_development else "OVRCR Bridge.app"
profile = ["--local-development"] if local_development else []
with tempfile.TemporaryDirectory(prefix="ovrcr-startup-package-") as temporary:
    root = Path(temporary).resolve()
    moved = root / "outside-checkout"
    shutil.copytree(payload, moved)
    bundle = moved / app_name
    validator = moved / "native/bridge/validate-bundle.py"
    validation = [sys.executable, "-I", str(validator), *profile, str(bundle)]
    result = subprocess.run(validation, capture_output=True, timeout=10)
    assert result.returncode == 0, result.stderr.decode()
    info_path = bundle / "Contents/Info.plist"
    info_bytes = info_path.read_bytes()
    helper = bundle / "Contents/MacOS/ovrcr"
    helper.write_bytes(b"changed callback helper")
    info = plistlib.loads(info_bytes)
    info["OVRCRCallbackSHA256"] = hashlib.sha256(helper.read_bytes()).hexdigest()
    info_path.write_bytes(plistlib.dumps(info))
    result = subprocess.run(validation, capture_output=True, timeout=10)
    assert result.returncode != 0, "relocated validator accepted a changed helper and matching metadata"
    shutil.copy2(payload / app_name / "Contents/MacOS/ovrcr", helper)
    info_path.write_bytes(info_bytes)
    entitlements = bundle / "Contents/Resources/OVRCRBridge.entitlements"
    entitlements_bytes = entitlements.read_bytes()
    expected_entitlements_hash = hashlib.sha256(
        (Path(__file__).resolve().parents[2] / "native/bridge/entitlements.plist").read_bytes()).hexdigest()
    assert entitlements.is_file() and not entitlements.is_symlink()
    assert hashlib.sha256(entitlements_bytes).hexdigest() == expected_entitlements_hash
    assert json.loads(validator.with_name("expected-contract.json").read_text())["entitlements_sha256"] == expected_entitlements_hash
    legacy_entitlements = bundle / "Contents/OVRCRBridge.entitlements"
    assert not os.path.lexists(legacy_entitlements)
    layout_checks = 1
    entitlements.unlink()
    result = subprocess.run(validation, capture_output=True, timeout=10)
    assert result.returncode != 0, "relocated validator accepted missing entitlements"
    entitlements.write_bytes(entitlements_bytes)
    for case in ("changed", "legacy", "legacy_duplicate", "legacy_symlink", "resource_symlink"):
        try:
            if case == "changed":
                # Keep the plist semantics but change its pinned bytes.
                entitlements.write_bytes(entitlements_bytes + b"\n")
            elif case == "legacy":
                entitlements.rename(legacy_entitlements)
            elif case == "legacy_duplicate":
                legacy_entitlements.write_bytes(entitlements_bytes)
            elif case == "legacy_symlink":
                legacy_entitlements.symlink_to(root / "missing-entitlements")
            else:
                entitlements.unlink()
                entitlements.symlink_to(payload / app_name / "Contents/Resources/OVRCRBridge.entitlements")
            result = subprocess.run(validation, capture_output=True, timeout=10)
            assert result.returncode != 0, "relocated validator accepted entitlement layout: " + case
            layout_checks += 1
        finally:
            if os.path.lexists(legacy_entitlements):
                legacy_entitlements.unlink()
            if entitlements.is_symlink():
                entitlements.unlink()
            entitlements.write_bytes(entitlements_bytes)
    info = plistlib.loads(info_bytes)
    info["NSAppleEventsUsageDescription"] = "changed purpose"
    info_path.write_bytes(plistlib.dumps(info))
    result = subprocess.run(validation, capture_output=True, timeout=10)
    assert result.returncode != 0, "relocated validator accepted a changed purpose"
    info_path.write_bytes(info_bytes)
    sound = bundle / "Contents/Resources/ovrcr-tap-v1.wav"
    sound_bytes = sound.read_bytes()
    sound.write_bytes(b"changed")
    result = subprocess.run(validation, capture_output=True, timeout=10)
    assert result.returncode != 0, "relocated validator accepted a changed sound"
    result = subprocess.run(["sh", str(moved / "scripts/install-bridge.sh"), str(bundle)], capture_output=True, timeout=10)
    assert result.returncode == 64 and b"non-ad-hoc" in result.stderr
    checks = 6 + layout_checks
    if local_development:
        sound.write_bytes(sound_bytes)
        contract_path = moved / "native/bridge/expected-contract.json"
        contract_bytes = contract_path.read_bytes()
        contract = json.loads(contract_bytes)

        def reject_validation(command, reason):
            global checks
            result = subprocess.run(command, capture_output=True, timeout=10)
            assert result.returncode != 0, reason
            checks += 1

        reject_validation([sys.executable, "-I", str(validator), str(bundle)],
                          "relocated local payload accepted production validation mode")
        reject_validation([*validation, "com.ovrcr.bridge.local", "OVRCR Local"],
                          "local validator accepted arbitrary identity arguments")
        production_info = plistlib.loads(info_bytes)
        production_info.pop("OVRCRBridgeLocalDevelopment")
        production_info.update(CFBundleIdentifier="com.ovrcr.bridge", CFBundleDisplayName="OVRCR")
        info_path.write_bytes(plistlib.dumps(production_info))
        reject_validation([sys.executable, "-I", str(validator), str(bundle)],
                          "relocated contract did not pin local profile despite production metadata")
        reject_validation(validation, "local validator accepted production metadata")
        info_path.write_bytes(info_bytes)
        for field, value in (("local_development", False), ("local_development", 1),
                             ("bundle_id", "com.ovrcr.bridge"), ("display_name", "OVRCR")):
            changed = dict(contract, **{field: value})
            contract_path.write_text(json.dumps(changed))
            reject_validation(validation, "relocated contract accepted changed " + field)
        contract_path.write_bytes(contract_bytes)
        for value in (None, False, 1, "true"):
            changed = plistlib.loads(info_bytes)
            if value is None:
                changed.pop("OVRCRBridgeLocalDevelopment")
            else:
                changed["OVRCRBridgeLocalDevelopment"] = value
            info_path.write_bytes(plistlib.dumps(changed))
            reject_validation(validation, "relocated local payload accepted an invalid marker")
        info_path.write_bytes(info_bytes)

        # Exercise the actual cache entry point. A cache miss reaches only this
        # fail-closed fake builder, never Swift, signing or a native lifecycle.
        repo = Path(__file__).resolve().parents[2]
        bin_dir = root / "bin"
        bin_dir.mkdir(mode=0o700)
        for name, target in {"python3": str(Path(sys.executable).resolve()),
                             "dirname": "/usr/bin/dirname", "cat": "/bin/cat",
                             "mkdir": "/bin/mkdir", "mktemp": "/usr/bin/mktemp",
                             "rm": "/bin/rm"}.items():
            (bin_dir / name).symlink_to(target)
        config_path = root / "cache-check.json"
        builder_calls = root / "builder-calls.jsonl"
        fake_builder = root / "fake-builder.py"
        fake_builder.write_text('''import json, sys
from pathlib import Path
config = json.loads(Path(sys.argv[1]).read_text())
args = sys.argv[2:]
expected = [config["builder"], *config["profile"], "--callback-executable", config["callback"]]
assert args[:-1] == expected and len(args) == len(expected) + 1
stage = Path(args[-1])
assert stage.is_absolute() and stage == stage.resolve()
assert stage.name == "payload" and stage.parent.name.startswith(".ovrcr-startup-build.")
assert stage.parent.parent == Path(config["destination"]).parent
calls = Path(config["calls"])
assert not calls.exists()
calls.write_text(json.dumps(args) + "\\n")
raise SystemExit(77)
''')
        shell = " ".join(shlex.quote(str(value)) for value in
                         (Path(sys.executable).resolve(), "-I", fake_builder, config_path))
        (bin_dir / "sh").write_text('#!/bin/sh\nexec ' + shell + ' "$@"\n')
        (bin_dir / "sh").chmod(0o700)
        home = root / "home"
        home.mkdir(mode=0o700)
        environment = {"HOME": str(home), "TMPDIR": str(root) + "/", "PATH": str(bin_dir), "LC_ALL": "C"}
        cache_callback = root / "ovrcr"
        shutil.copy2(helper, cache_callback)

        def payload_bytes():
            return {str(path.relative_to(moved)): hashlib.sha256(path.read_bytes()).hexdigest()
                    for path in moved.rglob("*") if path.is_file()}

        def check_cache(requested_profile, expected_code):
            global checks
            config_path.write_text(json.dumps({"builder": str(repo / "scripts/build-bridge.sh"),
                                              "profile": requested_profile, "callback": str(cache_callback),
                                              "destination": str(moved), "calls": str(builder_calls)}))
            if builder_calls.exists():
                builder_calls.unlink()
            before = payload_bytes()
            command = ["/bin/sh", str(repo / "scripts/package-startup.sh"), *requested_profile,
                       str(moved), str(cache_callback)]
            process = subprocess.Popen(command, env=environment, stdin=subprocess.DEVNULL,
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
            try:
                stdout, stderr = process.communicate(timeout=30)
            except subprocess.TimeoutExpired:
                # Signal only this still-unreaped fixture child group.
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.communicate(timeout=5)
                raise AssertionError("Cache fixture timed out")
            processes = subprocess.run(["/bin/ps", "-axo", "pid=,pgid="], capture_output=True,
                                       text=True, timeout=5, check=True)
            remaining = [line for line in processes.stdout.splitlines()
                         if len(line.split()) == 2 and line.split()[1] == str(process.pid)]
            assert not remaining, ("cache fixture group remained", remaining)
            assert process.returncode == expected_code, (process.returncode, stdout, stderr)
            assert len(stdout) + len(stderr) <= 65536
            assert payload_bytes() == before, "Cache request modified an existing payload"
            assert list(root.glob(".ovrcr-startup-build.*")) == [], "Cache stage was not cleaned"
            if expected_code == 0:
                assert not builder_calls.exists(), "Valid local cache unnecessarily rebuilt"
            else:
                assert builder_calls.is_file(), "Cache miss did not reach the intercepted real builder call"
                recorded = json.loads(builder_calls.read_text())
                assert recorded[:-1] == [str(repo / "scripts/build-bridge.sh"), *requested_profile,
                                        "--callback-executable", str(cache_callback)]
            print(json.dumps({"cache_profile": requested_profile, "exit": process.returncode,
                              "owned_pid": process.pid, "owned_pgid": process.pid,
                              "owned_group_members": remaining, "builder_called": builder_calls.exists()}))
            checks += 1

        check_cache(["--local-development"], 0)
        # Make the cached metadata valid for production but retain the local
        # fingerprint. Only profile participation in the real fingerprint may
        # force this miss; a missing app or invalid contract cannot explain it.
        production_bundle = moved / "OVRCR Bridge.app"
        bundle.rename(production_bundle)
        (production_bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(production_info))
        contract_path.write_text(json.dumps(dict(contract, local_development=False,
                                                bundle_id="com.ovrcr.bridge", display_name="OVRCR")))
        result = subprocess.run([sys.executable, "-I", str(validator), str(production_bundle)],
                                capture_output=True, timeout=10)
        assert result.returncode == 0, "Converted production cache metadata must be valid before testing its fingerprint"
        checks += 1
        check_cache([], 77)
        production_bundle.rename(bundle)
        info_path.write_bytes(info_bytes)
        contract_path.write_bytes(contract_bytes)
        contract_path.write_text(json.dumps(dict(contract, local_development=False)))
        check_cache(["--local-development"], 77)  # Matching fingerprint cannot bypass the relocated profile pin.
        contract_path.write_bytes(contract_bytes)
print(f"{checks} relocated startup payload checks passed; pinned entitlement SHA256 {expected_entitlements_hash}; no install or app launch")
