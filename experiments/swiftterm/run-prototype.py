#!/usr/bin/env python3
"""Build and run the disposable SwiftTerm host and local OVRCR fixture."""

from __future__ import annotations

import json
import os
import plistlib
import shutil
import stat
import subprocess
import sys
import tempfile
import time
from pathlib import Path

BUNDLE_ID = "dev.ovrcr.SwiftTermPrototype"
PRODUCT = "SwiftTermPrototype"
PROJECT = "swiftterm-fixture"
BRANCH = "swiftterm-prototype-fixture"
SHELLS = ("shell-one", "shell-two")
PROVIDERS = ("claude", "codex", "pi", "omp", "grok", "hermes", "cursor-agent")
TERM = "xterm-256color"
READY = "SWIFTERM_FIXTURE_READY="


def rtk_run(rtk: str, args: list[str], *, cwd: Path, env: dict[str, str] | None = None,
            capture: bool = False, timeout: float | None = None,
            check: bool = True) -> subprocess.CompletedProcess[str]:
    result = subprocess.run([rtk, "proxy", *args], cwd=cwd, env=env or os.environ.copy(),
                            text=True, capture_output=capture, timeout=timeout, check=False)
    if check and result.returncode:
        raise RuntimeError(f"command failed ({result.returncode}): {args!r}\n{result.stderr or result.stdout}")
    return result


def fixture_run(rtk: str, env: dict[str, str], args: list[str], *, cwd: Path,
                timeout: float = 30, check: bool = True) -> subprocess.CompletedProcess[str]:
    allowlist = [f"{key}={value}" for key, value in env.items()]
    result = rtk_run(rtk, ["env", "-i", *allowlist, *args], cwd=cwd, capture=True,
                     timeout=timeout, check=False)
    if check and result.returncode:
        raise RuntimeError(f"fixture command failed ({result.returncode}): {args!r}\n{result.stderr or result.stdout}")
    return result


def read_sessions(rtk: str, env: dict[str, str], cli: Path, cwd: Path) -> list[dict]:
    value = json.loads(fixture_run(rtk, env, [str(cli), "--json", "list"], cwd=cwd).stdout)
    if not isinstance(value, list):
        raise RuntimeError("OVRCR fixture inventory was not a JSON list")
    return value


def main() -> int:
    if sys.platform != "darwin":
        print("just swiftterm-prototype is available only on macOS", file=sys.stderr)
        return 64
    root_repo = Path(__file__).resolve().parents[2]
    rtk = shutil.which("rtk")
    if not rtk:
        print("rtk is required by the OVRCR development environment", file=sys.stderr)
        return 69

    target = Path(os.environ.get("CARGO_TARGET_DIR", "target"))
    target = (root_repo / target).resolve() if not target.is_absolute() else target.resolve()
    package = root_repo / "experiments/swiftterm/host"
    rtk_run(rtk, ["cargo", "build", "-p", "ovrcr", "--bin", "ovrcr", "--target-dir", str(target)], cwd=root_repo)
    swift_env = os.environ.copy()
    clt = Path("/Library/Developer/CommandLineTools")
    if "DEVELOPER_DIR" not in swift_env and clt.is_dir():
        swift_env["DEVELOPER_DIR"] = str(clt)
    rtk_run(rtk, ["xcrun", "swift", "--version"], cwd=root_repo, env=swift_env)
    rtk_run(rtk, ["xcrun", "swift", "build", "--package-path", str(package), "-c", "release",
                  "--product", PRODUCT], cwd=root_repo, env=swift_env)
    cli_source, app_source = target / "debug/ovrcr", package / ".build/release" / PRODUCT
    if not cli_source.is_file() or not os.access(cli_source, os.X_OK) \
            or not app_source.is_file() or not os.access(app_source, os.X_OK):
        raise RuntimeError("Rust or Swift build did not produce its expected executable")

    os.umask(0o077)
    root = Path(tempfile.mkdtemp(prefix="ovrcr-swiftterm-prototype.", dir="/private/tmp"))
    root.chmod(0o700)
    home, instance = root / "home", root / "ovrcr-home"
    socket, tmp = instance / "server.sock", root / "tmp"
    repo_fixture, workspaces = root / "repositories" / PROJECT, root / "workspaces"
    app_bundle, macos = root / f"{PRODUCT}.app", root / f"{PRODUCT}.app/Contents/MacOS"
    cli, app_executable = macos / "ovrcr", macos / PRODUCT
    marker, marker_text = instance / ".swiftterm-prototype", f"swiftterm-prototype\nroot={root}\nowner_pid={os.getpid()}\n"
    try:
        for path in (home, instance, tmp, repo_fixture, workspaces, macos):
            path.mkdir(parents=True, exist_ok=True, mode=0o700)
        marker.write_text(marker_text, encoding="utf-8")
        shutil.copy2(cli_source, cli)
        shutil.copy2(app_source, app_executable)
        with (app_bundle / "Contents/Info.plist").open("wb") as output:
            plistlib.dump({"CFBundleExecutable": PRODUCT, "CFBundleIdentifier": BUNDLE_ID,
                           "CFBundleName": PRODUCT, "CFBundlePackageType": "APPL",
                           "CFBundleVersion": "1", "CFBundleShortVersionString": "0.1",
                           "LSMinimumSystemVersion": "13.0", "NSHighResolutionCapable": True}, output)
    except BaseException:
        shutil.rmtree(root, ignore_errors=True)
        raise

    env = {"HOME": str(home), "OVRCR_HOME": str(instance), "OVRCR_SOCKET": str(socket),
           "OVRCR_SERVER_EXECUTABLE": str(cli), "TERM": TERM, "COLORTERM": "truecolor",
           "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
           "SHELL": "/bin/sh", "TMPDIR": str(tmp), "LANG": "en_US.UTF-8", "LC_CTYPE": "en_US.UTF-8",
           "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1", "GIT_TERMINAL_PROMPT": "0"}
    server: subprocess.Popen[bytes] | None = None
    app: subprocess.Popen[bytes] | None = None
    workspace: Path | None = None
    sessions: list[dict] = []
    socket_inode: int | None = None

    def cleanup() -> None:
        if not marker.is_file() or marker.read_text(encoding="utf-8") != marker_text:
            raise RuntimeError("fixture marker changed; preserving " + str(root))
        if root.parent.resolve() != Path("/private/tmp").resolve():
            raise RuntimeError("fixture root moved; preserving " + str(root))
        if server is None:
            if socket.exists() or socket.is_symlink():
                raise RuntimeError("fixture socket has no recorded owner; preserving " + str(root))
        elif server.poll() is None and socket_inode is not None:
            info = socket.lstat()
            if not stat.S_ISSOCK(info.st_mode) or info.st_uid != os.getuid() or info.st_ino != socket_inode:
                raise RuntimeError("fixture socket ownership changed; preserving " + str(root))
            fixture_run(rtk, env, [str(cli), "shutdown", "--kill"], cwd=workspace or repo_fixture, timeout=60)
            if server.wait(timeout=10) != 0 or socket.exists() or socket.is_symlink():
                raise RuntimeError("fixture Server did not stop cleanly; preserving " + str(root))
        elif server.poll() is None:
            server.terminate()
            server.wait(timeout=5)
            if socket.exists() or socket.is_symlink():
                raise RuntimeError("unverified fixture socket remains; preserving " + str(root))
        elif socket_inode is not None or socket.exists() or socket.is_symlink():
            raise RuntimeError("fixture Server exited unexpectedly; preserving " + str(root))
        if any(process_exists(item["pid"]) for item in sessions):
            raise RuntimeError("fixture shell still exists; preserving " + str(root))
        if not marker.is_file() or marker.read_text(encoding="utf-8") != marker_text:
            raise RuntimeError("fixture marker changed during cleanup; preserving " + str(root))
        shutil.rmtree(root)

    def stop_app() -> None:
        if app is not None and app.poll() is None:
            app.terminate()
            try:
                app.wait(timeout=5)
            except subprocess.TimeoutExpired:
                app.kill()
                app.wait()

    try:
        if marker.read_text(encoding="utf-8") != marker_text or any(
            shutil.which(name, path=env["PATH"]) for name in PROVIDERS
        ):
            raise RuntimeError("fixture ownership failed or a provider is present on the private PATH")
        fixture_run(rtk, env, ["infocmp", "-x", TERM], cwd=root)
        (repo_fixture / "FIXTURE.txt").write_text(
            "SwiftTerm local fixture\nKnown text: 東京 café e\u0301 🚀\nBorders: ┌──────────────┐\n         └──────────────┘\n",
            encoding="utf-8",
        )
        for args in (["git", "-C", str(repo_fixture), "init", "-b", "main"],
                     ["git", "-C", str(repo_fixture), "config", "user.name", "OVRCR SwiftTerm Fixture"],
                     ["git", "-C", str(repo_fixture), "config", "user.email", "swiftterm-fixture@example.invalid"],
                     ["git", "-C", str(repo_fixture), "add", "FIXTURE.txt"],
                     ["git", "-C", str(repo_fixture), "-c", "commit.gpgsign=false", "-c",
                      "core.hooksPath=/dev/null", "commit", "-m", "fixture"]):
            fixture_run(rtk, env, list(args), cwd=root)

        with (root / "server.log").open("wb") as log:
            server = subprocess.Popen([str(cli), "server"], cwd=repo_fixture, env=env,
                                      stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                                      start_new_session=True)
        for _ in range(200):
            if server.poll() is not None:
                raise RuntimeError(f"fixture Server exited early; see {root / 'server.log'}")
            try:
                info = socket.lstat()
                if stat.S_ISSOCK(info.st_mode) and fixture_run(
                    rtk, env, [str(cli), "--json", "list"], cwd=root, check=False
                ).returncode == 0:
                    if info.st_uid != os.getuid():
                        raise RuntimeError("fixture socket is not owned by this user")
                    socket_inode = info.st_ino
                    break
            except FileNotFoundError:
                pass
            time.sleep(0.05)
        if socket_inode is None:
            raise RuntimeError(f"fixture Server did not become ready; see {root / 'server.log'}")

        fixture_run(rtk, env, [str(cli), "project", "add", PROJECT, str(repo_fixture),
                               "--workspace-root", str(workspaces)], cwd=root)
        fixture_run(rtk, env, [str(cli), "workspace", "create", "--project", PROJECT,
                               "--new-branch", BRANCH, "--base", "main"], cwd=root)
        tree = read_sessions(rtk, env, cli, root)
        found = [w for p in tree if p.get("name") == PROJECT for w in p.get("workspaces", [])
                 if w.get("name") == BRANCH]
        if len(found) != 1:
            raise RuntimeError("could not locate the fixture Git workspace")
        workspace = Path(found[0]["path"]).resolve(strict=True)
        if not workspace.is_relative_to(workspaces.resolve()):
            raise RuntimeError("fixture workspace escaped its private root")
        for name in SHELLS:
            fixture_run(rtk, env, [str(cli), "new", "--project", PROJECT, "--workspace", BRANCH,
                                   "--name", name, "--", "/bin/sh", "-i"], cwd=workspace)
        tree = read_sessions(rtk, env, cli, workspace)
        sessions = [t for p in tree if p.get("name") == PROJECT for w in p.get("workspaces", [])
                    if w.get("name") == BRANCH for t in w.get("terminals", []) if t.get("name") in SHELLS]
        sessions.sort(key=lambda item: item["name"])
        if [t["name"] for t in sessions] != sorted(SHELLS):
            raise RuntimeError("fixture did not create exactly two local shells")
        sessions = [{"id": t["id"], "pid": t["pid"], "name": t["name"]} for t in sessions]

        while True:
            app = subprocess.Popen([str(app_executable)], cwd=workspace, env=env)
            record = {"root": str(root), "home": str(home), "ovrcr_home": str(instance),
                      "socket": str(socket), "app_bundle": str(app_bundle), "bundle_id": BUNDLE_ID,
                      "app_pid": app.pid, "cli_path": str(cli), "server_pid": server.pid,
                      "sessions": sessions, "cwd": str(workspace), "term": TERM, "env": env}
            print(READY + json.dumps(record, separators=(",", ":")), flush=True)
            status = app.wait()
            print("SWIFTERM_FIXTURE_APP_EXIT=" + json.dumps({"app_pid": app.pid, "status": status}), flush=True)
            print("Fixture Server remains live. Enter `reopen` or `teardown`.", flush=True)
            while True:
                command = sys.stdin.readline()
                if command == "reopen\n":
                    if server.poll() is None:
                        break
                    raise RuntimeError("fixture Server exited; preserving " + str(root))
                if command in ("teardown\n", ""):
                    cleanup()
                    print("SWIFTERM_FIXTURE_TORN_DOWN=" + json.dumps({"root": str(root)}), flush=True)
                    return 0
                print("Enter exactly `reopen` or `teardown`.", flush=True)

    except KeyboardInterrupt:
        stop_app()
        try:
            cleanup()
        except Exception as error:
            print(f"fixture retained at {root}: {error}", file=sys.stderr, flush=True)
        return 130
    except Exception as error:
        stop_app()
        try:
            cleanup()
        except Exception as cleanup_error:
            print(f"fixture retained at {root}: {cleanup_error}", file=sys.stderr)
        print(f"SwiftTerm prototype setup failed: {error}", file=sys.stderr)
        return 1


def process_exists(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


if __name__ == "__main__":
    raise SystemExit(main())
