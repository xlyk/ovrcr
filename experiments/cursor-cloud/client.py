#!/usr/bin/env python3
"""Preflight only: pinned official Cursor SDK Bridge, no CreateAgent or Send."""
import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import platform
import re
import select
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
from decimal import Decimal, InvalidOperation
from contextlib import contextmanager

WORK_SECONDS = 840
TOTAL_SECONDS = 900
CLEANUP_SECONDS = 60
SHUTDOWN_RESERVE = 10
VERSION = "1.0.0"
SDK_VERSION = "1.0.40"
RELEASE = "v1.0.40"
PINS = {
    "darwin-arm64": "56f7f68a43ce73daa8d93c60b35b5301feb6a351e1dabfefe9873d80d56931cb",
    "darwin-x64": "cc1e1779e9768fc0db09a93d98dce51498e507ee26a24ae04c0dd21b21700ea6",
    "linux-arm64": "83fee2aa4ff753756c1993937e154961a69048620e66969ad36dfdd3f8439254",
    "linux-x64": "2939798aa62e201417d9e1bba1a373ccc1d8da65fc3d95db386204c27379ac80",
}
EFFECTS = {"fixture_change", "result_branch", "exact_run_cancel", "owned_local_shutdown"}
MAX_JSON = 1024 * 1024


class Failure(Exception):
    """Only fixed, nonsecret reasons may cross this boundary."""


class Arguments(argparse.ArgumentParser):
    def error(self, message):
        raise Failure("explicit --manifest and --archive inputs are required")


class Deadlines:
    def __init__(self):
        self.started = time.monotonic()
        self.work = self.started + WORK_SECONDS
        self.total = self.started + TOTAL_SECONDS
        self.current = self.work

    def arm(self, deadline):
        self.current = deadline
        signal.setitimer(signal.ITIMER_REAL, self.remaining())

    @contextmanager
    def limit(self, deadline):
        try:
            self.arm(deadline)
            yield
        finally:
            signal.setitimer(signal.ITIMER_REAL, 0)

    def remaining(self):
        remaining = self.current - time.monotonic()
        if remaining <= 0:
            raise Failure("absolute deadline exceeded")
        return remaining


def expired(signum, frame):
    raise Failure("absolute deadline exceeded")


def read_json(path):
    with open(path, "rb") as source:
        data = source.read(MAX_JSON + 1)
    if len(data) > MAX_JSON:
        raise Failure("JSON input exceeds limit")
    value = json.loads(data)
    if not isinstance(value, dict):
        raise Failure("JSON object required")
    return value


def validate_manifest(manifest):
    required = {"credential_ref", "repository_policy", "repository", "disposable", "source_sha",
                "model", "max_cost_usd", "allowed_effects", "limits"}
    if set(manifest) != required:
        raise Failure("manifest fields missing or unknown")
    if not isinstance(manifest["credential_ref"], str) or not re.fullmatch(
            r"env:[A-Z][A-Z0-9_]{0,127}", manifest["credential_ref"]):
        raise Failure("credential_ref must explicitly name one environment variable")
    repo = manifest["repository"]
    if not isinstance(repo, str) or not re.fullmatch(r"https://github\.com/[A-Za-z0-9_-]+/[A-Za-z0-9_.-]+", repo):
        raise Failure("exact GitHub repository URL required")
    policy = manifest["repository_policy"]
    if not isinstance(policy, dict) or set(policy) != {"owner", "repository"} or (
            policy["repository"] != repo or policy["owner"] != repo.split("/")[-2]):
        raise Failure("repository does not match the explicit repository policy")
    if manifest["disposable"] is not True:
        raise Failure("disposable repository declaration required")
    if not isinstance(manifest["source_sha"], str) or not re.fullmatch(r"[0-9a-f]{40}", manifest["source_sha"]):
        raise Failure("full lowercase source SHA required")
    model = manifest["model"]
    if not isinstance(model, str) or not re.fullmatch(r"[A-Za-z0-9_.:/-]{1,200}", model) or model.lower() in {"auto", "default", "latest", "local"}:
        raise Failure("exact model required; automatic selection is forbidden")
    try:
        amount = Decimal(manifest["max_cost_usd"])
        if not isinstance(manifest["max_cost_usd"], str) or not amount.is_finite() or amount <= 0:
            raise Failure("positive explicit USD ceiling required")
    except (InvalidOperation, TypeError, ValueError):
        raise Failure("positive explicit USD ceiling required") from None
    effects = manifest["allowed_effects"]
    if not isinstance(effects, list) or not all(isinstance(x, str) for x in effects) or len(effects) != len(EFFECTS) or set(effects) != EFFECTS:
        raise Failure("only the four approved experiment effects are allowed")
    if manifest["limits"] != {"CreateAgent": 1, "Send": 3} or any(
            type(x) is not int for x in manifest["limits"].values()):
        raise Failure("experiment limits must be one CreateAgent and three Send attempts")


def host_pin():
    host = {"Darwin": "darwin", "Linux": "linux"}.get(platform.system())
    arch = {"arm64": "arm64", "aarch64": "arm64", "x86_64": "x64"}.get(platform.machine())
    key = str(host) + "-" + str(arch)
    if key not in PINS:
        raise Failure("unsupported host architecture")
    return key, host, arch


def unpack_archive(archive, root, deadlines):
    key, host, arch = host_pin()
    digest = hashlib.sha256()
    with open(archive, "rb") as source:
        while chunk := source.read(1024 * 1024):
            deadlines.remaining()
            digest.update(chunk)
        if digest.hexdigest() != PINS[key]:
            raise Failure("reviewed archive checksum mismatch")
        source.seek(0)
        with tarfile.open(fileobj=source, mode="r:gz") as bundle:
            manifest_file = bundle.getmember("manifest.json")
            if not manifest_file.isfile() or manifest_file.size > 65536:
                raise Failure("invalid archive manifest")
            manifest = json.load(bundle.extractfile(manifest_file))
            expected = {"bridgeVersion": VERSION, "sdkVersion": SDK_VERSION, "os": host,
                        "arch": arch, "entrypoint": "bin/cursor-sdk-bridge", "protocol": "sdk.v1",
                        "distribution": "standalone"}
            if not isinstance(manifest, dict) or any(manifest.get(k) != v for k, v in expected.items()):
                raise Failure("archive manifest/version/architecture mismatch")
            member = bundle.getmember(expected["entrypoint"])
            if not member.isfile() or not 0 < member.size <= 256 * 1024 * 1024:
                raise Failure("invalid archive entrypoint")
            binary = root / "bridge"
            with bundle.extractfile(member) as source_binary, binary.open("xb") as target:
                while chunk := source_binary.read(1024 * 1024):
                    deadlines.remaining()
                    target.write(chunk)
            binary.chmod(0o700)
    return binary, {"release": RELEASE, "archive_sha256": digest.hexdigest(), "host": key,
                    "bridge_version": VERSION, "sdk_version": SDK_VERSION, "protocol": "sdk.v1"}


def ready_line(process, deadlines):
    data = bytearray()
    os.set_blocking(process.stderr.fileno(), False)
    while True:
        if not select.select([process.stderr], [], [], deadlines.remaining())[0]:
            raise Failure("bridge ready-line deadline exceeded")
        chunk = os.read(process.stderr.fileno(), 4096)
        if not chunk:
            raise Failure("bridge exited before ready-line handshake")
        data.extend(chunk)
        if len(data) > 65536:
            raise Failure("bridge startup line exceeds limit")
        while b"\n" in data:
            line, _, tail = data.partition(b"\n")
            data = bytearray(tail)
            prefix = b"cursor-sdk-bridge ready "
            if line.startswith(prefix):
                ready = json.loads(line[len(prefix):])
                if not isinstance(ready, dict):
                    raise Failure("bridge discovery must be a JSON object")
                return ready
            # Diagnostics and discovery are never captured: both may contain credentials.


def identify_token(ready, root):
    token_path = ready.get("authTokenFile")
    if not isinstance(token_path, str):
        raise Failure("bridge token-file identity missing")
    path = Path(token_path)
    if not path.is_absolute() or path.name != "auth-token" or path.parent.parent != root / "tmp" or not path.parent.name.startswith("cursor-sdk-bridge-"):
        raise Failure("bridge token-file ownership not established")
    directory = path.parent.lstat()
    if not stat.S_ISDIR(directory.st_mode) or stat.S_IMODE(directory.st_mode) != 0o700 or directory.st_uid != os.getuid():
        raise Failure("bridge token directory is not private and owned")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        identity = os.fstat(fd)
        if not stat.S_ISREG(identity.st_mode) or stat.S_IMODE(identity.st_mode) != 0o600 or identity.st_uid != os.getuid() or identity.st_nlink != 1:
            raise Failure("bridge token file is not private and owned")
        token = os.read(fd, 4097).strip().decode("ascii")
        if not re.fullmatch(r"[A-Za-z0-9_-]{16,4096}", token):
            raise Failure("bridge token file is invalid")
    finally:
        os.close(fd)
    return token, (path, identity.st_dev, identity.st_ino, directory.st_dev, directory.st_ino)


def endpoint(ready, process, root):
    expected = {"schemaVersion": 1, "serverVersion": VERSION, "pid": process.pid,
                "transport": "tcp", "protocol": "connect", "host": "127.0.0.1",
                "workspaceRef": str(root / "workspace"), "stateRoot": str(root / "state")}
    port = ready.get("port")
    if any(ready.get(k) != v for k, v in expected.items()) or type(port) is not int or not 0 < port < 65536 or ready.get("url") != "http://127.0.0.1:" + str(port):
        raise Failure("ready-line identity/loopback/workspace mismatch")
    return port


def rpc(port, bearer, service, method, body, deadlines):
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=deadlines.remaining())
    try:
        connection.request("POST", "/sdk.v1." + service + "/" + method,
                           body=json.dumps(body).encode(), headers={"Authorization": "Bearer " + bearer,
                           "Content-Type": "application/json", "Connect-Protocol-Version": "1"})
        response = connection.getresponse()
        data = response.read(MAX_JSON + 1)
        if len(data) > MAX_JSON:
            raise Failure("RPC response exceeds limit")
        decoded = json.loads(data) if data else {}
        if not isinstance(decoded, dict):
            raise Failure("RPC response must be a JSON object")
        return response.status, decoded
    except TimeoutError:
        raise Failure("absolute deadline exceeded during RPC I/O") from None
    finally:
        connection.close()


def checked_rpc(port, token, service, method, body, deadlines):
    status, value = rpc(port, token, service, method, body, deadlines)
    if status != 200:
        raise Failure("RPC rejected (HTTP " + str(status) + ")")
    return value


def cleanup(process, root, token_identity, port, token, deadlines, resources):
    end = min(time.monotonic() + CLEANUP_SECONDS, deadlines.total)
    reserve = min(SHUTDOWN_RESERVE, max(0, end - time.monotonic()))
    not_started = process is None and not resources.get("launch_attempted")
    resources["process_exit"] = "NOT STARTED" if not_started else "UNRESOLVED"
    resources["token_cleanup"] = "NOT CREATED" if not_started else "UNRESOLVED"
    try:
        with deadlines.limit(end - reserve):
            if process is not None and process.poll() is None and port is not None:
                checked_rpc(port, token, "SdkBridgeControlService", "Shutdown", {"graceSeconds": 0}, deadlines)
    except (Failure, OSError, ValueError, http.client.HTTPException):
        resources["shutdown_rpc"] = "UNCONFIRMED"
    with deadlines.limit(end):
        if process is not None:
            try:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=min(5, deadlines.remaining() / 2))
                    except subprocess.TimeoutExpired:
                        process.kill()
                        resources["forced_shutdown"] = True
                        process.wait(timeout=deadlines.remaining())
                resources["process_exit"] = "CONFIRMED"
                resources["exit_status"] = process.returncode
            except (Failure, OSError, subprocess.TimeoutExpired):
                resources["process_exit"] = "UNRESOLVED"
            finally:
                process.stderr.close()
        if resources["process_exit"] == "UNRESOLVED":
            resources["private_root_cleanup"] = "RETAINED; process exit unconfirmed"
            return
        if token_identity is not None:
            path, dev, ino, parent_dev, parent_ino = token_identity
            try:
                deadlines.remaining()
                current = path.lstat()
                parent = path.parent.lstat()
                if (current.st_dev, current.st_ino, parent.st_dev, parent.st_ino) != (dev, ino, parent_dev, parent_ino):
                    raise Failure("token-file identity changed")
                path.unlink()
                path.parent.rmdir()
                resources["token_cleanup"] = "REMOVED"
            except (Failure, OSError):
                resources["token_cleanup"] = "UNRESOLVED"
        if root is not None:
            try:
                deadlines.remaining()
                # Empty-directory removal preserves unexpected bridge state and unknown token files.
                for name in ("bridge", "workspace", "state", "home", "tmp"):
                    path = root / name
                    if name == "bridge":
                        path.unlink(missing_ok=True)
                    else:
                        path.rmdir()
                root.rmdir()
                resources["private_root_cleanup"] = "REMOVED"
            except (Failure, OSError):
                resources["private_root_cleanup"] = "RETAINED; inspect exact owned root"


def redact(value, secrets):
    if isinstance(value, dict):
        return {k: redact(v, secrets) for k, v in value.items()}
    if isinstance(value, list):
        return [redact(v, secrets) for v in value]
    if isinstance(value, str):
        for secret in secrets:
            if secret:
                value = value.replace(secret, "[REDACTED]")
    return value


def main(argv=None):
    deadlines = Deadlines()
    previous = signal.signal(signal.SIGALRM, expired)
    report = {"preflight": "FAIL", "stages": [], "attempts": {"CreateAgent": 0, "Send": 0},
              "coding": "NOT RUN", "control_recovery": "NOT RUN", "native_provider": "NOT RUN",
              "usage": "UNAVAILABLE; no zero-cost or hard remote spend-cap claim", "resources": {}}
    root = process = token_identity = port = None
    key = token = ""
    stage = "inputs"
    parser = Arguments(description=__doc__)
    parser.add_argument("--manifest", required=True, help="explicit approved nonsecret JSON manifest")
    parser.add_argument("--archive", required=True, help="reviewed v1.0.40 standalone archive; no downloads/fallback")
    try:
        with deadlines.limit(deadlines.work):
            args = parser.parse_args(argv)
            manifest = read_json(args.manifest)
            validate_manifest(manifest)
            key = os.environ.get(manifest["credential_ref"][4:], "")
            if not key or len(key) > 4096 or any(c.isspace() for c in key):
                raise Failure("selected credential is missing or invalid; no ambient fallback")
            report["requested"] = manifest
            report["stages"].append({"stage": stage, "verdict": "PASS"})
            stage = "pin"
            root = Path(tempfile.mkdtemp(prefix="ovrcr-cursor-poc-")).resolve()
            report["resources"]["private_root"] = str(root)
            for name in ("workspace", "state", "home", "tmp"):
                (root / name).mkdir(mode=0o700)
            binary, report["pin"] = unpack_archive(args.archive, root, deadlines)
            report["stages"].append({"stage": stage, "verdict": "PASS", "receipt": report["pin"]})
            stage = "startup"
            env = {"PATH": os.defpath, "HOME": str(root / "home"), "TMPDIR": str(root / "tmp"),
                   "TMP": str(root / "tmp"), "TEMP": str(root / "tmp"), "CURSOR_API_KEY": key,
                   "CURSOR_SDK_CLIENT_LANGUAGE": "python"}
            report["resources"]["launch_attempted"] = True
            process = subprocess.Popen([str(binary), "--host", "127.0.0.1", "--port", "0",
                                       "--workspace", str(root / "workspace"), "--state-root", str(root / "state")],
                                       cwd=root / "workspace", env=env, stdin=subprocess.DEVNULL,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            report["resources"]["pid"] = process.pid
            ready = ready_line(process, deadlines)
            port = endpoint(ready, process, root)
            token, token_identity = identify_token(ready, root)
            report["resources"]["token_file"] = str(token_identity[0])
            report["stages"].append({"stage": stage, "verdict": "PASS", "receipt": {"pid": process.pid, "port": port}})
            stage = "Ping"
            ping = checked_rpc(port, token, "SdkBridgeControlService", stage, {}, deadlines)
            if not isinstance(ping.get("message"), str) or not ping["message"]:
                raise Failure("Ping did not return a liveness message")
            report["stages"].append({"stage": stage, "verdict": "PASS"})
            stage = "GetVersion"
            version = checked_rpc(port, token, "SdkBridgeControlService", stage, {}, deadlines)
            if version.get("bridgeVersion") != VERSION or version.get("protocolVersion") != "sdk.v1":
                raise Failure("runtime bridge/protocol version mismatch")
            report["stages"].append({"stage": stage, "verdict": "PASS", "receipt": {"bridgeVersion": VERSION, "protocolVersion": "sdk.v1"}})
            stage = "bad_bearer"
            status, bad = rpc(port, "invalid-" + token, "SdkBridgeControlService", "Ping", {}, deadlines)
            if status != 401 or bad.get("code") != "unauthenticated":
                raise Failure("bad bridge bearer was not rejected as unauthenticated")
            report["stages"].append({"stage": stage, "verdict": "PASS", "receipt": "HTTP 401 / unauthenticated"})
            options = {"options": {"apiKey": key}}
            stage = "Me"
            user = checked_rpc(port, token, "SdkCursorService", stage, options, deadlines).get("user")
            if not isinstance(user, dict) or not str(user.get("userId", "")).isdigit():
                raise Failure("Me did not return credential context")
            report["stages"].append({"stage": stage, "verdict": "PASS", "receipt": "selected-key credential context received; not stable billing-principal proof"})
            stage = "ListModels"
            models = checked_rpc(port, token, "SdkCursorService", stage, options, deadlines).get("items", [])
            if not isinstance(models, list) or not any(isinstance(x, dict) and x.get("id") == manifest["model"] for x in models):
                raise Failure("exact requested model is unavailable")
            report["stages"].append({"stage": stage, "verdict": "PASS", "receipt": {"model": manifest["model"]}})
            stage = "ListRepositories"
            repos = checked_rpc(port, token, "SdkCursorService", stage, options, deadlines).get("items", [])
            if not isinstance(repos, list) or not any(isinstance(x, dict) and x.get("url") == manifest["repository"] for x in repos):
                raise Failure("exact permitted repository is unavailable")
            report["stages"].append({"stage": stage, "verdict": "PASS", "receipt": {"repository": manifest["repository"]}})
            report["preflight"] = "PASS"
    except Failure as error:
        report["stages"].append({"stage": stage, "verdict": "FAIL", "reason": str(error)})
    except (OSError, ValueError, KeyError, TypeError, tarfile.TarError, http.client.HTTPException):
        report["stages"].append({"stage": stage, "verdict": "FAIL", "reason": "invalid input or unavailable local/bridge operation; raw diagnostics suppressed"})
    finally:
        try:
            cleanup(process, root, token_identity, port, token, deadlines, report["resources"])
        except (Failure, OSError):
            report["resources"]["cleanup"] = "UNRESOLVED; cleanup deadline or operation failed"
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)
    report["elapsed_seconds"] = round(time.monotonic() - deadlines.started, 3)
    report = redact(report, [key, token])
    print(json.dumps(report, sort_keys=True))
    clean = report["resources"].get("process_exit") in {"NOT STARTED", "CONFIRMED"} and report["resources"].get("token_cleanup") in {"NOT CREATED", "REMOVED"}
    return 0 if report["preflight"] == "PASS" and clean else 1


if __name__ == "__main__":
    sys.exit(main())
