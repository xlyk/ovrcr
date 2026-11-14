"""Cold-start the built app, quit it, and reattach to its surviving shell in a private HOME."""
import fcntl
import json
import os
import pty
import struct
import termios
from pathlib import Path
import subprocess
import sys
import tempfile
import time

bundle = Path(sys.argv[1]).resolve() / "Contents/MacOS"
cli = bundle / "ovrcr"
with tempfile.TemporaryDirectory(prefix="ovrcr-app-", dir="/private/tmp") as tmp:
    home = Path(tmp)
    state = home / "state"
    state.mkdir()
    socket = state / "server.sock"
    (state / "dashboard.toml").write_text("[quota]\nenabled = false\n")
    # Decline optional setup without changing any real Mac or provider settings.
    invalid_profile = home / "no-provider-profile"
    invalid_profile.touch()
    env = dict(HOME=tmp, OVRCR_HOME=str(state), OVRCR_SOCKET=str(socket), PATH="/usr/bin:/bin",
               SHELL="/bin/sh", LANG="en_US.UTF-8", NO_COLOR="1", OVRCR_BRIDGE_LOCAL_DEVELOPMENT="1",
               CLAUDE_CONFIG_DIR=str(invalid_profile), CODEX_HOME=str(invalid_profile))

    def control(*args):
        return subprocess.check_output([cli, *args], env=env, cwd=home, timeout=15, text=True)

    def launch():
        return subprocess.Popen([bundle / "OVRCRApp"], env=env, cwd=home)

    def quit_app(app):
        # Address the process this test started, even if the user's app is also running.
        result = subprocess.run(["/usr/bin/swift", "-e",
            "import AppKit; let app = NSRunningApplication(processIdentifier: " + str(app.pid)
            + "); exit(app?.terminate() == true ? 0 : 1)"], timeout=60)
        result.check_returncode()
        assert app.wait(timeout=15) == 0

    def dashboard_in_raw_mode(app):
        children = subprocess.check_output(["ps", "-axo", "ppid=,tty="], text=True)
        for line in children.splitlines():
            parent, tty = line.split()
            if parent != str(app.pid) or tty == "??":
                continue
            try:
                fd = os.open("/dev/" + tty, os.O_RDONLY | os.O_NOCTTY | os.O_NONBLOCK)
            except FileNotFoundError:
                continue
            try:
                return not (termios.tcgetattr(fd)[3] & termios.ICANON)
            finally:
                os.close(fd)
        return False

    def await_dashboard(app):
        deadline = time.monotonic() + 30
        errors = b""
        while time.monotonic() < deadline:
            if app.poll() is not None:
                raise RuntimeError(f"Native app exited during launch with {app.returncode}; last probe: {errors!r}")
            # Probe only after the real client owns the Dashboard and has entered raw mode.
            if socket.is_socket() and dashboard_in_raw_mode(app):
                # A real second Dashboard is refused even on an existing Server.
                master, slave = pty.openpty()
                try:
                    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 100, 0, 0))
                    with subprocess.Popen([cli], env=env, cwd=home, stdin=subprocess.DEVNULL,
                                          stdout=slave, stderr=subprocess.PIPE, start_new_session=True) as probe:
                        try:
                            _, errors = probe.communicate(timeout=2)
                        except subprocess.TimeoutExpired:
                            probe.terminate()
                            _, errors = probe.communicate(timeout=5)
                    if b"another dashboard is already connected" in errors:
                        return
                finally:
                    os.close(master)
                    os.close(slave)
            time.sleep(.1)
        raise RuntimeError(f"App failed to attach its Dashboard: {errors[-300:]!r}")

    app = launch()
    try:
        await_dashboard(app)
        repo = home / "sample"
        subprocess.run(["git", "init", "-q", "-b", "main", repo], check=True, env=env)
        subprocess.run(["git", "-c", "user.name=App Check", "-c", "user.email=test@example.invalid",
                        "commit", "-q", "--allow-empty", "-m", "fixture"], check=True, cwd=repo, env=env)
        control("project", "add", "sample", str(repo), "--workspace-root", str(home / "worktrees"))
        shell, = json.loads(control("terminal", "list", "--json"))
        assert shell["phase"] == "running" and shell["pid"] > 0
        quit_app(app)
        app = launch()
        await_dashboard(app)
        again, = json.loads(control("terminal", "list", "--json"))
        assert (shell["pid"], shell["run"], shell["id"]) == (again["pid"], again["run"], again["id"])
        quit_app(app)
        print("Cold start, Dashboard ownership, app quit and same-shell reattachment passed.")
    finally:
        if app.poll() is None:
            app.terminate()
            app.wait(timeout=10)
        if socket.is_socket():
            control("shutdown", "--kill")
