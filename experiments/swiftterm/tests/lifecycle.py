#!/usr/bin/env python3
"""Run the normal SwiftTerm fixture and verify Dashboard ownership survives quit."""

from __future__ import annotations

import fcntl
import json
import os
import pty
import queue
import select
import struct
import subprocess
import tempfile
import termios
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
READY = "SWIFTERM_FIXTURE_READY="
TIMEOUT = 600  # SwiftPM's first pinned dependency build can be cold.
ROWS, COLS = 30, 100


class Log:
    def __init__(self) -> None:
        fd, name = tempfile.mkstemp(prefix="ovrcr-swiftterm-lifecycle-", suffix=".log", dir="/private/tmp")
        self.path = Path(name)
        self.file = os.fdopen(fd, "w", encoding="utf-8")
        self.lock = threading.Lock()

    def write(self, text: str) -> None:
        with self.lock:
            print(text, flush=True)
            self.file.write(text.rstrip("\n") + "\n")
            self.file.flush()


class Runner:
    def __init__(self, log: Log) -> None:
        env = os.environ.copy()
        for key in list(env):
            if key.startswith(("OVRCR_", "ANTHROPIC_", "CLAUDE_", "OPENAI_", "CHATGPT_", "XAI_", "GROK_", "GEMINI_", "OPENROUTER_")):
                env.pop(key, None)
        self.log = log
        self.proc = subprocess.Popen(
            ["just", "swiftterm-prototype"], cwd=ROOT, env=env, stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace",
        )
        self.ready: queue.Queue[dict | None] = queue.Queue()
        self.records: list[dict] = []
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()

    def _read(self) -> None:
        assert self.proc.stdout is not None
        for line in self.proc.stdout:
            self.log.write("runner> " + line.rstrip())
            if READY in line:
                self.ready.put(json.loads(line.split(READY, 1)[1]))
        self.ready.put(None)

    def record(self, number: int = 1) -> dict:
        while len(self.records) < number:
            try:
                record = self.ready.get(timeout=TIMEOUT)
            except queue.Empty as exc:
                raise RuntimeError("fixture did not emit its ready record") from exc
            if record is None:
                raise RuntimeError(f"fixture runner exited with {self.proc.poll()}")
            self.records.append(record)
        return self.records[number - 1]

    def send(self, line: str) -> None:
        assert self.proc.stdin is not None
        self.log.write("runner command> " + line)
        self.proc.stdin.write(line + "\n")
        self.proc.stdin.flush()

    def teardown(self) -> None:
        self.send("teardown")
        status = self.proc.wait(timeout=60)
        self.reader.join(timeout=5)
        if status:
            raise RuntimeError(f"fixture teardown exited with {status}")


def env_for(info: dict) -> dict[str, str]:
    env = dict(info["env"])
    env.update({
        "HOME": str(info["home"]),
        "OVRCR_HOME": str(info["ovrcr_home"]),
        "OVRCR_SOCKET": str(info["socket"]),
        "OVRCR_SERVER_EXECUTABLE": str(info["cli_path"]),
        "TERM": info.get("term", "xterm-256color"),
    })
    return env


def cli(info: dict, *args: str) -> str:
    result = subprocess.run(
        [str(info["cli_path"]), *args], cwd=info["cwd"], env=env_for(info),
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=30,
    )
    if result.returncode:
        raise RuntimeError(f"CLI {args} exited {result.returncode}: {result.stdout}")
    return result.stdout


def snapshot(info: dict, log: Log) -> tuple[dict[int, tuple[int, object]], str]:
    rows = json.loads(cli(info, "terminal", "list", "--json"))
    by_id = {int(row["id"]): row for row in rows}
    result = {}
    for seed in info["sessions"]:
        session_id, pid = int(seed["id"]), int(seed["pid"])
        row = by_id.get(session_id)
        if not row or row.get("kind") != "terminal" or int(row.get("pid") or 0) != pid:
            raise RuntimeError(f"seeded session {session_id} missing or changed: {row}")
        if not proc_stat(pid):
            raise RuntimeError(f"seeded shell process {pid} is not running")
        result[session_id] = (pid, row.get("run"))
    if not result:
        raise RuntimeError("fixture did not seed a local shell session")
    log.write("sessions> " + json.dumps({sid: identity for sid, identity in result.items()}, sort_keys=True))
    for session_id in result:
        size = json.loads(cli(info, "terminal", "read", str(session_id), "--json"))
        log.write(f"session {session_id} screen grid> {size['rows']}x{size['cols']}")
    return result, str(next(row.get("cwd") for row in rows if int(row["id"]) in result and row.get("cwd")))


def proc_stat(pid: int) -> str:
    return subprocess.run(["/bin/ps", "-p", str(pid), "-o", "stat="], capture_output=True, text=True).stdout.strip()


def wait_gone(pid: int) -> None:
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if not proc_stat(pid):
            return
        time.sleep(0.05)
    raise RuntimeError(f"process {pid} did not exit and get reaped")


def dashboard_child(info: dict) -> int:
    parent = int(info["app_pid"])
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        output = subprocess.run(["/bin/ps", "-axo", "pid=,ppid=,command="], capture_output=True, text=True, check=True).stdout
        children = []
        for line in output.splitlines():
            fields = line.strip().split(maxsplit=2)
            if len(fields) == 3 and int(fields[1]) == parent and Path(fields[2].split()[0]).name == "ovrcr":
                children.append(int(fields[0]))
        if len(children) == 1:
            return children[0]
        time.sleep(0.05)
    raise RuntimeError(f"expected one bundled Dashboard child under app {parent}, found {children}")


def pty_dashboard(info: dict, cwd: str, refusal: bool, log: Log) -> None:
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
    proc = subprocess.Popen([str(info["cli_path"])], cwd=cwd, env=env_for(info), stdin=slave, stdout=slave, stderr=slave)
    os.close(slave)
    output = bytearray()
    deadline = time.monotonic() + 30
    declined_startup = False
    try:
        while time.monotonic() < deadline:
            ready, _, _ = select.select([master], [], [], 0.1)
            if ready:
                try:
                    output.extend(os.read(master, 65536))
                except OSError:
                    break
            text = output.decode("utf-8", errors="replace")
            if not declined_startup and "OVRCR · startup" in text and "Connect agent status" in text and "Not now" in text:
                os.write(master, b"n\r")
                declined_startup = True
            if refusal and "another dashboard is already connected" in text:
                status = proc.wait(timeout=5)
                log.write(f"second Dashboard refusal> exit={status}; PTY={ROWS}x{COLS}")
                if status == 0:
                    raise RuntimeError("refused Dashboard exited successfully")
                return
            if not refusal and "BROWSE" in text:
                os.write(master, b"q")
                status = proc.wait(timeout=10)
                log.write(f"reattach CLI Dashboard> Browse and detach, exit={status}; PTY={ROWS}x{COLS}")
                if status:
                    raise RuntimeError(f"reattached CLI Dashboard exited with {status}")
                return
            if proc.poll() is not None:
                break
        text = output.decode("utf-8", errors="replace")[-2000:]
        if refusal:
            raise RuntimeError(f"second Dashboard did not show the expected refusal: {text}")
        raise RuntimeError(f"CLI Dashboard did not reach Browse mode: {text}")
    finally:
        if proc.poll() is None:
            try:
                os.write(master, b"q")
                proc.wait(timeout=3)
            except (OSError, subprocess.TimeoutExpired):
                proc.terminate()
                proc.wait(timeout=3)
        os.close(master)


def quit_app(info: dict, child: int, log: Log) -> None:
    command = ["/usr/bin/osascript", "-e", f'tell application id "{info["bundle_id"]}" to quit']
    result = subprocess.run(command, capture_output=True, text=True, timeout=30)
    log.write(f"AppKit quit Apple Event> exit={result.returncode} {result.stdout.strip()} {result.stderr.strip()}")
    if result.returncode:
        raise RuntimeError("standard AppKit quit event failed")
    wait_gone(int(info["app_pid"]))
    wait_gone(child)


def main() -> int:
    log = Log()
    runner, current, safe_to_teardown = None, None, False
    try:
        log.write("run> rtk python3 experiments/swiftterm/tests/lifecycle.py")
        log.write("Rust revision> " + subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip())
        lock = json.loads((ROOT / "experiments/swiftterm/host/Package.resolved").read_text())
        swiftterm = next(pin for pin in lock["pins"] if pin["identity"] == "swiftterm")
        log.write("SwiftTerm revision> " + swiftterm["state"]["revision"])
        log.write("macOS> " + subprocess.run(["/usr/bin/sw_vers", "-productVersion"], capture_output=True, text=True, check=True).stdout.strip())

        runner = Runner(log)
        first = runner.record()
        current = first
        before, cwd = snapshot(first, log)
        child = dashboard_child(first)
        pty_dashboard(first, cwd, True, log)
        log.write("PASS> real app attached and refused a second Dashboard")
        quit_app(first, child, log)
        safe_to_teardown = True
        if not proc_stat(int(first["server_pid"])) or not Path(first["socket"]).exists():
            raise RuntimeError("app quit stopped its fixture Server")
        after, _ = snapshot(first, log)
        if before != after:
            raise RuntimeError("session identities changed after app quit")
        pty_dashboard(first, cwd, False, log)
        log.write("PASS> same sessions survived and a real CLI Dashboard reattached")

        runner.send("reopen")
        safe_to_teardown = False
        second = runner.record(2)
        current = second
        if any(first[key] != second[key] for key in ("root", "socket", "server_pid", "sessions")):
            raise RuntimeError("reopen changed the fixture identity")
        if int(first["app_pid"]) == int(second["app_pid"]):
            raise RuntimeError("reopen reused the previous app process")
        child = dashboard_child(second)
        pty_dashboard(second, cwd, True, log)
        quit_app(second, child, log)
        safe_to_teardown = True
        final, _ = snapshot(second, log)
        if before != final:
            raise RuntimeError("session identities changed after app reopen and quit")

        root, socket, server = Path(second["root"]), Path(second["socket"]), int(second["server_pid"])
        runner.teardown()
        runner = None
        if root.exists() or socket.exists() or proc_stat(server):
            raise RuntimeError("explicit teardown left the fixture root, socket, or Server")
        log.write("RESULT> PASS; fixture removed after two real app quits")
        log.write(f"transcript> {log.path}")
        return 0
    except Exception as error:
        log.write(f"RESULT> FAIL: {type(error).__name__}: {error}")
        if runner is not None and runner.proc.poll() is None:
            if current and proc_stat(int(current["app_pid"])):
                try:
                    quit_app(current, dashboard_child(current), log)
                    safe_to_teardown = True
                except Exception as cleanup_error:
                    log.write(f"cleanup> {cleanup_error}")
            if safe_to_teardown:
                try:
                    runner.teardown()
                except Exception as cleanup_error:
                    log.write(f"fixture retained> {cleanup_error}")
            else:
                log.write("fixture retained> app or Dashboard child ownership is unresolved")
        log.write(f"transcript> {log.path}")
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
