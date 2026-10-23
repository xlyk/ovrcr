#!/usr/bin/env python3
"""Bounded three-run Cursor cloud proof; --preflight-only checks catalogs."""
import argparse
import fcntl
import secrets
import shlex
import shutil
import struct
from contextlib import contextmanager
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
MAX_JOURNAL = 256 * 1024
GIT = "/usr/bin/git"
GH = shutil.which("gh")
SOURCE_PROBE = "git rev-parse HEAD && cat SOURCE_MARKER"
SELECTED_MARKER = "OVRCR_SELECTED_BASELINE_364\n"
DEFAULT_MARKER = "OVRCR_DEFAULT_TIP_364\n"
SDK_TYPES = {"system", "assistant", "user", "tool_call", "status", "thinking", "task", "usage"}
TERMINAL = {"RUN_LIFECYCLE_STATUS_FINISHED", "RUN_LIFECYCLE_STATUS_ERROR",
            "RUN_LIFECYCLE_STATUS_CANCELLED", "RUN_LIFECYCLE_STATUS_EXPIRED"}



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
        self.cleanup_deadline = None

    def cleanup_end(self):
        if self.cleanup_deadline is None:
            self.cleanup_deadline = min(time.monotonic() + CLEANUP_SECONDS, self.total)
        return self.cleanup_deadline

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



@contextmanager
def authority():
    previous = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGUSR1, signal.SIGUSR2})
    try:
        yield
    finally:
        signal.pthread_sigmask(signal.SIG_SETMASK, previous)


class Journal:
    """Private experiment evidence; no replay, recovery queue or second owner."""
    def __init__(self, path, secret_values):
        self.path = Path(path)
        self.secrets = secret_values
        self.closed = False
        self.closure_durable = False
        self.close_kind = None
        self.failed = False
        self.lock = None
        self.log = None
        if not self.path.is_absolute():
            raise Failure("absolute private --journal directory required")
        self.path.mkdir(mode=0o700, exist_ok=True)
        info = self.path.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
            raise Failure("journal directory must be private and owned")
        self.lock = os.open(self.path / "lock", os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        self.private_file(self.lock)
        try:
            fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise Failure("experiment already has an owner") from None
        self.log = os.open(self.path / "events.jsonl", os.O_WRONLY | os.O_APPEND | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        self.private_file(self.log)
        if os.fstat(self.log).st_size or set(x.name for x in self.path.iterdir()) != {"lock", "events.jsonl"}:
            raise Failure("retained experiment evidence forbids a new attempt; no replay")
        self.sync_directory()
        self.append({"event": "opened", "pid": os.getpid(), "time": time.time()})

    @staticmethod
    def private_file(fd):
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or stat.S_IMODE(info.st_mode) != 0o600 or info.st_uid != os.getuid() or info.st_nlink != 1:
            raise Failure("journal file must be regular, private and singly linked")

    def sync_directory(self):
        fd = os.open(self.path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)

    def encoded(self, value):
        if redact(value, self.secrets) != value:
            raise Failure("secret found in nonsecret evidence; publication refused")
        data = (json.dumps(value, sort_keys=True) + "\n").encode()
        # Reserve closure/cleanup receipts; never evict an unresolved request.
        size = sum(x.lstat().st_size for x in self.path.iterdir())
        if size + len(data) > MAX_JOURNAL - (0 if value.get("event") == "closure" else 4096):
            raise Failure("bounded journal capacity exhausted; evidence retained")
        return data

    def append(self, value):
        with authority():
            try:
                data = self.encoded(value)
                if os.write(self.log, data) != len(data):
                    raise OSError("short journal write")
                os.fsync(self.log)
            except (Failure, OSError):
                self.failed = True
                raise

    def ensure_open(self):
        if self.closed or self.failed:
            raise Failure("admission closed; earlier claims remain uncertain")

    def claim(self, method, request):
        with authority():
            self.ensure_open()
            name = method + ".request.json"
            value = {"event": "request", "method": method, "request": request}
            fd = os.open(self.path / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            try:
                data = self.encoded(value)
                if os.write(fd, data) != len(data):
                    raise OSError("short request write")
                os.fsync(fd)
            except (Failure, OSError):
                self.failed = True
                raise
            finally:
                os.close(fd)
            self.sync_directory()
            self.append({"event": "claim", "method": method, "operation_key": request["idempotencyKey"],
                         "request_sha256": hashlib.sha256(data).hexdigest(), "time": time.time()})

    def receipt(self, method, value):
        with authority():
            self.append({"event": "receipt", "method": method, "receipt": value, "late": self.closed})
            (self.path / (method + ".request.json")).unlink(missing_ok=True)
            self.sync_directory()

    def close(self, kind):
        with authority():
            if self.closed:
                return
            self.closed = True
            self.close_kind = kind
            self.append({"event": "closure", "kind": kind, "time": time.time(),
                         "remote_cancellation": "NOT CONFIRMED"})
            self.closure_durable = True
            if kind == "discard":
                for method in ("CreateAgent", "Send"):
                    (self.path / (method + ".request.json")).unlink(missing_ok=True)
                self.sync_directory()

    def release(self):
        if self.log is not None:
            os.close(self.log)
        if self.lock is not None:
            os.close(self.lock)


def command(argv, cwd, root, deadlines, resources, expected=0, extra_env=None):
    env = {"PATH": os.defpath, "HOME": str(root / "home"), "TMPDIR": str(root / "tmp"),
           "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
           "GIT_TERMINAL_PROMPT": "0", "PYTHONDONTWRITEBYTECODE": "1"}
    env.update(extra_env or {})
    receipt = {"pid": None, "exit": "UNRESOLVED", "launch_attempted": True}
    resources.setdefault("verification_processes", []).append(receipt)
    deadline = deadlines.current
    process = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
    receipt["pid"] = process.pid
    data = bytearray()
    termination_attempted = False
    try:
        os.set_blocking(process.stdout.fileno(), False)
        while True:
            if not select.select([process.stdout], [], [], deadlines.remaining())[0]:
                raise Failure("absolute deadline exceeded during verification")
            chunk = os.read(process.stdout.fileno(), 4096)
            if not chunk:
                break
            data.extend(chunk)
            if len(data) > MAX_JSON:
                raise Failure("verification command output exceeds limit")
        process.wait(timeout=deadlines.remaining())
        receipt["exit"] = process.returncode
        if process.returncode != expected:
            raise Failure("independent Git/fixture command failed; diagnostics suppressed")
        return data.decode("utf-8")
    finally:
        try:
            while True:
                try:
                    if receipt["exit"] == "UNRESOLVED":
                        end = deadlines.cleanup_end()
                    process.stdout.close()
                    if receipt["exit"] != "UNRESOLVED":
                        break
                    if not termination_attempted:
                        previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGALRM})
                        try:
                            termination_attempted = True
                            if process.poll() is None:
                                try:
                                    os.killpg(process.pid, signal.SIGKILL)
                                except OSError:
                                    pass  # A failed signal is not exit confirmation.
                        finally:
                            signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
                    # Retry only this owned child's cleanup, under one fixed end.
                    deadlines.arm(end - SHUTDOWN_RESERVE)
                    process.wait(timeout=deadlines.remaining())
                    receipt["exit"] = process.returncode
                    break
                except (Failure, subprocess.TimeoutExpired):
                    if receipt["exit"] != "UNRESOLVED":
                        raise
                    if time.monotonic() >= deadlines.cleanup_end() - SHUTDOWN_RESERVE:
                        break
        finally:
            if deadlines.cleanup_deadline is not None:
                if deadline > time.monotonic():
                    deadlines.arm(deadline)
                else:
                    signal.setitimer(signal.ITIMER_REAL, 0)
                    deadlines.current = deadline


def github_auth(config, login, repository, root, deadlines, resources):
    if not GH or not config or not Path(config).is_absolute() or not Path(config).is_dir() or not login or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9-]{0,38}", login):
        raise Failure("coding requires explicit --github-config, --github-login and installed gh")
    env = {"GH_CONFIG_DIR": config, "GH_PROMPT_DISABLED": "1", "GH_NO_UPDATE_NOTIFIER": "1"}
    query = '.hosts["github.com"] | map({login,active,state,tokenSource})'
    output = command([GH, "auth", "status", "--active", "--hostname", "github.com", "--json", "hosts", "--jq", query],
                     root, root, deadlines, resources, extra_env=env)
    accounts = json.loads(output)
    if not isinstance(accounts, list) or len(accounts) != 1 or not isinstance(accounts[0], dict) or accounts[0].get("login") != login or accounts[0].get("active") is not True or accounts[0].get("state") != "success":
        raise Failure("selected GitHub login is not the active qualified gh account")
    resources["github_config"] = config
    resources["github_login"] = login
    resources["github_repository"] = repository
    return {"host": "github.com", "login": login, "active": True,
            "credential_route": "selected gh config and git-credential; token never captured"}


def pull_requests(repository, root, deadlines, resources, journal):
    github_auth(resources["github_config"], resources["github_login"], repository, root, deadlines, resources)
    fields = "number,url,title,body,state,isDraft,headRefName,headRefOid,baseRefName,baseRefOid,updatedAt,labels,assignees,reviewRequests,milestone,autoMergeRequest,maintainerCanModify"
    env = {"GH_CONFIG_DIR": resources["github_config"], "GH_PROMPT_DISABLED": "1", "GH_NO_UPDATE_NOTIFIER": "1"}
    output = command([GH, "pr", "list", "--repo", repository, "--state", "all", "--limit", "100", "--json", fields],
                     root, root, deadlines, resources, extra_env=env)
    snapshot = json.loads(output)
    if not isinstance(snapshot, list) or len(snapshot) >= 100:
        raise Failure("independent PR snapshot unavailable or incomplete")
    numbers = set()
    for row in snapshot:
        if not isinstance(row, dict) or set(row) != set(fields.split(",")) or type(row.get("number")) is not int or row["number"] <= 0 or row["number"] in numbers:
            raise Failure("independent PR snapshot malformed")
        numbers.add(row["number"])
        if (any(not isinstance(row[name], str) for name in ("url", "title", "body", "state", "headRefName", "headRefOid", "baseRefName", "baseRefOid", "updatedAt")) or
                row["url"] != repository + "/pull/" + str(row["number"]) or row["state"] not in {"OPEN", "CLOSED", "MERGED"} or
                any(type(row[name]) is not bool for name in ("isDraft", "maintainerCanModify")) or
                any(not re.fullmatch(r"[0-9a-f]{40}", row[name]) for name in ("headRefOid", "baseRefOid")) or
                any(not row[name] for name in ("headRefName", "baseRefName", "updatedAt")) or
                any(not isinstance(row[name], list) for name in ("labels", "assignees", "reviewRequests")) or
                any(row[name] is not None and not isinstance(row[name], dict) for name in ("milestone", "autoMergeRequest"))):
            raise Failure("independent PR snapshot malformed")
    canonical = json.dumps(sorted(snapshot, key=lambda row: row["number"]), sort_keys=True)
    journal.append({"event": "github_pr_snapshot", "repository": repository, "count": len(snapshot),
                    "numbers": sorted(numbers), "sha256": hashlib.sha256(canonical.encode()).hexdigest()})
    return canonical


def git(argv, checkout, root, deadlines, resources):
    options = [GIT, "-c", "core.hooksPath=" + os.devnull]
    env = None
    if argv[0] in {"fetch", "ls-remote"}:
        repository = argv[-2] if argv[0] == "fetch" else argv[-1]
        if repository != resources["github_repository"]:
            raise Failure("Git credential helper is restricted to the approved repository")
        scope = "credential." + resources["github_repository"]
        options += ["-c", "credential.helper=", "-c", "credential.useHttpPath=true", "-c", scope + ".helper=!" + shlex.quote(GH) + " auth git-credential",
                    "-c", scope + ".username=" + resources["github_login"]]
        env = {"GH_CONFIG_DIR": resources["github_config"], "GH_PROMPT_DISABLED": "1", "GH_NO_UPDATE_NOTIFIER": "1"}
    return command([*options, *argv], checkout, root, deadlines, resources, extra_env=env).strip()


def fixture_source(manifest, root, deadlines, resources):
    checkout = root / "verification"
    checkout.mkdir(mode=0o700)
    git(["init", "-q"], checkout, root, deadlines, resources)
    remote = manifest["repository"]
    git(["fetch", "--quiet", "--no-tags", remote, manifest["source_sha"]], checkout, root, deadlines, resources)
    selected = git(["rev-parse", "FETCH_HEAD"], checkout, root, deadlines, resources)
    if selected != manifest["source_sha"]:
        raise Failure("independent source fetch returned another revision")
    git(["update-ref", "refs/heads/selected", selected], checkout, root, deadlines, resources)
    git(["fetch", "--quiet", "--no-tags", remote, "HEAD"], checkout, root, deadlines, resources)
    default = git(["rev-parse", "FETCH_HEAD"], checkout, root, deadlines, resources)
    marker = git(["show", default + ":SOURCE_MARKER"], checkout, root, deadlines, resources)
    if default == selected or marker != DEFAULT_MARKER.strip():
        raise Failure("default-tip marker is not the distinct public fixture marker")
    tree = git(["ls-tree", selected], checkout, root, deadlines, resources).splitlines()
    names = {line.split("\t")[-1] for line in tree if line.startswith("100644 blob ")}
    if names != {"addition.py", "test_addition.py", "SOURCE_MARKER"} or len(tree) != 3:
        raise Failure("selected fixture must contain exactly three regular source files")
    git(["checkout", "--quiet", "--detach", selected], checkout, root, deadlines, resources)
    fixture = Path(__file__).with_name("fixture")
    if any((checkout / name).read_bytes() != (fixture / name).read_bytes()
           for name in ("addition.py", "test_addition.py", "SOURCE_MARKER")):
        raise Failure("selected fixture differs from reviewed deliberately failing baseline")
    output = command([sys.executable, "-I", "-B", "-m", "unittest", "discover", "-s", str(checkout), "-p", "test_addition.py"], checkout, root, deadlines, resources, expected=1)
    if "Ran 3 tests" not in output or "FAILED (failures=2)" not in output:
        raise Failure("selected fixture did not demonstrate the expected failing tests")
    return {"source_sha": selected, "source_marker": SELECTED_MARKER.strip(),
            "default_tip_sha": default, "default_tip_marker": marker, "baseline_tests": "FAIL; 3 tests, 2 failures"}


def read_exact(response, length, deadlines):
    data = bytearray()
    while len(data) < length:
        deadlines.remaining()
        chunk = response.read(length - len(data))
        if not chunk:
            raise Failure("Connect stream ended without a complete frame/end receipt")
        data.extend(chunk)
    return bytes(data)


def send_stream(port, token, request, deadlines, method="Send"):
    payload = json.dumps(request).encode()
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=deadlines.remaining())
    try:
        connection.request("POST", "/sdk.v1.SdkAgentService/" + method, body=struct.pack(">BI", 0, len(payload)) + payload,
                           headers={"Authorization": "Bearer " + token,
                                    "Content-Type": "application/connect+json", "Connect-Protocol-Version": "1"})
        response = connection.getresponse()
        if response.status != 200 or response.getheader("Content-Type", "").split(";")[0] != "application/connect+json":
            raise Failure("stream transport rejected or invalid Connect content type")
        total = 0
        for _ in range(1024):
            flags, length = struct.unpack(">BI", read_exact(response, 5, deadlines))
            total += length + 5
            if flags not in (0, 2) or length > MAX_JSON or total > 2 * MAX_JSON:
                raise Failure("invalid/oversized Connect frame or stream limit")
            data = read_exact(response, length, deadlines)
            value = json.loads(data) if data else {}
            if not isinstance(value, dict):
                raise Failure("Connect frame requires a JSON object")
            if flags == 2:
                if "error" in value:
                    raise Failure("Connect end-stream error; remote outcome retained")
                if response.read(1):
                    raise Failure("Connect bytes after end-stream receipt")
                return
            yield value
        raise Failure("Connect frame count exceeds limit")
    finally:
        connection.close()


def valid_id(value):
    return isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_-]{1,200}", value)


def exact_snapshot(port, token, key, state, deadlines, journal, require_success=True):
    snapshot = checked_rpc(port, token, "SdkAgentService", "GetRun", {"runId": state["runId"], "options": {
        "runtime": "RUNTIME_CLOUD", "agentId": state["agentId"], "apiKey": key}}, deadlines).get("run")
    if not isinstance(snapshot, dict) or snapshot.get("agentId") != state["agentId"] or snapshot.get("runId") != state["runId"]:
        raise Failure("observation snapshot agent/run identity mismatch")
    status = snapshot.get("status")
    if status not in TERMINAL | {"RUN_LIFECYCLE_STATUS_RUNNING"}:
        raise Failure("observation snapshot lifecycle is unverified")
    journal.append({"event": "observation_snapshot", "receipt": snapshot})
    state["active"] = status not in TERMINAL
    if not state["active"]:
        state["terminal_status"] = status
        if require_success and status != "RUN_LIFECYCLE_STATUS_FINISHED":
            raise Failure("unsuccessful terminal snapshot forbids qualification and follow-up")
    return status


def stream_identity(envelope, state):
    for name in ("sdkMessage", "result", "done"):
        if name not in envelope:
            continue
        record = envelope[name]
        if name == "sdkMessage":
            if isinstance(record, dict) and record.get("type") not in SDK_TYPES:
                continue
            record = record.get("message") if isinstance(record, dict) else None
            fields = ("agent_id", "run_id")
        else:
            fields = ("agentId", "runId")
        if not isinstance(record, dict) or record.get(fields[0]) != state["agentId"] or record.get(fields[1]) != state["runId"]:
            raise Failure("observation stream agent/run identity mismatch")


def recover_stream(port, token, submission, deadlines, journal, state, key, report):
    original = send_stream(port, token, submission, deadlines)
    seen = set()
    try:
        for envelope in original:
            yield envelope
            seen.add(json.dumps({k: v for k, v in envelope.items() if k != "offset"}, sort_keys=True))
            if state.get("runId") and not journal.closed:
                report["active_stage"] = "observation"
                before = exact_snapshot(port, token, key, state, deadlines, journal)
                break
        else:
            return
    finally:
        original.close()
    report["recovery_criteria"]["active_observation"] = {"verdict": "BLOCKED", "reason": "observation not yet qualified"}
    journal.append({"event": "send_stream_dropped", "agentId": state["agentId"], "runId": state["runId"], "status": before})
    request = {"runId": state["runId"]}
    offsets = set()
    interrupted = False
    recovered = False
    attached_active = False
    # One beginning replay and at most one intentional durable-offset resume.
    for attachment in range(2):
        journal.append({"event": "observe_request", "agentId": state["agentId"], "request": request, "origin": "ObserveRun"})
        observer = send_stream(port, token, request, deadlines, "ObserveRun")
        try:
            for envelope in observer:
                stream_identity(envelope, state)
                encoded = json.dumps({k: v for k, v in envelope.items() if k != "offset"}, sort_keys=True)
                replay = attachment == 0 and encoded in seen
                journal.append({"event": "observe_event", "agentId": state["agentId"], "runId": state["runId"],
                                "replay": replay, "sha256": hashlib.sha256(encoded.encode()).hexdigest()})
                if not replay:
                    yield envelope
                if attachment == 0 and not replay and envelope.get("sdkMessage", {}).get("type") in SDK_TYPES and not attached_active and state["active"]:
                    attached_active = exact_snapshot(port, token, key, state, deadlines, journal) == "RUN_LIFECYCLE_STATUS_RUNNING"
                offset = envelope.get("offset")
                if offset is not None:
                    if not isinstance(offset, str) or not offset or len(offset) > 1024 or offset in offsets:
                        raise Failure("invalid or repeated exclusive ObserveRun offset")
                    offsets.add(offset)
                    journal.append({"event": "observe_offset", "origin": "ObserveRun", "agentId": state["agentId"],
                                    "runId": state["runId"], "offset": offset})
                    if attachment == 0 and not replay and envelope.get("sdkMessage", {}).get("type") in SDK_TYPES and attached_active and state["active"]:
                        request = {"runId": state["runId"], "afterOffset": offset}
                        interrupted = True
                        report["recovery_criteria"]["offset_resume"] = {"verdict": "BLOCKED", "reason": "cursor saved; resumed stream not yet qualified"}
                        break
            else:
                recovered = True
                break
        finally:
            observer.close()
    if not recovered:
        raise Failure("observation did not finish after bounded cursor resume")
    state["observation_receipt"] = {"before": before, "interrupted": interrupted, "attached_active": attached_active, "terminal_at_checkpoint": not attached_active and not state["active"], "request": request}


def collect_run(manifest, key, token, port, submission, journal, deadlines, report, state, coding):
    if report["attempts"]["Send"] >= 3:
        raise Failure("three-Send experiment exhausted; no retry")
    report["active_stage"] = "Send_claim"
    journal.claim("Send", submission)
    report["operation_keys"]["Send" if coding else "FollowUpSend"] = submission["idempotencyKey"]
    journal.ensure_open()
    report["active_stage"] = "Send"
    retain_run(report, state)
    state.pop("runId", None)
    state.pop("terminal_status", None)
    state["active"] = False
    state["send_attempted"] = True
    report["attempts"]["Send"] += 1
    result = None
    done = False
    source_proven = not coding
    probe_call = None
    for envelope in (recover_stream(port, token, submission, deadlines, journal, state, key, report) if coding else
                     send_stream(port, token, submission, deadlines)):
        if done:
            raise Failure("message after done; stream order invalid")
        if sum(field in envelope for field in ("sdkMessage", "result", "done", "interactionUpdate", "step")) > 1:
            raise Failure("multiple Connect envelope cases are invalid")
        sdk = envelope.get("sdkMessage")
        if "sdkMessage" in envelope and not isinstance(sdk, dict):
            raise Failure("SDK envelope payload must be an object")
        if isinstance(sdk, dict) and sdk.get("type") in SDK_TYPES:
            message = sdk.get("message")
            if not isinstance(message, dict):
                raise Failure("SDK message payload is invalid")
            if state.get("runId") is None:
                if sdk["type"] != "system" or message.get("subtype") != "init" or not valid_id(message.get("run_id")) or message.get("agent_id") != state["agentId"] or message.get("model") != {"id": manifest["model"]}:
                    raise Failure("early original-stream run identity/model missing or mismatched; unresolved")
                if not coding and message["run_id"] == report["run"]["runId"]:
                    raise Failure("follow-up reused the original run identity")
                identity = {"agentId": message["agent_id"], "runId": message["run_id"], "model": message["model"]}
                journal.receipt("Send", identity)
                state.update(identity, active=True)
                report["run" if coding else "follow_up_run"] = identity
            if message.get("agent_id") != state["agentId"] or message.get("run_id") != state["runId"]:
                raise Failure("stream agent/run identity mismatch")
            if sdk["type"] == "tool_call" and not source_proven:
                if message.get("name") != "shell" or message.get("args") != {"command": SOURCE_PROBE} or not valid_id(message.get("call_id")) or message.get("truncated"):
                    raise Failure("initial source probe is missing/unrecognized; source unverified")
                if message.get("status") == "running" and probe_call is None:
                    probe_call = message["call_id"]
                elif message.get("status") == "completed" and message["call_id"] == probe_call:
                    expected = {"stdout": manifest["source_sha"] + "\n" + SELECTED_MARKER, "stderr": "", "exitCode": 0}
                    observed = message.get("result")
                    if observed != expected or type(observed.get("exitCode")) is not int:
                        raise Failure("initial source probe differs from independently verified source")
                    journal.append({"event": "source_probe", "receipt": message})
                    source_proven = True
                    report["source_probe"] = {"verdict": "PASS", "call_id": probe_call, "receipt": "first correlated shell tool; exact HEAD and marker"}
                else:
                    raise Failure("source probe tool ordering/correlation mismatch")
            if sdk["type"] == "status" and message.get("status") in {"ERROR", "CANCELLED", "EXPIRED"}:
                state.update(active=False, terminal_status="RUN_LIFECYCLE_STATUS_" + message["status"])
                raise Failure("cloud run failed; status diagnostics suppressed")
        if "result" in envelope:
            if result is not None:
                raise Failure("duplicate terminal result")
            terminal = envelope["result"]
            result = terminal.get("result") if isinstance(terminal, dict) else None
            if not state.get("runId") or not isinstance(result, dict) or any(
                    record.get("agentId") != state["agentId"] or record.get("runId") != state["runId"]
                    for record in (terminal, result)):
                raise Failure("terminal agent/run identity missing or mismatched")
            journal.append({"event": "result", "receipt": terminal, "late": journal.closed})
            if terminal.get("status") != "RUN_LIFECYCLE_STATUS_FINISHED" or result.get("status") != terminal["status"] or result.get("model") != {"id": manifest["model"]} or terminal.get("errorCode") or result.get("errorCode"):
                status = result.get("status")
                state["active"] = not isinstance(status, str) or status not in TERMINAL
                if not state["active"]:
                    state["terminal_status"] = status
                raise Failure("run result/model is not successful FINISHED")
            state["active"] = False
            state["terminal_status"] = result["status"]
        if "done" in envelope:
            if result is None or envelope["done"] != {"agentId": state["agentId"], "runId": state["runId"]} or done:
                raise Failure("done identity/order mismatch")
            done = True
    if not result or not done or not source_proven:
        raise Failure("coding lacks source/result/done/successful stream end evidence")
    journal.ensure_open()
    if coding:
        recovery = state.get("observation_receipt")
        if not recovery:
            raise Failure("coding observation recovery is unverified")
        criteria = report["recovery_criteria"]
        if recovery["before"] in TERMINAL or recovery["terminal_at_checkpoint"]:
            criteria["completed_result"] = {"verdict": "PASS", "receipt": "terminal snapshot before Send drop or first observer checkpoint; exact-run replay finished"}
            criteria["active_observation"] = {"verdict": "NOT RUN" if recovery["before"] in TERMINAL else "BLOCKED", "reason": "completion preceded interruption or active observer checkpoint"}
        elif recovery["attached_active"]:
            criteria["active_observation"] = {"verdict": "PASS", "receipt": "exact RUNNING snapshots before Send drop and during ObserveRun; terminal stream recovered"}
        else:
            criteria["active_observation"] = {"verdict": "BLOCKED", "reason": "inconclusive; no active snapshot during observation"}
        criteria["offset_resume"] = ({"verdict": "PASS", "receipt": recovery["request"]} if recovery["interrupted"] else
                                     {"verdict": "NOT RUN", "reason": "no durable ObserveRun offset interrupted while active"})
        report["observation"] = "PASS"
    return result


def coding_run(manifest, key, token, port, journal, root, deadlines, report, state):
    report["coding"] = "BLOCKED"
    report["active_stage"] = "source_fixture"
    journal.ensure_open()
    source = fixture_source(manifest, root, deadlines, report["resources"])
    report["source"] = source
    journal.append({"event": "source", "receipt": source})
    creation = {"idempotencyKey": secrets.token_hex(16), "options": {
        "model": {"id": manifest["model"]}, "cloud": {
            "repos": [{"url": manifest["repository"], "startingRef": manifest["source_sha"]}],
            "workOnCurrentBranch": False, "autoCreatePr": False}}}
    report["active_stage"] = "CreateAgent_claim"
    journal.claim("CreateAgent", creation)
    report.setdefault("operation_keys", {})["CreateAgent"] = creation["idempotencyKey"]
    journal.ensure_open()
    report["active_stage"] = "CreateAgent"
    state["creation_attempted"] = True
    report["attempts"]["CreateAgent"] = 1
    response = checked_rpc(port, token, "SdkAgentService", "CreateAgent",
                           {**creation, "options": {**creation["options"], "apiKey": key}}, deadlines)
    if not valid_id(response.get("agentId")) or response.get("model") != {"id": manifest["model"]}:
        raise Failure("creation identity/model missing or mismatched; unresolved; no retry")
    journal.receipt("CreateAgent", response)
    state["agentId"] = response["agentId"]
    report["agent"] = response
    journal.ensure_open()
    report["active_stage"] = "GetAgent"
    agent = checked_rpc(port, token, "SdkAgentService", "GetAgent",
                        {"agentId": state["agentId"], "options": {"apiKey": key}}, deadlines)
    journal.append({"event": "GetAgent", "receipt": agent})
    info = agent.get("agent")
    if not isinstance(info, dict) or not isinstance(info.get("cloud"), dict):
        raise Failure("GetAgent cloud identity/repository mismatch")
    if agent.get("agent", {}).get("agentId") != state["agentId"] or "local" in agent.get("agent", {}) or agent.get("agent", {}).get("cloud", {}).get("repos") != [manifest["repository"]]:
        raise Failure("GetAgent cloud identity/repository mismatch")
    continuity = secrets.token_hex(16)
    journal.append({"event": "continuity_token", "token": continuity})
    report["continuity_token"] = continuity
    prompt = ("Before editing or any other tool, run exactly `" + SOURCE_PROBE + "` in the repository root. "
              "Report its starting SHA and public source marker. Fix only addition.py so addition is correct, "
              "run python3 -I -B -m unittest discover -s . -p test_addition.py, and publish the result branch. Do not change any other file "
              "or create a PR. Remember this harmless public continuity token for a later turn: " + continuity)
    submission = {"agentId": state["agentId"], "idempotencyKey": secrets.token_hex(16),
                  "message": {"text": prompt}, "options": {"model": {"id": manifest["model"]}}}
    result = collect_run(manifest, key, token, port, submission, journal, deadlines, report, state, True)
    report["stream"] = {"result": "FINISHED", "done": True, "end": "SUCCESS"}
    report["active_stage"] = "result_fetch"
    metadata = result.get("git")
    if not isinstance(metadata, dict):
        raise Failure("result repository/branch/PR metadata is invalid")
    branches = metadata.get("branches", [])
    if not isinstance(branches, list) or len(branches) != 1 or not isinstance(branches[0], dict) or branches[0].get("repoUrl") != manifest["repository"] or branches[0].get("prUrl"):
        raise Failure("result repository/branch/PR metadata is invalid")
    branch = branches[0].get("branch")
    if not isinstance(branch, str) or not re.fullmatch(r"[A-Za-z0-9_-]+(?:/[A-Za-z0-9_-]+)*", branch):
        raise Failure("result branch is missing or unsafe")
    checkout = root / "verification"
    git(["fetch", "--quiet", "--no-tags", manifest["repository"], "refs/heads/" + branch], checkout, root, deadlines, report["resources"])
    tip = git(["rev-parse", "FETCH_HEAD"], checkout, root, deadlines, report["resources"])
    git(["merge-base", "--is-ancestor", manifest["source_sha"], tip], checkout, root, deadlines, report["resources"])
    report["active_stage"] = "result_diff"
    diff = git(["diff", "--name-status", manifest["source_sha"], tip], checkout, root, deadlines, report["resources"])
    touched = git(["log", "--format=", "--name-only", manifest["source_sha"] + ".." + tip], checkout, root, deadlines, report["resources"])
    if diff != "M\taddition.py" or set(touched.split()) != {"addition.py"}:
        raise Failure("result changes files outside the permitted addition module")
    git(["checkout", "--quiet", "--detach", tip], checkout, root, deadlines, report["resources"])
    report["active_stage"] = "result_tests"
    output = command([sys.executable, "-I", "-B", "-m", "unittest", "discover", "-s", str(checkout), "-p", "test_addition.py"], checkout, root, deadlines, report["resources"])
    if "Ran 3 tests" not in output or "\nOK\n" not in output:
        raise Failure("independent fixture test completion is unverified")
    with authority():
        journal.ensure_open()
        report["result"] = {"branch": branch, "commit": tip, "ancestry": "PASS", "allowed_diff": diff, "tests": "PASS; 3 tests"}
        journal.append({"event": "verified", "receipt": report["result"]})
        report["stages"].append({"stage": "coding", "verdict": "PASS", "receipt": report["result"]})
        report["coding"] = "PASS"


def follow_up_run(manifest, key, token, port, journal, root, deadlines, report, state):
    report["active_stage"] = "follow_up"
    report["recovery_criteria"]["follow_up"] = {"verdict": "BLOCKED", "reason": "follow-up not yet qualified"}
    journal.ensure_open()
    checkout = root / "verification"
    refs = git(["ls-remote", manifest["repository"]], checkout, root, deadlines, report["resources"])
    prs = pull_requests(manifest["repository"], root, deadlines, report["resources"], journal)
    submission = {"agentId": state["agentId"], "idempotencyKey": secrets.token_hex(16),
                  "message": {"text": "Recall the harmless public continuity token from the earlier turn and the completed fix. "
                              "Do not change files, branches or PRs. Reply exactly on two lines: TOKEN=<earlier token> then "
                              "FIX=addition.py adds its two inputs."}, "options": {"model": {"id": manifest["model"]}}}
    result = collect_run(manifest, key, token, port, submission, journal, deadlines, report, state, False)
    report["active_stage"] = "follow_up"
    if result.get("result") != "TOKEN=" + report["continuity_token"] + "\nFIX=addition.py adds its two inputs.":
        raise Failure("follow-up recalled token/fix is incorrect or unverified")
    expected_git = {"branches": [{"repoUrl": manifest["repository"], "branch": report["result"]["branch"]}]}
    if result.get("git") != expected_git:
        raise Failure("follow-up changed repository/branch/PR metadata")
    after = git(["ls-remote", manifest["repository"]], checkout, root, deadlines, report["resources"])
    if after != refs or (report["result"]["commit"] + "\trefs/heads/" + report["result"]["branch"]) not in refs.splitlines():
        raise Failure("follow-up changed independently verified branch/file state")
    if pull_requests(manifest["repository"], root, deadlines, report["resources"], journal) != prs:
        raise Failure("follow-up changed independently verified PR state")
    with authority():
        journal.ensure_open()
        report["follow_up"] = {**report["follow_up_run"], "token": "CORRECT", "fix": "CORRECT", "result": "FINISHED",
                               "tests": "NO NEW FILES; original verified commit unchanged", "git_refs": "UNCHANGED; all advertised refs", "pull_requests": "UNCHANGED; independent all-state snapshot"}
        journal.append({"event": "follow_up_verified", "receipt": report["follow_up"]})
        report["recovery_criteria"]["follow_up"] = {"verdict": "PASS", "receipt": report["follow_up"]}
        report["stages"].append({"stage": "follow_up", "verdict": "PASS"})


def retain_run(report, state):
    if state.get("send_attempted"):
        record = {"agentId": state.get("agentId"), "runId": state.get("runId"),
                  "terminal": state.get("terminal_status", "UNRESOLVED"),
                  "identity": "DURABLE" if state.get("runId") else "UNRESOLVED; no replay"}
        runs = report.setdefault("runs", [])
        if runs and runs[-1]["runId"] == record["runId"]:
            runs[-1] = record
        else:
            runs.append(record)


def cancellation_run(manifest, key, token, port, journal, deadlines, report, state):
    report["active_stage"] = "cancellation_proof"
    report["cancellation"] = {"verdict": "BLOCKED", "reason": "exact active run and terminal cancellation not yet proven"}
    journal.ensure_open()
    if state.get("terminal_status") != "RUN_LIFECYCLE_STATUS_FINISHED" or report["attempts"]["Send"] != 2:
        raise Failure("third Send requires the two earlier successful terminal runs")
    submission = {"agentId": state["agentId"], "idempotencyKey": secrets.token_hex(16),
                  "message": {"text": "Run only a harmless shell command `sleep 90` (at most 90 seconds). "
                              "Do not edit files, publish branches or create PRs. This run will be cancelled."},
                  "options": {"model": {"id": manifest["model"]}}}
    journal.claim("Send", submission)
    report["operation_keys"]["CancellationSend"] = submission["idempotencyKey"]
    journal.ensure_open()
    retain_run(report, state)
    state.pop("runId", None)
    state.pop("terminal_status", None)
    state.update(active=False, send_attempted=True)
    report["attempts"]["Send"] += 1
    original = send_stream(port, token, submission, deadlines)
    activity = False
    try:
        for envelope in original:
            if sum(field in envelope for field in ("sdkMessage", "result", "done", "interactionUpdate", "step")) > 1:
                raise Failure("multiple Connect envelope cases are invalid")
            sdk = envelope.get("sdkMessage")
            if not state.get("runId"):
                if not isinstance(sdk, dict) or sdk.get("type") not in SDK_TYPES:
                    continue
                message = sdk.get("message")
                if not isinstance(message, dict) or sdk["type"] != "system" or message.get("subtype") != "init" or not valid_id(message.get("run_id")) or message.get("agent_id") != state["agentId"] or message.get("model") != {"id": manifest["model"]}:
                    raise Failure("third original-stream run identity/model missing or mismatched; unresolved")
                if message["run_id"] in {row["runId"] for row in report["runs"]}:
                    raise Failure("third Send reused an earlier run identity")
                identity = {"agentId": state["agentId"], "runId": message["run_id"], "model": message["model"]}
                journal.receipt("Send", identity)
                state.update(identity, active=True)
                report["cancellation_run"] = identity
            stream_identity(envelope, state)
            journal.ensure_open()
            if isinstance(sdk, dict) and sdk.get("type") in {"tool_call", "status"}:
                message = sdk.get("message", {})
                if message.get("status") in {"running", "RUNNING"}:
                    journal.append({"event": "cancellation_activity", "agentId": state["agentId"], "runId": state["runId"]})
                    activity = True
                    break
            if "result" in envelope or "done" in envelope:
                break
    finally:
        original.close()
    if not state.get("runId"):
        raise Failure("third Send has no exact durable run identity; no discovery or retry")
    if activity:
        journal.close("cancel")
        journal.append({"event": "cancel_intent", "agentId": state["agentId"], "runId": state["runId"]})
        state["cancel_attempted"] = True
        report["cancellation"]["request"] = "UNCONFIRMED"
        checked_rpc(port, token, "SdkAgentService", "CancelRun", {"agentId": state["agentId"], "runId": state["runId"]}, deadlines)
        report["cancellation"]["request"] = "ACKNOWLEDGED; not terminal proof"
    status = exact_snapshot(port, token, key, state, deadlines, journal, require_success=False)
    if status not in TERMINAL and activity:
        for envelope in send_stream(port, token, {"runId": state["runId"]}, deadlines, "ObserveRun"):
            if sum(field in envelope for field in ("sdkMessage", "result", "done", "interactionUpdate", "step")) > 1:
                raise Failure("multiple Connect envelope cases are invalid")
            stream_identity(envelope, state)
            if "result" in envelope:
                terminal = envelope["result"]
                result = terminal.get("result")
                if not isinstance(result, dict) or result.get("agentId") != state["agentId"] or result.get("runId") != state["runId"] or result.get("status") not in TERMINAL or result["status"] != terminal.get("status") or result.get("model") != {"id": manifest["model"]}:
                    raise Failure("cancellation terminal identity/status/model mismatch")
                journal.append({"event": "cancellation_result", "receipt": terminal, "late": journal.closed})
                state.update(active=False, terminal_status=result["status"])
    status = state.get("terminal_status", "UNRESOLVED")
    report["cancellation"].update(terminal=status, runId=state["runId"], activity=activity,
                                 verdict="PASS" if activity and status == "RUN_LIFECYCLE_STATUS_CANCELLED" else "BLOCKED",
                                 reason="authoritative CANCELLED" if activity and status == "RUN_LIFECYCLE_STATUS_CANCELLED" else "inconclusive completion race" if status == "RUN_LIFECYCLE_STATUS_FINISHED" else "cancellation unconfirmed")
    if status not in TERMINAL:
        raise Failure("cancellation remains unresolved; acknowledgement is not terminal proof")
    report["stages"].append({"stage": "cancellation_proof", "verdict": report["cancellation"]["verdict"]})


def cleanup(process, root, token_identity, port, token, deadlines, resources, journal=None, state=None, key=""):
    end = deadlines.cleanup_end()
    reserve = min(SHUTDOWN_RESERVE, max(0, end - time.monotonic()))
    not_started = process is None and not resources.get("launch_attempted")
    resources["process_exit"] = "NOT STARTED" if not_started else "UNRESOLVED"
    resources["token_cleanup"] = "NOT CREATED" if not_started else "UNRESOLVED"
    if journal is not None:
        uncertain_identity = (state.get("creation_attempted") and not state.get("agentId")) or (state.get("send_attempted") and not state.get("runId"))
        resources["remote_outcome"] = ("UNRESOLVED; an effect may have escaped without exact durable identity" if uncertain_identity else
                                       state.get("terminal_status", "NO KNOWN ACTIVE RUN") if not state.get("active") else "UNRESOLVED")
    try:
        with deadlines.limit(end - reserve):
            if journal is not None:
                try:
                    journal.close("cleanup")
                    if journal.closure_durable and not journal.failed and state.get("active") and state.get("runId") and journal.close_kind != "discard":
                        if not state.get("cancel_attempted"):
                            journal.append({"event": "cancel_intent", "agentId": state["agentId"], "runId": state["runId"]})
                            state["cancel_attempted"] = True
                            resources["cancel_request"] = "UNCONFIRMED"
                            checked_rpc(port, token, "SdkAgentService", "CancelRun", {"agentId": state["agentId"], "runId": state["runId"]}, deadlines)
                            resources["cancel_request"] = "ACKNOWLEDGED; not terminal proof"
                        snapshot = checked_rpc(port, token, "SdkAgentService", "GetRun", {"runId": state["runId"], "options": {
                            "runtime": "RUNTIME_CLOUD", "agentId": state["agentId"], "apiKey": key}}, deadlines).get("run", {})
                        journal.append({"event": "cancel_snapshot", "receipt": snapshot})
                        if isinstance(snapshot, dict) and snapshot.get("agentId") == state["agentId"] and snapshot.get("runId") == state["runId"] and isinstance(snapshot.get("status"), str) and snapshot["status"] in TERMINAL:
                            resources["remote_outcome"] = snapshot["status"]
                            state.update(active=False, terminal_status=snapshot["status"])
                except (Failure, OSError, ValueError, http.client.HTTPException):
                    resources["remote_cleanup"] = "UNCONFIRMED; exact identities and evidence retained"
                resources["journal_health"] = "FAILED; no further dispatch" if journal.failed else "DURABLE"
            try:
                if process is not None and process.poll() is None and port is not None:
                    checked_rpc(port, token, "SdkBridgeControlService", "Shutdown", {"graceSeconds": 0}, deadlines)
            except (Failure, OSError, ValueError, http.client.HTTPException):
                resources["shutdown_rpc"] = "UNCONFIRMED"
    except (Failure, OSError):
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
            if any(x["exit"] == "UNRESOLVED" for x in resources.get("verification_processes", [])):
                resources["private_root_cleanup"] = "RETAINED; verification process exit unconfirmed"
                return
            try:
                deadlines.remaining()
                # Empty-directory removal preserves unexpected bridge state and unknown token files.
                if (root / "verification").exists() and all(x["exit"] != "UNRESOLVED" for x in resources.get("verification_processes", [])):
                    shutil.rmtree(root / "verification")
                    resources["verification_cleanup"] = "REMOVED"
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
        return {redact(k, secrets): redact(v, secrets) for k, v in value.items()}
    if isinstance(value, list):
        return [redact(v, secrets) for v in value]
    if isinstance(value, str):
        for secret in secrets:
            if secret:
                value = value.replace(secret, "[REDACTED]")
    return value


def launch_bridge(binary, root, key, resources):
    env = {"PATH": os.defpath, "HOME": str(root / "home"), "TMPDIR": str(root / "tmp"),
           "TMP": str(root / "tmp"), "TEMP": str(root / "tmp"), "CURSOR_API_KEY": key,
           "CURSOR_SDK_CLIENT_LANGUAGE": "python"}
    resources["launch_attempted"] = True
    process = subprocess.Popen([str(binary), "--host", "127.0.0.1", "--port", "0",
                               "--workspace", str(root / "workspace"), "--state-root", str(root / "state")],
                               cwd=root / "workspace", env=env, stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    resources["pid"] = process.pid
    return process


def main(argv=None):
    deadlines = Deadlines()
    previous = signal.signal(signal.SIGALRM, expired)
    report = {"preflight": "FAIL", "stages": [], "attempts": {"CreateAgent": 0, "Send": 0},
              "coding": "NOT RUN", "control_recovery": "NOT RUN", "native_provider": "NOT RUN",
              "usage": "UNAVAILABLE; no zero-cost or hard remote spend-cap claim", "resources": {},
              "recovery_criteria": {name: {"verdict": "NOT RUN", "reason": "stage not reached"} for name in
                                    ("active_observation", "completed_result", "offset_resume", "follow_up")}}
    root = process = token_identity = port = journal = None
    state = {}
    old_controls = {}
    key = token = ""
    stage = "inputs"
    parser = Arguments(description=__doc__)
    parser.add_argument("--github-config", help="explicit existing gh config directory; read by selected credential helper only")
    parser.add_argument("--github-login", help="exact active GitHub login for independent Git fetches; separate from Cursor key")
    parser.add_argument("--journal", help="absolute owner-only evidence directory; retained, never replayed")
    parser.add_argument("--preflight-only", action="store_true", help="catalog checks only; zero CreateAgent/Send")
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
            if not args.preflight_only and not args.journal:
                raise Failure("explicit --journal required for a coding run")
            if args.preflight_only and args.journal:
                raise Failure("--journal belongs only to the coding path")
            if args.journal:
                stage = "journal"
                journal = Journal(args.journal, [key])
                report["resources"]["journal"] = str(journal.path)
                for sig, kind in ((signal.SIGUSR1, "cancel"), (signal.SIGUSR2, "discard")):
                    old_controls[sig] = signal.signal(sig, lambda signum, frame, kind=kind: journal.close(kind))
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
            process = launch_bridge(binary, root, key, report["resources"])
            ready = ready_line(process, deadlines)
            port = endpoint(ready, process, root)
            token, token_identity = identify_token(ready, root)
            if journal is not None:
                journal.secrets.append(token)
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
            if not args.preflight_only:
                report["coding"] = "BLOCKED"
                stage = "github_auth"
                report["github_auth"] = github_auth(args.github_config, args.github_login, manifest["repository"], root, deadlines, report["resources"])
                journal.append({"event": "github_auth", "receipt": report["github_auth"]})
                stage = "coding"
                coding_run(manifest, key, token, port, journal, root, deadlines, report, state)
                follow_up_run(manifest, key, token, port, journal, root, deadlines, report, state)
                cancellation_run(manifest, key, token, port, journal, deadlines, report, state)
                try:
                    usage = checked_rpc(port, token, "SdkAgentService", "GetUsage", {"agentId": state["agentId"]}, deadlines).get("usage")
                    if isinstance(usage, dict):
                        journal.append({"event": "usage", "receipt": usage})
                        report["usage"] = {"observed": usage, "cost": "AVAILABLE; float cents, eventually consistent" if isinstance(usage.get("cost"), dict) else "UNAVAILABLE; optional cost absent",
                                           "hard_remote_spend_cap": "UNVERIFIED"}
                except (Failure, OSError, ValueError, http.client.HTTPException):
                    pass
                retain_run(report, state)
                report["resume_options"] = {"verdict": "NOT RUN", "reason": "pinned ResumeAgent applies updated options; preservation unverified; no call"}
                report["fresh_bridge"] = {"verdict": "NOT RUN", "reason": "all three terminal runs and remaining original budget required"}
                if len(report["runs"]) == 3 and all(row["terminal"] in TERMINAL for row in report["runs"]):
                    report["active_stage"] = "fresh_bridge"
                    previous_bridge = {name: report["resources"][name] for name in ("pid", "token_file", "launch_attempted")}
                    cleanup(process, None, token_identity, port, token, deadlines, previous_bridge)
                    report["resources"]["previous_bridge"] = previous_bridge
                    if previous_bridge.get("process_exit") == "CONFIRMED" and previous_bridge.get("token_cleanup") == "REMOVED":
                        process = token_identity = port = None
                        token = ""
                        report["resources"]["launch_attempted"] = False
                        report["resources"].pop("pid", None)
                        report["resources"].pop("token_file", None)
                        deadlines.arm(min(deadlines.work, deadlines.cleanup_deadline - SHUTDOWN_RESERVE))
                        process = launch_bridge(binary, root, key, report["resources"])
                        ready = ready_line(process, deadlines)
                        port = endpoint(ready, process, root)
                        token, token_identity = identify_token(ready, root)
                        journal.secrets.append(token)
                        report["resources"]["token_file"] = str(token_identity[0])
                        snapshot_state = {**report["run"], "active": False}
                        try:
                            status = exact_snapshot(port, token, key, snapshot_state, deadlines, journal)
                            if status != "RUN_LIFECYCLE_STATUS_FINISHED":
                                raise Failure("fresh bridge did not retrieve the exact completed run")
                            report["fresh_bridge"] = {"verdict": "PASS", "runId": snapshot_state["runId"], "method": "GetRun",
                                                      "options": "read-only cloud routing; no remote option updates",
                                                      "active_continuation": "NOT RUN; completed-run retrieval only"}
                        except (Failure, OSError, ValueError, http.client.HTTPException):
                            report["fresh_bridge"] = {"verdict": "BLOCKED", "reason": "exact read-only retrieval unavailable; no ResumeAgent or new work"}
                    else:
                        report["fresh_bridge"] = {"verdict": "BLOCKED", "reason": "original owned process/token disposition unresolved; no replacement launched"}
    except Failure as error:
        report["stages"].append({"stage": report.pop("active_stage", stage), "verdict": "FAIL", "reason": str(error)})
    except (OSError, ValueError, KeyError, TypeError, subprocess.TimeoutExpired, tarfile.TarError, http.client.HTTPException):
        report["stages"].append({"stage": report.pop("active_stage", stage), "verdict": "FAIL", "reason": "invalid input or unavailable local/bridge operation; raw diagnostics suppressed"})
    finally:
        try:
            cleanup(process, root, token_identity, port, token, deadlines, report["resources"], journal, state, key)
        except (Failure, OSError):
            report["resources"]["cleanup"] = "UNRESOLVED; cleanup deadline or operation failed"
        retain_run(report, state)
        for sig, handler in old_controls.items():
            signal.signal(sig, handler)
        if journal is not None:
            report["resources"]["admission"] = ("CLOSED; durable closure" if journal.closure_durable else
                                                 "CLOSED LOCALLY; durable closure UNCONFIRMED" if journal.closed else
                                                 "UNCONFIRMED; durable closure not recorded")
            report["resources"]["journal_health"] = "FAILED; no further dispatch" if journal.failed else "DURABLE"
            journal.release()
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)
    report.pop("active_stage", None)
    report["proof_stages"] = {name: "NOT RUN" for name in ("observation", "follow_up", "cancellation_proof", "fresh_bridge", "live_cloud", "native_provider")}
    report["proof_stages"]["observation"] = report.get("observation", "NOT RUN")
    report["proof_stages"]["follow_up"] = report["recovery_criteria"]["follow_up"]["verdict"]
    for name, field in (("cancellation_proof", "cancellation"), ("fresh_bridge", "fresh_bridge")):
        report["proof_stages"][name] = report.get(field, {}).get("verdict", "NOT RUN")
    report["native_gates"] = {"terminal_response": "BLOCKED; no verified correlated cloud method", "uncertain_retry": "BLOCKED; no verified replay contract", "active_process_death": "NOT RUN"}
    report["remaining_remote"] = {"agentId": state.get("agentId", "UNRESOLVED" if state.get("creation_attempted") else "NOT CREATED"), "result_branch": report.get("result", {}).get("branch", "UNAVAILABLE"), "cleanup": "no remote archive/delete"}
    report["deadlines"] = {"work_seconds": WORK_SECONDS, "total_seconds": TOTAL_SECONDS, "clock": "absolute monotonic; no renewal",
                           "started_monotonic": deadlines.started, "work_deadline": deadlines.work,
                           "total_deadline": deadlines.total, "cleanup_deadline": deadlines.cleanup_deadline}
    report.setdefault("resume_options", {"verdict": "NOT RUN", "reason": "pinned ResumeAgent updates options; preservation unverified; no call"})
    if "cancellation" in report:
        report["cancellation"]["terminal"] = state.get("terminal_status", "UNRESOLVED")
    if report["coding"] == "PASS":
        report["control_recovery"] = "PASS" if all(report["recovery_criteria"][name]["verdict"] == "PASS" for name in ("active_observation", "offset_resume", "follow_up")) and report.get("cancellation", {}).get("verdict") == "PASS" and report.get("fresh_bridge", {}).get("verdict") == "PASS" else "BLOCKED"
    receipts = {
        "1": ("agent" in report, report.get("operation_keys", "no admitted request")),
        "2": ("source" in report, report.get("source", "fixture source unverified")),
        "3": (report["resources"].get("journal_health") == "DURABLE", report["resources"].get("journal", "no journal")),
        "4": (report["attempts"]["Send"] in {1, 2, 3}, report.get("agent", "creation/GetAgent unverified")),
        "5": ("stream" in report, {"run": report.get("run"), "stream": report.get("stream")}),
        "6": (report["coding"] == "PASS", report.get("result", "source/effect result unverified")),
        "7": ("UNRESOLVED" not in report["resources"].get("remote_outcome", "UNRESOLVED"), report["resources"].get("remote_outcome", "no remote identity receipt")),
        "8": (report["resources"].get("admission") == "CLOSED; durable closure", report["resources"].get("admission", "durable closure unconfirmed")),
        "9": (report["resources"].get("process_exit") == "CONFIRMED", {"deadlines": report["deadlines"], "cleanup": report["resources"]}),
    }
    report["criteria"] = {number: {"verdict": "PASS" if qualified else "NOT RUN" if report["coding"] == "NOT RUN" else "BLOCKED", "receipt": receipt}
                          for number, (qualified, receipt) in receipts.items()}
    report["criteria"]["10"] = {"verdict": "NOT RUN", "receipt": "offline entry-point suite is separate acceptance evidence; this invocation does not attest that suite"}
    report["elapsed_seconds"] = round(time.monotonic() - deadlines.started, 3)
    clean = report["resources"].get("process_exit") in {"NOT STARTED", "CONFIRMED"} and report["resources"].get("token_cleanup") in {"NOT CREATED", "REMOVED"}
    journal_ok = journal is None or (journal.closure_durable and not journal.failed)
    if not journal_ok:
        report["resources"]["journal_qualification"] = "FAIL; required persistence or durable closure is unconfirmed"
        if not any(item["verdict"] == "FAIL" for item in report["stages"]):
            report["stages"].append({"stage": "journal_cleanup", "verdict": "FAIL", "reason": "required journal persistence or durable closure is unconfirmed"})
    stages_ok = not any(item["verdict"] == "FAIL" for item in report["stages"])
    passed = report["preflight"] == "PASS" and report["coding"] in {"NOT RUN", "PASS"} and clean and journal_ok and stages_ok
    report["outcome"] = "PASS" if passed else "FAIL"
    report = redact(report, [key, token])
    print(json.dumps(report, sort_keys=True))
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
