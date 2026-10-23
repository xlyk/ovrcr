"""Offline acceptance at the standalone client entry point; no real credentials."""
import hashlib
import fcntl
import io
import json
import os
import platform
import runpy
import shutil
import tarfile
import tempfile
import time
import subprocess
import sys
import unittest
from pathlib import Path

CLIENT = Path(__file__).with_name("client.py")
PREFLIGHT_BUDGETS = (3, 5, 1.5, 0.75)
CODING_BUDGETS = (15, 20, 1.5, 0.75)


class PreflightEntryPointTests(unittest.TestCase):
    def run_fixture(self, scenario="success", mutate=None, pin_change=None, env_change=None, budgets=None, coding=False, process_delay=0):
        if budgets is None:
            budgets = CODING_BUDGETS if coding else PREFLIGHT_BUDGETS
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        root = Path(temp.name)
        audit = root / "audit.jsonl"
        unrelated = root / "unrelated-token"
        unrelated.write_text("UNRELATED-SECRET")
        manifest = {
            "credential_ref": "env:EXPLICIT_FIXTURE_KEY",
            "repository_policy": {"owner": "fixture", "repository": "https://github.com/fixture/disposable"},
            "repository": "https://github.com/fixture/disposable", "disposable": True,
            "source_sha": "a" * 40, "model": "fixture-model", "max_cost_usd": "1.00",
            "allowed_effects": ["fixture_change", "result_branch", "exact_run_cancel", "owned_local_shutdown"],
            "limits": {"CreateAgent": 1, "Send": 3},
        }
        self.case_root = root
        journal = root / "journal"
        config = {"audit": str(audit), "unrelated": str(unrelated), "scenario": scenario, "journal": str(journal)}
        git_override = ""
        if coding:
            repo = root / "remote"
            repo.mkdir()
            def git(*args):
                return subprocess.check_output(["/usr/bin/git", "-c", "user.name=Offline Fixture", "-c", "user.email=fixture@example.invalid", *args], cwd=repo, stderr=subprocess.STDOUT).decode().strip()
            git("init", "-b", "main")
            fixture = Path(__file__).with_name("fixture")
            for name in ("addition.py", "test_addition.py", "SOURCE_MARKER"):
                (repo / name).write_bytes((fixture / name).read_bytes())
            git("add", ".")
            git("commit", "-m", "deliberately failing selected source")
            selected = git("rev-parse", "HEAD")
            manifest["source_sha"] = selected
            (repo / "SOURCE_MARKER").write_text("OVRCR_DEFAULT_TIP_364\n")
            git("commit", "-am", "different default source marker")
            git("switch", "-c", "cursor/result", selected)
            (repo / "addition.py").write_text("def add(a, b):\n    return a + b\n")
            if scenario == "wrong_diff":
                (repo / "test_addition.py").write_text("raise RuntimeError('outside allowed module')\n")
            if scenario == "test_stall":
                program = "import json, os, time\nwith open(" + repr(str(root / "verification-stall.json")) + ", 'w') as receipt: json.dump({'phase': 'unittest_stall', 'pid': os.getpid()}, receipt)\ntime.sleep(30)\ndef add(a,b): return a+b\n"
                (repo / "addition.py").write_text(program)
            if scenario == "failed_tests":
                (repo / "addition.py").write_text("def add(a, b):\n    return a - b + 1\n")
            git("commit", "-am", "fix addition")
            if scenario == "wrong_ancestry":
                git("switch", "--orphan", "unrelated-root")
                for name in ("addition.py", "test_addition.py", "SOURCE_MARKER"):
                    (repo / name).write_bytes((fixture / name).read_bytes())
                (repo / "addition.py").write_text("def add(a, b):\n    return a + b\n")
                git("add", ".")
                git("commit", "-m", "unrelated source")
                git("branch", "-f", "cursor/result", "HEAD")
            git("switch", "main")
            gh_config = root / "github-config"
            gh_config.mkdir(mode=0o700)
            (gh_config / "owned-marker").write_text("preserve selected config")
            gh_peer = root / "gh-peer"
            gh_peer.write_text("#!" + sys.executable + "\n" + r"""import json,os,sys
from pathlib import Path
config = """ + repr(str(gh_config)) + r"""
scenario = """ + repr(scenario) + r"""
kind = 'pulls' if sys.argv[1:3] == ['pr', 'list'] else 'credential' if 'git-credential' in sys.argv else 'status'
with open(""" + repr(str(root / "github-audit.jsonl")) + r""", 'a') as audit:
    audit.write(json.dumps({'kind': kind, 'config_matches': os.environ.get('GH_CONFIG_DIR') == config, 'ambient_excluded': not (os.environ.get('GH_TOKEN') or os.environ.get('GITHUB_TOKEN')), 'pr_argv': sys.argv[1:] if kind == 'pulls' else None}) + '\n')
if kind == 'status':
    changed_account = scenario == 'pr_account_after' and Path(""" + repr(str(root / "pr-effects.json")) + r""").exists()
    print(json.dumps([{'login': 'other' if scenario == 'github_wrong_login' or changed_account else 'fixture', 'active': True, 'state': 'error' if scenario == 'github_auth_bad' else 'success', 'tokenSource': 'keyring'}]))
elif kind == 'pulls':
    request_shape = '--repo' in sys.argv and sys.argv[sys.argv.index('--repo') + 1] == 'https://github.com/fixture/disposable' and '--state' in sys.argv and sys.argv[sys.argv.index('--state') + 1] == 'all' and '--limit' in sys.argv and sys.argv[sys.argv.index('--limit') + 1] == '100'
    assert request_shape
    after = Path(""" + repr(str(root / "pr-effects.json")) + r""").exists()
    phase = 'after' if after else 'before'
    if scenario == 'pr_unavailable_' + phase:
        sys.exit(1)
    if scenario == 'pr_malformed_' + phase:
        print('{}')
        sys.exit(0)
    if scenario == 'pr_bad_record_' + phase:
        print('[{}]')
        sys.exit(0)
    if scenario == 'pr_incomplete_' + phase:
        print(json.dumps([{}] * 100))
        sys.exit(0)
    if scenario == 'pr_stall_' + phase:
        import time
        Path(""" + repr(str(root / "verification-stall.json")) + r""").write_text(json.dumps({'phase': 'pr_inventory_stall', 'pid': os.getpid()}))
        time.sleep(30)
    entry = {'number': 1, 'url': 'https://github.com/fixture/disposable/pull/1', 'title': 'before', 'body': '', 'state': 'OPEN', 'isDraft': True, 'headRefName': 'cursor/result', 'headRefOid': 'a' * 40, 'baseRefName': 'main', 'baseRefOid': 'b' * 40, 'updatedAt': '2026-10-23T00:00:00Z', 'labels': [], 'assignees': [], 'reviewRequests': [], 'milestone': None, 'autoMergeRequest': None, 'maintainerCanModify': False}
    if scenario == 'follow_up_external_pr' and after:
        print(json.dumps([entry]))
    elif scenario == 'follow_up_pr_edited':
        entry['title'] = 'after' if after else 'before'
        print(json.dumps([entry]))
    else:
        print('[]')
else:
    request = sys.stdin.read()
    if scenario == 'github_helper_refused' or 'host=github.com' not in request:
        sys.exit(1)
    print('username=fixture\npassword=GITHUB-SECRET-SENTINEL-364\n')
""")
            gh_peer.chmod(0o700)
            shim = root / "git-peer"
            shim.write_text("#!" + sys.executable + "\n" + r"""import json,os,subprocess,sys,time
args = sys.argv[1:]
delay = """ + repr(process_delay) + r"""
if delay:
    position = 0
    while args[position] == '-c':
        position += 2
    command = args[position]
    for phase in ('waiting', 'progressed'):
        if phase == 'progressed':
            time.sleep(delay)
        with open(""" + repr(str(root / "process-latency.jsonl")) + r""", 'a') as audit:
            audit.write(json.dumps({'phase': phase, 'command': command, 'pid': os.getpid(), 'delay_seconds': delay, 'time': time.monotonic()}) + '\n')
if 'fetch' in args or 'ls-remote' in args:
    operation = 'fetch' if 'fetch' in args else 'ls-remote'
    options = args[:args.index(operation)]
    auth = subprocess.run(['/usr/bin/git', *options, 'credential', 'fill'], input='protocol=https\nhost=github.com\npath=fixture/disposable\n\n', text=True, capture_output=True)
    supplied = auth.returncode == 0 and 'password=GITHUB-SECRET-SENTINEL-364' in auth.stdout
    other = subprocess.run(['/usr/bin/git', *options, 'credential', 'fill'], input='protocol=https\nhost=github.com\npath=unrelated/other\n\n', text=True, capture_output=True)
    unrelated_refused = other.returncode != 0 and 'GITHUB-SECRET-SENTINEL-364' not in other.stdout
    with open(""" + repr(str(root / "git-auth-audit.jsonl")) + r""", 'a') as audit:
        audit.write(json.dumps({'credential_supplied': supplied, 'refused': auth.returncode != 0, 'other_remote_refused': unrelated_refused}) + '\n')
    if not supplied:
        sys.exit(1)
os.execv('/usr/bin/git', ['git', '-c', """ + repr("url." + str(repo) + ".insteadOf=https://github.com/fixture/disposable") + r""", *args])
""")
            if scenario in {"git_stall", "verification_reap_delayed", "verification_reap_exhausted", "verification_reap_interrupted", "verification_reap_transition", "verification_reap_termination"}:
                program = "import json, os, sys, time\nif 'fetch' in sys.argv:\n with open(" + repr(str(root / "verification-stall.json")) + ", 'w') as receipt: json.dump({'phase': 'git_fetch_stall', 'pid': os.getpid()}, receipt)\n time.sleep(30)\nos.execv('/usr/bin/git', ['git', *sys.argv[1:]])\n"
                shim.write_text("#!" + sys.executable + "\n" + program)
            shim.chmod(0o700)
            git_override = "g['GIT']=" + repr(str(shim)) + "; g['GH']=" + repr(str(gh_peer)) + "; "
            config.update(source_sha=selected, repo=str(repo))
            if scenario == "unpublished_source":
                manifest["source_sha"] = "f" * 40
            if scenario == "same_default_source":
                git("reset", "--hard", selected)
            if scenario == "wrong_baseline":
                git("switch", "-c", "wrong-baseline", selected)
                (repo / "addition.py").write_text("def add(a,b): return a+b\n")
                git("commit", "-am", "not a failing baseline")
                manifest["source_sha"] = git("rev-parse", "HEAD")
                git("switch", "main")
        if scenario in {"competing_owner", "retained_journal"}:
            journal.mkdir(mode=0o700)
            lock = os.open(journal / "lock", os.O_RDWR | os.O_CREAT, 0o600)
            self.addCleanup(os.close, lock)
            if scenario == "competing_owner":
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            else:
                fd = os.open(journal / "events.jsonl", os.O_WRONLY | os.O_CREAT, 0o600)
                os.write(fd, b'{"event":"prior-unresolved-claim"}\n')
                os.close(fd)
        if mutate:
            mutate(manifest)
        manifest_path = root / "manifest.json"
        manifest_path.write_text(json.dumps(manifest))
        code = Path(__file__).with_name("fake_bridge.py").read_text().replace(
            "FIXTURE_CONFIG", repr(config))
        code = "#!" + sys.executable + "\n" + code
        host = {"Darwin": "darwin", "Linux": "linux"}[platform.system()]
        arch = {"arm64": "arm64", "aarch64": "arm64", "x86_64": "x64"}[platform.machine()]
        pin_manifest = {"bridgeVersion": "1.0.0", "sdkVersion": "1.0.40", "os": host,
                        "arch": arch, "entrypoint": "bin/cursor-sdk-bridge", "protocol": "sdk.v1",
                        "distribution": "standalone"}
        if pin_change:
            pin_change(pin_manifest)
        archive = root / "bridge.tar.gz"
        with tarfile.open(archive, "w:gz") as out:
            for name, data in [("bin/cursor-sdk-bridge", code.encode()), ("manifest.json", json.dumps(pin_manifest).encode())]:
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mode = 0o700
                out.addfile(info, io.BytesIO(data))
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        if scenario == "checksum_mismatch":
            digest = "0" * 64
        runner = """
import runpy, sys, time, os, signal, json
from pathlib import Path
module = runpy.run_path(sys.argv[1])
g = module["main"].__globals__
g.update(PINS={sys.argv[2]:sys.argv[3]}, WORK_SECONDS=%r, TOTAL_SECONDS=%r,
         CLEANUP_SECONDS=%r, SHUTDOWN_RESERVE=%r)
%s
original_spawn = g["subprocess"].Popen
original_killpg = g["os"].killpg
owned = []
scenario = %r
def tracked_spawn(*args, **kwargs):
    process = original_spawn(*args, **kwargs)
    owned.append(process)
    if scenario in {"verification_reap_delayed", "verification_reap_exhausted", "verification_reap_interrupted", "verification_reap_transition", "verification_reap_termination"} and args[0][0] == g["GIT"] and "fetch" in args[0]:
        original_wait = process.wait
        group = os.getpgid(process.pid)
        interrupted = False
        def tracked_killpg(pgid, sig):
            nonlocal interrupted
            if scenario == "verification_reap_termination" and not interrupted:
                interrupted = True
                blocked = signal.SIGALRM in signal.pthread_sigmask(signal.SIG_BLOCK, set())
                with (journal_path.parent / "verification-reaping.jsonl").open("a") as gate:
                    gate.write(json.dumps({"event": "stale_alarm", "boundary": "killpg", "pid": process.pid, "pgid": pgid, "time": time.monotonic(), "signal_blocked": blocked}) + chr(10))
                os.kill(os.getpid(), signal.SIGALRM)
            with (journal_path.parent / "verification-reaping.jsonl").open("a") as gate:
                gate.write(json.dumps({"event": "termination_attempt", "pid": process.pid, "pgid": pgid, "signal": sig, "time": time.monotonic()}) + chr(10))
            return original_killpg(pgid, sig)
        g["os"].killpg = tracked_killpg
        original_close = process.stdout.close
        def interrupted_close():
            nonlocal interrupted
            if scenario == "verification_reap_transition" and not interrupted:
                interrupted = True
                with (journal_path.parent / "verification-reaping.jsonl").open("a") as gate:
                    gate.write(json.dumps({"event": "stale_alarm", "boundary": "stdout.close", "pid": process.pid, "pgid": group, "time": time.monotonic(), "cleanup_end": sys._getframe(1).f_locals["deadlines"].cleanup_deadline}) + chr(10))
                os.kill(os.getpid(), signal.SIGALRM)
            return original_close()
        process.stdout.close = interrupted_close
        def delayed_wait(timeout=None):
            nonlocal interrupted
            caller = sys._getframe(1)
            if caller.f_code.co_name != "command":
                return original_wait(timeout=timeout)
            started = time.monotonic()
            with (journal_path.parent / "verification-reaping.jsonl").open("a") as gate:
                gate.write(json.dumps({"event": "wait_started", "pid": process.pid, "pgid": group, "time": started, "timeout": timeout, "total_deadline": caller.f_locals["deadlines"].total, "cleanup_end": caller.f_locals.get("end")}) + "\\n")
            if scenario in {"verification_reap_interrupted", "verification_reap_transition", "verification_reap_termination"}:
                if not interrupted:
                    interrupted = True
                    with (journal_path.parent / "verification-reaping.jsonl").open("a") as gate:
                        gate.write(json.dumps({"event": "stale_alarm", "pid": process.pid, "time": time.monotonic()}) + chr(10))
                    os.kill(os.getpid(), signal.SIGALRM)
            else:
                time.sleep(0.4)
            result = original_wait(timeout=timeout)
            with (journal_path.parent / "verification-reaping.jsonl").open("a") as gate:
                gate.write(json.dumps({"event": "wait_confirmed", "pid": process.pid, "time": time.monotonic(), "exit": result}) + "\\n")
            return result
        process.wait = delayed_wait
    if scenario == "launch_uncertain" or (scenario == "verification_launch_uncertain" and args[0][0] == g["GH"]) or (scenario == "fresh_launch_uncertain" and Path(args[0][0]).name == "bridge" and sum(Path(p.args[0]).name == "bridge" for p in owned) == 2):
        time.sleep(0.2)
        raise TimeoutError("synthetic interrupted process operation")
    return process
g["subprocess"].Popen = tracked_spawn
fault_fired = False
journal_path = Path(sys.argv[sys.argv.index("--journal") + 1]) if "--journal" in sys.argv else None
publication_line = next(i for i, line in enumerate(Path(sys.argv[1]).read_text().splitlines(), 1) if 'report["result"] = {"branch": branch' in line)
def fault_at_boundary(frame, event, arg):
    global fault_fired
    if frame.f_code.co_filename == sys.argv[1] and not fault_fired:
        if scenario == "discard_after_send" and event == "call" and frame.f_code.co_name == "sync_directory" and frame.f_back.f_code.co_name == "receipt" and frame.f_back.f_locals.get("method") == "Send":
            fault_fired = True
            gate = {"boundary": "Send receipt before directory sync", "signal_blocked": signal.SIGUSR2 in signal.pthread_sigmask(signal.SIG_BLOCK, set())}
            until = time.monotonic() + 2.5
            while time.monotonic() < until:
                rows = [json.loads(line) for line in (journal_path / "events.jsonl").read_text().splitlines()]
                if any(row["event"] == "closure" for row in rows):
                    gate["outcome"] = "closure durable"
                    break
                try:
                    audit = [json.loads(line) for line in (journal_path.parent / "audit.jsonl").read_text().splitlines()]
                except ValueError:
                    audit = []
                if any(row.get("journal_gate_timeout") for row in audit):
                    gate["outcome"] = "peer closure gate timed out"
                    break
                time.sleep(0.002)
            else:
                gate["outcome"] = "control gate unavailable"
            (journal_path.parent / "fault-gate.json").write_text(json.dumps(gate))
        if scenario == "capacity_at_cleanup" and event == "call" and frame.f_code.co_name == "cleanup":
            fault_fired = True
            (journal_path / "capacity").write_bytes(b"x" * (256 * 1024))
            (journal_path.parent / "fault-gate.json").write_text(json.dumps({"boundary": "cleanup", "capacity_written": True}))
        if scenario == "discard_at_verification" and event == "line" and frame.f_lineno == publication_line:
            fault_fired = True
            blocked = signal.SIGUSR2 in signal.pthread_sigmask(signal.SIG_BLOCK, set())
            (journal_path.parent / "fault-gate.json").write_text(json.dumps({"boundary": "verification", "signal_requested": True, "signal_blocked": blocked}))
            os.kill(os.getpid(), signal.SIGUSR2)
        if scenario == "alarm_after_coding_pass" and event == "return" and frame.f_code.co_name == "coding_run" and frame.f_locals["report"]["coding"] == "PASS":
            fault_fired = True
            (journal_path.parent / "fault-gate.json").write_text(json.dumps({"boundary": "coding_run return", "coding": frame.f_locals["report"]["coding"], "signal": "SIGALRM", "total_budget_remaining": frame.f_locals["deadlines"].total - time.monotonic()}))
            os.kill(os.getpid(), signal.SIGALRM)
    return fault_at_boundary
if scenario in {"discard_after_send", "capacity_at_cleanup", "discard_at_verification", "alarm_after_coding_pass"}:
    sys.settrace(fault_at_boundary)
if scenario in {"verification_reap_delayed", "verification_reap_exhausted", "verification_reap_interrupted", "verification_reap_transition", "verification_reap_termination"}:
    original_cleanup = g["cleanup"]
    def observed_cleanup(*args, **kwargs):
        deadlines = args[5]
        entered = time.monotonic()
        end = deadlines.cleanup_deadline
        reserve = min(g["SHUTDOWN_RESERVE"], max(0, end - entered))
        try:
            return original_cleanup(*args, **kwargs)
        finally:
            with (journal_path.parent / "verification-reaping.jsonl").open("a") as gate:
                gate.write(json.dumps({"event": "cleanup_bound", "time": entered, "end": end, "reserve": reserve, "returned_end": deadlines.cleanup_deadline}) + chr(10))
    g["cleanup"] = observed_cleanup
try:
    result = module["main"](sys.argv[4:])
finally:
    sys.settrace(None)
    for process in owned:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=1)
sys.exit(result)
""" % (*budgets, git_override, scenario)
        env = dict(os.environ, EXPLICIT_FIXTURE_KEY="API-SECRET-SENTINEL-363", CURSOR_API_KEY="AMBIENT-MUST-NOT-WIN")
        if env_change:
            env_change(env)
        start = time.monotonic()
        run = subprocess.run([sys.executable, "-I", "-c", runner, str(CLIENT), host + "-" + arch,
                              digest, "--manifest", str(manifest_path), "--archive", str(archive),
                              *(["--journal", str(journal), "--github-config", str(gh_config), "--github-login", "fixture"] if coding else ["--preflight-only"])],
                             env=env, text=True, capture_output=True, timeout=budgets[1] + 2)
        elapsed = time.monotonic() - start
        events = [json.loads(line) for line in audit.read_text().splitlines()] if audit.exists() else []
        for event in events:
            if "root" in event:
                self.addCleanup(shutil.rmtree, event["root"], True)
        self.assertLess(elapsed, budgets[1] + 1.5)
        self.assertEqual(run.stderr, "")
        self.assertNotIn("API-SECRET-SENTINEL-363", run.stdout)
        self.assertNotIn("BRIDGE-SECRET-SENTINEL-363", run.stdout)
        self.assertNotIn("GITHUB-SECRET-SENTINEL-364", run.stdout)
        report = json.loads(run.stdout)
        if not coding:
            self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
        methods = [e["method"] for e in events if "method" in e]
        if not coding:
            self.assertFalse(set(methods) & {"CreateAgent", "Send"})
        if coding:
            self.assertEqual(report["attempts"], {name: methods.count(name) for name in ("CreateAgent", "Send")})
            self.assertLessEqual(methods.count("CreateAgent"), 1)
            self.assertLessEqual(methods.count("Send"), 3)
            self.assertTrue(all(e["request_shape"] for e in events if e.get("method") in {"CreateAgent", "GetAgent", "Send"}))
        self.assertEqual(unrelated.read_text(), "UNRELATED-SECRET")
        if journal.exists():
            for path in journal.iterdir():
                if path.is_file():
                    saved = path.read_bytes()
                    self.assertNotIn(b"API-SECRET-SENTINEL-363", saved)
                    self.assertNotIn(b"BRIDGE-SECRET-SENTINEL-363", saved)
                    self.assertNotIn(b"GITHUB-SECRET-SENTINEL-364", saved)
        # Only the test owns this private root; preserve the report before fixture cleanup.
        if report["resources"].get("private_root"):
            self.addCleanup(shutil.rmtree, report["resources"]["private_root"], True)
        evidence_root = os.environ.get("OVRCR_CURSOR_OFFLINE_EVIDENCE")
        if evidence_root:
            target = Path(tempfile.mkdtemp(prefix=scenario + "-", dir=evidence_root))
            items = {"report.json": run.stdout.encode(), "wire.json": json.dumps(events, sort_keys=True).encode()}
            if journal.exists():
                items.update({"journal-" + path.name: path.read_bytes() for path in journal.iterdir() if path.is_file()})
            for name in ("verification-stall.json", "fault-gate.json", "github-audit.jsonl", "git-auth-audit.jsonl", "process-latency.jsonl", "verification-reaping.jsonl"):
                if (root / name).exists():
                    items[name] = (root / name).read_bytes()
            for name, data in items.items():
                fd = os.open(target / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                with os.fdopen(fd, "wb") as output:
                    output.write(data)
        return run, report, events

    def test_third_send_cancels_exact_active_run_then_reads_completed_run_on_fresh_bridge(self):
        run, report, events = self.run_fixture("third_delayed_cancel", coding=True)
        self.assertEqual(run.returncode, 0, report)
        self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 3})
        self.assertEqual(report["cancellation"]["verdict"], "PASS", report)
        self.assertEqual(report["cancellation"]["terminal"], "RUN_LIFECYCLE_STATUS_CANCELLED")
        self.assertEqual(report["fresh_bridge"]["verdict"], "PASS", report)
        self.assertEqual(report["fresh_bridge"]["runId"], "run-fixture")
        self.assertEqual([row["terminal"] for row in report["runs"]], ["RUN_LIFECYCLE_STATUS_FINISHED", "RUN_LIFECYCLE_STATUS_FINISHED", "RUN_LIFECYCLE_STATUS_CANCELLED"])
        methods = [row["method"] for row in events if "method" in row]
        self.assertEqual(methods.count("CreateAgent"), 1)
        self.assertEqual(methods.count("Send"), 3)
        self.assertEqual(methods.count("CancelRun"), 1)
        self.assertNotIn("ResumeAgent", methods)
        self.assertNotIn("ListRuns", methods)
        self.assertTrue(all(row["request_shape"] for row in events if row.get("method") in {"GetRun", "Send"}))
        self.assertTrue(next(row["cancel_gate"] for row in events if row.get("method") == "CancelRun"))
        self.assertTrue(next(row["exact_cancel"] for row in events if row.get("method") == "CancelRun"))
        self.assertEqual(methods.count("ObserveRun"), 3)
        self.assertEqual(len([row for row in events if "pid" in row and "token_file" in row]), 2)
        self.assertEqual(report["resources"]["previous_bridge"]["process_exit"], "CONFIRMED")
        self.assertEqual(report["resources"]["previous_bridge"]["token_cleanup"], "REMOVED")
        self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
        self.assertEqual(report["resources"]["token_cleanup"], "REMOVED")
        rows = [json.loads(line) for line in (self.case_root / "journal" / "events.jsonl").read_text().splitlines()]
        self.assertEqual(len({row["operation_key"] for row in rows if row["event"] == "claim"}), 4)
        self.assertEqual([row["kind"] for row in rows if row["event"] == "closure"], ["cancel"])
        self.assertTrue(any(row["event"] == "cancellation_result" and row["late"] for row in rows))
        self.assertFalse(list((self.case_root / "journal").glob("*.request.json")))

    def test_third_cancellation_ack_race_unknown_ids_and_late_receipts_never_add_work(self):
        for scenario in ("third_ack_only", "third_completion_race", "third_no_id", "third_wrong_agent", "third_reused_id", "third_no_activity", "third_snapshot_wrong_id", "third_observer_wrong_id", "third_observer_bad_status", "third_observer_error", "third_late_discard"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                methods = [row["method"] for row in events if "method" in row]
                self.assertEqual(methods.count("Send"), 3)
                self.assertEqual(methods.count("CreateAgent"), 1)
                self.assertNotIn("ListRuns", methods)
                self.assertNotIn("ResumeAgent", methods)
                self.assertNotEqual(report["cancellation"]["verdict"], "PASS")
                self.assertEqual(report["follow_up"]["result"], "FINISHED")
                if scenario == "third_completion_race":
                    self.assertEqual(run.returncode, 0, report)
                    self.assertEqual(report["cancellation"]["terminal"], "RUN_LIFECYCLE_STATUS_FINISHED")
                    self.assertIn("inconclusive", report["cancellation"]["reason"])
                    self.assertEqual(report["fresh_bridge"]["verdict"], "PASS")
                else:
                    self.assertEqual(run.returncode, 1, report)
                    self.assertEqual(len([row for row in events if "token_file" in row]), 1)
                if scenario in {"third_no_id", "third_wrong_agent", "third_reused_id", "third_late_discard"}:
                    self.assertNotIn("CancelRun", methods)
                if scenario in {"third_no_id", "third_wrong_agent", "third_reused_id"}:
                    self.assertTrue((self.case_root / "journal" / "Send.request.json").exists())
                    self.assertIsNone(report["runs"][-1]["runId"])
                    self.assertEqual(report["runs"][-1]["terminal"], "UNRESOLVED")
                if scenario == "third_ack_only":
                    self.assertEqual(report["cancellation"]["request"], "ACKNOWLEDGED; not terminal proof")
                    self.assertEqual(report["runs"][-1]["terminal"], "UNRESOLVED")
                    self.assertEqual(methods.count("CancelRun"), 1)
                if scenario == "third_late_discard":
                    rows = [json.loads(line) for line in (self.case_root / "journal" / "events.jsonl").read_text().splitlines()]
                    self.assertTrue(any(row["event"] == "receipt" and row.get("method") == "Send" and row.get("late") and row["receipt"].get("runId") == "run-cancel" for row in rows))
                    self.assertFalse((self.case_root / "journal" / "Send.request.json").exists())

    def test_third_stalled_control_and_observation_keep_original_shutdown_reserve(self):
        for scenario, phase in (("third_cancel_stall", "third_cancel"), ("third_observer_stall", "third_observer")):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True, budgets=(6, 8, 1.5, 0.75))
                self.assertEqual(run.returncode, 1, report)
                self.assertTrue(any(row.get("stall") == phase for row in events))
                self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 3})
                self.assertEqual(sum(row.get("method") == "CancelRun" for row in events), 1)
                self.assertNotEqual(report["cancellation"]["verdict"], "PASS")
                self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
                self.assertEqual(report["resources"]["token_cleanup"], "REMOVED")
                self.assertLess(report["elapsed_seconds"], 8)
                self.assertEqual(len([row for row in events if "token_file" in row]), 1)

    def test_fresh_read_only_probe_refuses_resume_updates_and_retains_original_clock(self):
        for scenario in ("fresh_unavailable", "fresh_requires_resume", "fresh_wrong_id", "fresh_stall"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 0, report)
                self.assertEqual(report["fresh_bridge"]["verdict"], "BLOCKED")
                self.assertEqual(report["resume_options"]["verdict"], "NOT RUN")
                self.assertIn("preservation unverified", report["resume_options"]["reason"])
                methods = [row["method"] for row in events if "method" in row]
                self.assertNotIn("ResumeAgent", methods)
                self.assertEqual(methods.count("CreateAgent"), 1)
                self.assertEqual(methods.count("Send"), 3)
                fresh = [row for row in events if row.get("bridge_index") == 2]
                self.assertEqual([row["method"] for row in fresh], ["GetRun", "Shutdown"] if scenario != "fresh_stall" else ["GetRun"])
                self.assertTrue(all(row["request_shape"] for row in fresh))
                if scenario == "fresh_stall":
                    self.assertTrue(any(row.get("stall") == "fresh_get_run" for row in events))
                clock = report["deadlines"]
                self.assertEqual(clock["work_deadline"], clock["started_monotonic"] + CODING_BUDGETS[0])
                self.assertEqual(clock["total_deadline"], clock["started_monotonic"] + CODING_BUDGETS[1])
                self.assertLess(report["elapsed_seconds"], CODING_BUDGETS[0])
                self.assertLess(clock["cleanup_deadline"], clock["total_deadline"])
                self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
                self.assertEqual(report["resources"]["token_cleanup"], "REMOVED")

    def test_restart_waits_for_owned_exit_and_token_disposition_and_unknown_launch_stays_unknown(self):
        for scenario in ("first_token_leftover", "second_token_leftover", "fresh_launch_uncertain"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 1, report)
                started = [row for row in events if "token_file" in row]
                self.assertEqual(len(started), 1 if scenario == "first_token_leftover" else 2)
                if scenario == "first_token_leftover":
                    self.assertEqual(report["fresh_bridge"]["verdict"], "BLOCKED")
                    self.assertEqual(report["resources"]["previous_bridge"]["token_cleanup"], "UNRESOLVED")
                if scenario == "fresh_launch_uncertain":
                    self.assertEqual(report["resources"]["process_exit"], "UNRESOLVED")
                    self.assertEqual(report["resources"]["token_cleanup"], "UNRESOLVED")
                else:
                    self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
                    self.assertEqual(report["resources"]["token_cleanup"], "UNRESOLVED")
                self.assertIn("RETAINED", report["resources"]["private_root_cleanup"])

    def test_usage_cost_keeps_float_cents_and_missing_cost_unavailable(self):
        for scenario in ("success", "usage_cost"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 0, report)
                self.assertEqual(sum(row.get("method") == "GetUsage" for row in events), 1)
                self.assertEqual(report["usage"]["observed"]["usage"]["totalTokens"], "24")
                if scenario == "usage_cost":
                    self.assertEqual(report["usage"]["observed"]["cost"], {"rawCostCents": 2.0, "chargedCents": 1.5})
                else:
                    self.assertNotIn("cost", report["usage"]["observed"])
                    self.assertIn("UNAVAILABLE", report["usage"]["cost"])
                self.assertEqual(report["native_gates"]["active_process_death"], "NOT RUN")
                self.assertIn("BLOCKED", report["native_gates"]["terminal_response"])
                self.assertIn("BLOCKED", report["native_gates"]["uncertain_retry"])

    def test_recovery_and_follow_up_use_exact_run_and_private_original_keys(self):
        run, report, events = self.run_fixture(coding=True)
        self.assertEqual(run.returncode, 0, report)
        self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 3})
        for name in ("active_observation", "offset_resume", "follow_up"):
            self.assertEqual(report["recovery_criteria"][name]["verdict"], "PASS", report)
        self.assertEqual(report["recovery_criteria"]["completed_result"]["verdict"], "NOT RUN")
        self.assertEqual(report["follow_up"]["runId"], "run-follow-up")
        self.assertEqual(report["follow_up"]["agentId"], report["run"]["agentId"])
        methods = [e["method"] for e in events if "method" in e]
        self.assertEqual(methods.count("ObserveRun"), 2)
        self.assertEqual(methods.count("CancelRun"), 1)
        self.assertTrue(any(e.get("original_send_closed") for e in events))
        self.assertTrue(any(e.get("original_observer_closed") for e in events))
        self.assertEqual([e["observation_request"] for e in events if e.get("method") == "ObserveRun"], [{"runId": "run-fixture"}, {"runId": "run-fixture", "afterOffset": "observe-3"}])
        self.assertTrue(all(e["request_shape"] for e in events if e.get("method") in {"ObserveRun", "GetRun", "Send"}))
        self.assertTrue(all(e["write_ahead"] for e in events if e.get("method") in {"CreateAgent", "Send"}))
        rows = [json.loads(line) for line in (self.case_root / "journal" / "events.jsonl").read_text().splitlines()]
        claims = [row for row in rows if row["event"] == "claim"]
        self.assertEqual(len({row["operation_key"] for row in claims}), 4)
        offsets = [row for row in rows if row["event"] == "observe_offset"]
        self.assertTrue(offsets)
        self.assertTrue(all(row["origin"] == "ObserveRun" and row["runId"] == "run-fixture" for row in offsets))
        self.assertTrue(any(row["event"] == "observe_event" and row["replay"] for row in rows))
        self.assertEqual(report["follow_up"]["tests"], "NO NEW FILES; original verified commit unchanged")
        pr_receipts = [row for row in rows if row["event"] == "github_pr_snapshot"]
        self.assertEqual(len(pr_receipts), 2)
        self.assertEqual(pr_receipts[0], pr_receipts[1])
        gh_rows = [json.loads(line) for line in (self.case_root / "github-audit.jsonl").read_text().splitlines()]
        pr_reads = [row for row in gh_rows if row["kind"] == "pulls"]
        self.assertEqual(len(pr_reads), 2)
        self.assertTrue(all(row["config_matches"] and row["ambient_excluded"] for row in gh_rows))
        for row in pr_reads:
            self.assertEqual(row["pr_argv"][:8], ["pr", "list", "--repo", "https://github.com/fixture/disposable", "--state", "all", "--limit", "100"])


    def test_completion_race_and_missing_cursor_do_not_pass_unexercised_criteria(self):
        for scenario in ("complete_before_drop", "complete_during_attach", "no_observe_offset"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 0, report)
                criteria = report["recovery_criteria"]
                self.assertEqual(criteria["offset_resume"]["verdict"], "NOT RUN")
                self.assertEqual(criteria["follow_up"]["verdict"], "PASS")
                self.assertEqual(criteria["completed_result"]["verdict"], "NOT RUN" if scenario == "no_observe_offset" else "PASS")
                self.assertEqual(criteria["active_observation"]["verdict"], {"complete_before_drop": "NOT RUN", "complete_during_attach": "BLOCKED", "no_observe_offset": "PASS"}[scenario])
                self.assertEqual(sum(e.get("method") == "ObserveRun" for e in events), 1)
                self.assertTrue(any(e.get("original_send_closed") for e in events))

    def test_observation_errors_close_admission_without_replacement_work(self):
        for scenario, reason in (("wrong_offset", "exclusive ObserveRun offset"),
                                 ("observer_wrong_agent", "observation stream agent/run"),
                                 ("observer_wrong_run", "observation stream agent/run"),
                                 ("snapshot_wrong_id", "observation snapshot agent/run"),
                                 ("observer_error", "Connect end-stream error"),
                                 ("invalid_offset", "exclusive ObserveRun offset")):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 1})
                self.assertIn(reason, report["stages"][-1]["reason"], report)
                self.assertNotEqual(report["recovery_criteria"]["active_observation"]["verdict"], "PASS")
                self.assertNotEqual(report["recovery_criteria"]["offset_resume"]["verdict"], "PASS")
                methods = [e["method"] for e in events if "method" in e]
                self.assertEqual(methods.count("CreateAgent"), 1)
                self.assertEqual(methods.count("Send"), 1)
                self.assertNotIn("ListRuns", methods)
                self.assertNotIn("ResumeAgent", methods)
                self.assertEqual(report["resources"]["admission"], "CLOSED; durable closure")

    def test_follow_up_rejects_wrong_recall_identity_result_and_remote_effects(self):
        for scenario, reason in (("follow_up_wrong_token", "recalled token/fix"),
                                 ("follow_up_failed", "run result/model"),
                                 ("follow_up_same_run", "reused the original run"),
                                 ("follow_up_wrong_agent", "early original-stream"),
                                 ("follow_up_branch", "repository/branch/PR"),
                                 ("follow_up_pr", "repository/branch/PR"),
                                 ("follow_up_extra_file", "branch/file state"),
                                 ("follow_up_extra_ref", "branch/file state")):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["coding"], "PASS", report)
                self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 2})
                self.assertNotEqual(report["recovery_criteria"]["follow_up"]["verdict"], "PASS")
                self.assertIn(reason, report["stages"][-1]["reason"], report)
                methods = [e["method"] for e in events if "method" in e]
                self.assertEqual(methods.count("Send"), 2)
                self.assertEqual(methods.count("CreateAgent"), 1)
                self.assertNotIn("ListRuns", methods)
                self.assertNotIn("ResumeAgent", methods)
                self.assertEqual(report["resources"]["admission"], "CLOSED; durable closure")

    def test_follow_up_uncertain_receipt_and_exact_second_run_cleanup(self):
        for scenario in ("follow_up_lost", "follow_up_partial"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["coding"], "PASS")
                self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 2})
                methods = [e["method"] for e in events if "method" in e]
                self.assertEqual(methods.count("Send"), 2)
                journal = self.case_root / "journal"
                if scenario == "follow_up_lost":
                    self.assertTrue((journal / "Send.request.json").exists())
                    self.assertNotIn("CancelRun", methods)
                    self.assertIn("without exact durable identity", report["resources"]["remote_outcome"])
                else:
                    self.assertEqual(methods.count("CancelRun"), 1)
                    self.assertTrue(next(e["exact_cancel"] for e in events if e.get("method") == "CancelRun"))
                    self.assertEqual(report["resources"]["remote_outcome"], "RUN_LIFECYCLE_STATUS_CANCELLED")
                self.assertNotEqual(report["recovery_criteria"]["follow_up"]["verdict"], "PASS")

    def test_failed_terminal_snapshots_forbid_recovery_success_and_follow_up(self):
        for timing in ("before_drop", "during_attach"):
            for terminal in ("ERROR", "CANCELLED", "EXPIRED"):
                scenario = "snapshot_" + terminal.lower() + "_" + timing
                with self.subTest(scenario=scenario):
                    run, report, events = self.run_fixture(scenario, coding=True)
                    self.assertEqual(run.returncode, 1, report)
                    self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 1})
                    self.assertNotEqual(report["coding"], "PASS")
                    self.assertNotEqual(report["recovery_criteria"]["completed_result"]["verdict"], "PASS")
                    self.assertNotEqual(report["recovery_criteria"]["follow_up"]["verdict"], "PASS")
                    self.assertIn("unsuccessful terminal snapshot", report["stages"][-1]["reason"])
                    self.assertEqual(report["resources"]["remote_outcome"], "RUN_LIFECYCLE_STATUS_" + terminal)
                    methods = [e["method"] for e in events if "method" in e]
                    self.assertEqual(methods.count("ObserveRun"), 0 if timing == "before_drop" else 1)
                    self.assertNotIn("CancelRun", methods)
                    rows = [json.loads(line) for line in (self.case_root / "journal" / "events.jsonl").read_text().splitlines()]
                    self.assertTrue(any(row["event"] == "observation_snapshot" and row["receipt"]["status"] == "RUN_LIFECYCLE_STATUS_" + terminal for row in rows))

    def test_independent_follow_up_ref_and_pr_effects_cannot_hide_in_result_metadata(self):
        for scenario in ("follow_up_extra_tag", "follow_up_external_pr", "follow_up_pr_edited"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 2})
                self.assertEqual(report["coding"], "PASS")
                self.assertNotEqual(report["recovery_criteria"]["follow_up"]["verdict"], "PASS")
                self.assertIn("independently verified", report["stages"][-1]["reason"])
                if scenario == "follow_up_extra_tag":
                    tag = subprocess.check_output(["/usr/bin/git", "show-ref", "--verify", "refs/tags/cursor-extra"], cwd=self.case_root / "remote", text=True)
                    self.assertIn("refs/tags/cursor-extra", tag)
                else:
                    gh_rows = [json.loads(line) for line in (self.case_root / "github-audit.jsonl").read_text().splitlines()]
                    self.assertEqual(sum(row["kind"] == "pulls" for row in gh_rows), 2)
                    self.assertTrue(all(row["ambient_excluded"] and row["config_matches"] for row in gh_rows))
                    rows = [json.loads(line) for line in (self.case_root / "journal" / "events.jsonl").read_text().splitlines()]
                    results = [row["receipt"]["result"]["git"] for row in rows if row["event"] == "result"]
                    self.assertEqual(results[0], results[1])

    def test_pr_verification_fails_closed_when_unavailable_malformed_incomplete_or_stalled(self):
        for phase in ("before", "after"):
            for fault in ("unavailable", "malformed", "bad_record", "incomplete"):
                scenario = "pr_" + fault + "_" + phase
                with self.subTest(scenario=scenario):
                    run, report, events = self.run_fixture(scenario, coding=True)
                    self.assertEqual(run.returncode, 1, report)
                    self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 1 if phase == "before" else 2})
                    self.assertNotEqual(report["recovery_criteria"]["follow_up"]["verdict"], "PASS")
                    self.assertEqual(report["stages"][-1]["stage"], "follow_up")
        run, report, events = self.run_fixture("pr_account_after", coding=True)
        self.assertEqual(run.returncode, 1, report)
        self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 2})
        self.assertIn("selected GitHub login", report["stages"][-1]["reason"])
        run, report, events = self.run_fixture("pr_stall_after", coding=True, budgets=(6, 8, 1.5, 0.75))
        self.assertEqual(run.returncode, 1, report)
        self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 2})
        self.assertEqual(report["stages"][-1]["stage"], "follow_up")
        self.assertIn("deadline", report["stages"][-1]["reason"])
        phase = json.loads((self.case_root / "verification-stall.json").read_text())
        self.assertEqual(phase["phase"], "pr_inventory_stall")
        self.assertIn(phase["pid"], [row["pid"] for row in report["resources"]["verification_processes"]])

    def test_coding_verifies_patch_and_exact_write_ahead_order(self):
        run, report, events = self.run_fixture(coding=True)
        self.assertEqual(run.returncode, 0, report)
        self.assertEqual(report["coding"], "PASS")
        self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 3})
        methods = [event["method"] for event in events if "method" in event]
        self.assertEqual(methods, ["Ping", "GetVersion", "Ping", "Me", "ListModels", "ListRepositories", "CreateAgent", "GetAgent", "Send", "GetRun", "ObserveRun", "GetRun", "ObserveRun", "Send", "Send", "CancelRun", "GetRun", "GetUsage", "Shutdown", "GetRun", "Shutdown"])
        self.assertTrue(all(event["write_ahead"] for event in events if event.get("method") in {"CreateAgent", "Send"}))
        self.assertEqual(report["result"]["tests"], "PASS; 3 tests")
        self.assertEqual(report["control_recovery"], "PASS")
        journal = self.case_root / "journal"
        rows = [json.loads(line) for line in (journal / "events.jsonl").read_text().splitlines()]
        claims = [row for row in rows if row["event"] == "claim"]
        self.assertEqual(len({row["operation_key"] for row in claims}), 4)
        self.assertFalse(list(journal.glob("*.request.json")))
        self.assertEqual([row["method"] for row in rows if row["event"] == "receipt"], ["CreateAgent", "Send", "Send", "Send"])

    def test_slow_external_processes_progress_under_normal_budget_and_obey_short_deadline(self):
        for name, budgets in [("short", (2, 2.6, 0.4, 0.15)), ("normal", None)]:
            with self.subTest(budget=name):
                kwargs = {"budgets": budgets} if budgets is not None else {}
                run, report, events = self.run_fixture(coding=True, process_delay=0.25, **kwargs)
                latency = [json.loads(line) for line in (self.case_root / "process-latency.jsonl").read_text().splitlines()]
                progressed = [row for row in latency if row["phase"] == "progressed"]
                process_ids = {row["pid"] for row in report["resources"]["verification_processes"]}
                self.assertTrue(progressed)
                self.assertTrue(all(row["pid"] in process_ids for row in latency))
                self.assertTrue(all(row["delay_seconds"] == 0.25 for row in latency))
                waits = {row["pid"]: row["time"] for row in latency if row["phase"] == "waiting"}
                self.assertTrue(all(row["time"] - waits[row["pid"]] >= 0.25 for row in progressed))
                self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
                self.assertTrue(all(row["exit"] != "UNRESOLVED" for row in report["resources"]["verification_processes"]))
                if name == "short":
                    self.assertEqual(run.returncode, 1, report)
                    self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
                    self.assertEqual(report["stages"][-1]["stage"], "source_fixture")
                    self.assertIn("deadline", report["stages"][-1]["reason"])
                    self.assertLess(report["elapsed_seconds"], 2.7)
                    self.assertFalse(set(e.get("method") for e in events) & {"CreateAgent", "Send"})
                else:
                    self.assertEqual(run.returncode, 0, report)
                    self.assertEqual(report["coding"], "PASS")
                    self.assertEqual(report["deadlines"]["work_seconds"], CODING_BUDGETS[0])
                    self.assertEqual(report["deadlines"]["total_seconds"], CODING_BUDGETS[1])
                    self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 3})
                    self.assertEqual(report["source"]["baseline_tests"], "FAIL; 3 tests, 2 failures")
                    self.assertEqual(report["result"]["tests"], "PASS; 3 tests")
                    self.assertEqual(sum(row["command"] == "fetch" for row in progressed), 3)
                    self.assertEqual(sum(row["command"] == "ls-remote" for row in progressed), 2)
                    for criterion in ("active_observation", "offset_resume", "follow_up"):
                        self.assertEqual(report["recovery_criteria"][criterion]["verdict"], "PASS")
                    self.assertGreater(sum(row["delay_seconds"] for row in progressed), 3)
                    self.assertEqual(report["resources"]["token_cleanup"], "REMOVED")

    def test_lost_receipts_close_admission_and_retain_exact_original_request(self):
        for scenario, expected, retained in [("creation_lost", {"CreateAgent": 1, "Send": 0}, "CreateAgent"),
                                               ("send_lost", {"CreateAgent": 1, "Send": 1}, "Send")]:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["attempts"], expected)
                self.assertEqual(report["resources"]["remote_outcome"], "UNRESOLVED; an effect may have escaped without exact durable identity")
                journal = self.case_root / "journal"
                request = json.loads((journal / (retained + ".request.json")).read_text())
                self.assertEqual(request["method"], retained)
                self.assertNotIn("apiKey", json.dumps(request))
                rows = [json.loads(line) for line in (journal / "events.jsonl").read_text().splitlines()]
                self.assertEqual(rows[-1]["event"], "closure")
                self.assertEqual(rows[-1]["kind"], "cleanup")
                methods = [e["method"] for e in events if "method" in e]
                self.assertEqual(methods.count("CreateAgent"), 1)
                self.assertEqual(methods.count("Send"), expected["Send"])
                self.assertFalse(set(methods) & {"ListRuns", "CancelRun", "ObserveRun", "ResumeAgent"})

    def test_coding_rejects_unverified_source_identity_results_and_framing(self):
        cases = ["wrong_source", "assistant_source_only", "late_source_probe", "wrong_model", "wrong_diff", "failed_tests", "failed_run",
                 "creation_no_id", "creation_wrong_model", "get_agent_wrong_id", "get_agent_local", "get_agent_wrong_repo",
                 "early_no_id", "early_wrong_agent", "early_wrong_model", "terminal_wrong_run", "terminal_wrong_model",
                 "missing_result", "missing_done", "wrong_done", "end_error", "missing_end", "bad_flags", "bad_json", "duplicate_result", "after_done", "result_bad_git", "result_bad_branch", "sdk_bad_payload", "probe_bool_exit", "wrong_ancestry"]
        reasons = {
            "wrong_source": "initial source probe differs", "assistant_source_only": "coding lacks", "late_source_probe": "initial source probe is missing",
            "wrong_model": "run result/model", "wrong_diff": "result changes files", "failed_tests": "independent Git/fixture command failed", "failed_run": "run result/model",
            "creation_no_id": "creation identity/model", "creation_wrong_model": "creation identity/model",
            "get_agent_wrong_id": "GetAgent cloud identity", "get_agent_local": "GetAgent cloud identity", "get_agent_wrong_repo": "GetAgent cloud identity",
            "early_no_id": "early original-stream", "early_wrong_agent": "early original-stream", "early_wrong_model": "early original-stream",
            "terminal_wrong_run": "terminal agent/run", "terminal_wrong_model": "run result/model", "missing_result": "done identity/order",
            "missing_done": "coding lacks", "wrong_done": "observation stream agent/run", "end_error": "Connect end-stream error",
            "missing_end": "Connect stream ended", "bad_flags": "invalid/oversized Connect frame", "bad_json": "invalid input",
            "duplicate_result": "duplicate terminal result", "after_done": "message after done", "result_bad_git": "result repository/branch",
            "result_bad_branch": "result repository/branch", "sdk_bad_payload": "observation stream agent/run", "probe_bool_exit": "initial source probe differs",
            "wrong_ancestry": "independent Git/fixture command failed",
        }
        for scenario in cases:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 1, report)
                self.assertNotEqual(report["coding"], "PASS")
                self.assertIn(reasons[scenario], report["stages"][-1]["reason"], report)
                methods = [e["method"] for e in events if "method" in e]
                self.assertEqual(methods.count("CreateAgent"), 1)
                self.assertLessEqual(methods.count("Send"), 1)
                self.assertFalse(set(methods) & {"ListRuns", "ResumeAgent"})
                self.assertTrue((self.case_root / "journal" / "events.jsonl").exists())

    def test_selected_github_helper_authenticates_private_fetch_and_refuses_bad_auth(self):
        for scenario in ("success", "github_wrong_login", "github_auth_bad", "github_helper_refused"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True,
                                                       env_change=lambda env: env.update(GH_TOKEN="AMBIENT_GH_MUST_NOT_WIN", GITHUB_TOKEN="AMBIENT_GITHUB_MUST_NOT_WIN"))
                self.assertEqual(run.returncode, 0 if scenario == "success" else 1, report)
                gh_rows = [json.loads(line) for line in (self.case_root / "github-audit.jsonl").read_text().splitlines()]
                self.assertTrue(all(row["ambient_excluded"] and row["config_matches"] for row in gh_rows))
                if scenario == "success":
                    self.assertEqual(sum(row["kind"] == "credential" for row in gh_rows), 5)
                    git_rows = [json.loads(line) for line in (self.case_root / "git-auth-audit.jsonl").read_text().splitlines()]
                    self.assertTrue(all(row["credential_supplied"] and row["other_remote_refused"] for row in git_rows))
                    self.assertEqual(report["github_auth"]["login"], "fixture")
                else:
                    self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
                    self.assertEqual(report["stages"][-1]["stage"], "source_fixture" if scenario == "github_helper_refused" else "github_auth")
                self.assertEqual((self.case_root / "github-config" / "owned-marker").read_text(), "preserve selected config")

    def test_json_property_names_cannot_leak_credentials_into_receipts_or_report(self):
        for scenario in ("secret_response_key", "secret_nested_key", "bridge_token_key"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 0})
                self.assertEqual(report["stages"][-1]["stage"], "CreateAgent")
                self.assertIn("secret found", report["stages"][-1]["reason"])
                self.assertTrue((self.case_root / "journal" / "CreateAgent.request.json").exists())

    def test_failed_required_closure_is_not_durable_or_an_overall_success(self):
        for scenario in ("discard_storage_failure", "capacity_at_cleanup"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True, budgets=(6, 8, 1.5, 0.75))
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["resources"]["journal_health"], "FAILED; no further dispatch")
                self.assertNotEqual(report["criteria"]["8"]["verdict"], "PASS")
                self.assertNotEqual(report["resources"].get("admission"), "CLOSED; durable closure")
                rows = [json.loads(line) for line in (self.case_root / "journal" / "events.jsonl").read_text().splitlines()]
                self.assertFalse(any(row["event"] == "closure" for row in rows))
                if scenario == "discard_storage_failure":
                    self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
                else:
                    self.assertEqual(report["criteria"]["6"]["verdict"], "PASS")
                    self.assertEqual(json.loads((self.case_root / "fault-gate.json").read_text())["boundary"], "cleanup")

    def test_final_verified_publication_and_discard_share_one_authority(self):
        run, report, events = self.run_fixture("discard_at_verification", coding=True, budgets=(6, 8, 1.5, 0.75))
        gate = json.loads((self.case_root / "fault-gate.json").read_text())
        self.assertTrue(gate["signal_requested"])
        rows = [json.loads(line) for line in (self.case_root / "journal" / "events.jsonl").read_text().splitlines()]
        closure = next(i for i, row in enumerate(rows) if row["event"] == "closure")
        self.assertEqual(rows[closure]["kind"], "discard")
        self.assertFalse(any(row["event"] == "verified" for row in rows[closure + 1:]), rows)
        if report["coding"] == "PASS":
            self.assertEqual(run.returncode, 1, report)
            self.assertEqual(report["attempts"]["Send"], 1)
            self.assertNotEqual(report["recovery_criteria"]["follow_up"]["verdict"], "PASS")
            self.assertTrue(gate["signal_blocked"])
            self.assertTrue(any(row["event"] == "verified" for row in rows[:closure]))
        else:
            self.assertEqual(run.returncode, 1, report)
        self.assertFalse(list((self.case_root / "journal").glob("*.request.json")))

    def test_caught_deadline_after_coding_pass_fails_the_overall_invocation(self):
        run, report, events = self.run_fixture("alarm_after_coding_pass", coding=True, budgets=(6, 8, 1.5, 0.75))
        gate = json.loads((self.case_root / "fault-gate.json").read_text())
        self.assertEqual(gate["boundary"], "coding_run return")
        self.assertEqual(gate["coding"], "PASS")
        self.assertEqual(gate["signal"], "SIGALRM")
        self.assertGreater(gate["total_budget_remaining"], 0)
        self.assertEqual(report["coding"], "PASS")
        self.assertEqual(report["criteria"]["6"]["verdict"], "PASS")
        self.assertEqual(report["result"]["tests"], "PASS; 3 tests")
        self.assertEqual(report["stages"][-1], {"stage": "result_tests", "verdict": "FAIL", "reason": "absolute deadline exceeded"})
        self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 1})
        methods = [e["method"] for e in events if "method" in e]
        self.assertEqual(methods.count("CreateAgent"), 1)
        self.assertEqual(methods.count("Send"), 1)
        self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
        self.assertEqual(report["resources"]["token_cleanup"], "REMOVED")
        self.assertEqual(report["resources"]["journal_health"], "DURABLE")
        self.assertEqual(report["resources"]["admission"], "CLOSED; durable closure")
        rows = [json.loads(line) for line in (self.case_root / "journal" / "events.jsonl").read_text().splitlines()]
        self.assertEqual(sum(row["event"] == "verified" for row in rows), 1)
        self.assertEqual(sum(row["event"] == "closure" for row in rows), 1)
        self.assertEqual(run.returncode, 1, report)
        self.assertEqual(report["outcome"], "FAIL")

    def test_independent_fixture_source_admission_precedes_any_creation(self):
        reasons = {"unpublished_source": "independent Git/fixture command failed",
                   "same_default_source": "default-tip marker is not the distinct",
                   "wrong_baseline": "selected fixture differs from reviewed"}
        for scenario in ("unpublished_source", "same_default_source", "wrong_baseline"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
                self.assertEqual(report["stages"][-1]["stage"], "source_fixture")
                self.assertIn(reasons[scenario], report["stages"][-1]["reason"])
                self.assertFalse(set(e.get("method") for e in events) & {"CreateAgent", "Send"})

    def test_private_owner_storage_and_capacity_fail_before_new_effect(self):
        for scenario, creates in [("competing_owner", 0), ("retained_journal", 0), ("request_conflict", 0), ("capacity_before_create", 0), ("receipt_capacity", 1)]:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["attempts"], {"CreateAgent": creates, "Send": 0})
                journal = self.case_root / "journal"
                self.assertTrue(journal.exists())
                if scenario == "retained_journal":
                    self.assertEqual((journal / "events.jsonl").read_text(), '{"event":"prior-unresolved-claim"}\n')
                if scenario == "receipt_capacity":
                    self.assertTrue((journal / "CreateAgent.request.json").exists())
                    self.assertEqual(report["resources"]["remote_outcome"], "UNRESOLVED; an effect may have escaped without exact durable identity")

    def test_closure_orders_before_claim_and_late_receipts_never_reopen_delivery(self):
        for scenario, creates, sends in [("close_before_create", 0, 0), ("cancel_after_create", 1, 0), ("discard_after_create", 1, 0), ("close_before_send", 1, 0), ("discard_after_send", 1, 1)]:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True)
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["attempts"], {"CreateAgent": creates, "Send": sends})
                methods = [e["method"] for e in events if "method" in e]
                self.assertNotIn("CancelRun", methods)
                rows = [json.loads(line) for line in (self.case_root / "journal" / "events.jsonl").read_text().splitlines()]
                closure = next(i for i, row in enumerate(rows) if row["event"] == "closure")
                self.assertFalse(any(row["event"] == "claim" for row in rows[closure + 1:]))
                self.assertFalse(any(row["event"] == "verified" for row in rows))
                self.assertFalse(list((self.case_root / "journal").glob("*.request.json")))
                if "after_create" in scenario:
                    self.assertTrue(any(row["event"] == "receipt" and row["late"] for row in rows[closure + 1:]))
                if scenario == "discard_after_send":
                    self.assertTrue(any(row["event"] == "result" and row["late"] for row in rows[closure + 1:]))
                    identity = next(row for row in rows[closure + 1:] if row["event"] == "receipt" and row.get("method") == "Send")
                    self.assertTrue(identity["late"])
                    self.assertEqual(identity["receipt"], {"agentId": "bc-fixture", "runId": "run-fixture", "model": {"id": "fixture-model"}})
                    self.assertTrue(next(e["write_ahead"] for e in events if e.get("method") == "Send"))
                    self.assertTrue(any(e.get("discard_gate") == {"send_received_before_closure": True, "closure_before_init": True} for e in events))
                    self.assertFalse(set(methods) & {"GetRun", "ObserveRun"})
                    gate = json.loads((self.case_root / "fault-gate.json").read_text())
                    self.assertEqual(gate, {"boundary": "Send receipt before directory sync", "signal_blocked": True, "outcome": "closure durable"})
                    self.assertFalse(any(e.get("journal_gate_timeout") for e in events))

    def test_only_exact_known_active_run_can_be_cancelled_and_ack_is_not_terminal(self):
        for scenario, outcome in [("cancel_known", "RUN_LIFECYCLE_STATUS_CANCELLED"), ("cancel_unconfirmed", "UNRESOLVED"), ("cancel_stall", "UNRESOLVED")]:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True, budgets=(*CODING_BUDGETS[:2], 0.4, 0.15))
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["resources"]["remote_outcome"], outcome)
                methods = [e["method"] for e in events if "method" in e]
                self.assertEqual(methods.count("CancelRun"), 1)
                self.assertEqual(methods.count("Send"), 1)
                self.assertTrue(all(e["exact_cancel"] for e in events if e.get("method") == "CancelRun"))
                self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")

    def test_interrupted_verification_launch_retains_unknown_process_resources(self):
        run, report, events = self.run_fixture("verification_launch_uncertain", coding=True, budgets=(2.2, 2.8, 0.4, 0.15))
        self.assertEqual(run.returncode, 1, report)
        self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
        self.assertEqual(report["resources"]["verification_processes"][0]["exit"], "UNRESOLVED")
        self.assertIn("verification process exit unconfirmed", report["resources"]["private_root_cleanup"])
        self.assertTrue(Path(report["resources"]["private_root"]).exists())

    def test_known_verification_child_reaping_shares_cleanup_budget_and_preserves_uncertainty(self):
        cases = [("verification_reap_delayed", (3, 5, 1.5, 0.5)),
                 ("verification_reap_exhausted", (3, 3.4, 1.5, 0.3))]
        for scenario, budgets in cases:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True, budgets=budgets)
                phase = json.loads((self.case_root / "verification-stall.json").read_text())
                self.assertEqual(phase["phase"], "git_fetch_stall")
                receipt = next(row for row in report["resources"]["verification_processes"] if row["pid"] == phase["pid"])
                gates = [json.loads(line) for line in (self.case_root / "verification-reaping.jsonl").read_text().splitlines()]
                waiting = next(row for row in gates if row["event"] == "wait_started")
                cleanup = next(row for row in gates if row["event"] == "cleanup_bound")
                self.assertEqual(waiting["pid"], phase["pid"])
                self.assertEqual(waiting["pgid"], phase["pid"])
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["outcome"], "FAIL")
                self.assertEqual(report["stages"][-1]["stage"], "source_fixture")
                self.assertIn("deadline", report["stages"][-1]["reason"])
                self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
                self.assertFalse(set(e.get("method") for e in events) & {"CreateAgent", "Send"})
                self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
                self.assertLess(report["elapsed_seconds"], budgets[1] + 0.1)
                if scenario == "verification_reap_delayed":
                    self.assertEqual(receipt["exit"], -9, report)
                    confirmed = next(row for row in gates if row["event"] == "wait_confirmed")
                    self.assertEqual(confirmed["pid"], phase["pid"])
                    self.assertEqual(confirmed["exit"], receipt["exit"])
                    self.assertGreaterEqual(confirmed["time"] - waiting["time"], 0.4)
                    self.assertEqual(report["resources"]["verification_cleanup"], "REMOVED")
                    self.assertEqual(report["resources"]["private_root_cleanup"], "REMOVED")
                else:
                    self.assertEqual(receipt["exit"], "UNRESOLVED")
                    self.assertFalse(any(row["event"] == "wait_confirmed" for row in gates))
                    self.assertIn("verification process exit unconfirmed", report["resources"]["private_root_cleanup"])
                    self.assertTrue(Path(report["resources"]["private_root"]).exists())
                expected_end = min(waiting["time"] + budgets[2], waiting["total_deadline"])
                self.assertLessEqual(cleanup["end"], expected_end + 0.05)
                self.assertLessEqual(waiting["timeout"], expected_end - budgets[3] - waiting["time"] + 0.05)

    def test_stale_alarm_retries_exact_child_cleanup_transition_and_wait_within_shared_end(self):
        for scenario in ["verification_reap_interrupted", "verification_reap_transition", "verification_reap_termination"]:
            with self.subTest(scenario=scenario):
                self.check_stale_alarm_cleanup(scenario)

    def check_stale_alarm_cleanup(self, scenario):
        run, report, events = self.run_fixture(scenario, coding=True, budgets=(3, 5, 1.5, 0.5))
        phase = json.loads((self.case_root / "verification-stall.json").read_text())
        self.assertEqual(phase["phase"], "git_fetch_stall")
        receipt = next(row for row in report["resources"]["verification_processes"] if row["pid"] == phase["pid"])
        gates = [json.loads(line) for line in (self.case_root / "verification-reaping.jsonl").read_text().splitlines()]
        waits = [row for row in gates if row["event"] == "wait_started"]
        self.assertEqual(sum(row["event"] == "stale_alarm" for row in gates), 1)
        self.assertTrue(waits)
        self.assertTrue(all(row["pid"] == phase["pid"] and row["pgid"] == phase["pid"] for row in waits))
        termination = [row for row in gates if row["event"] == "termination_attempt"]
        self.assertEqual(len(termination), 1)
        self.assertEqual((termination[0]["pid"], termination[0]["pgid"], termination[0]["signal"]), (phase["pid"], phase["pid"], 9))
        self.assertEqual(run.returncode, 1, report)
        self.assertEqual(report["outcome"], "FAIL")
        self.assertEqual(report["stages"][-1]["stage"], "source_fixture")
        self.assertIn("deadline", report["stages"][-1]["reason"])
        self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
        self.assertFalse(set(e.get("method") for e in events) & {"CreateAgent", "Send"})
        self.assertEqual(receipt["exit"], -9, report)
        self.assertGreaterEqual(len(waits), 2 if scenario == "verification_reap_interrupted" else 1)
        if scenario == "verification_reap_transition":
            alarm = next(row for row in gates if row["event"] == "stale_alarm")
            self.assertEqual(alarm["boundary"], "stdout.close")
            self.assertEqual(alarm["pgid"], phase["pid"])
            self.assertLess(alarm["time"], waits[0]["time"])
        if scenario == "verification_reap_termination":
            alarm = next(row for row in gates if row["event"] == "stale_alarm")
            self.assertEqual(alarm["boundary"], "killpg")
            self.assertTrue(alarm["signal_blocked"])
            self.assertEqual(alarm["pgid"], phase["pid"])
        confirmed = next(row for row in gates if row["event"] == "wait_confirmed")
        self.assertEqual(confirmed["pid"], phase["pid"])
        self.assertEqual(confirmed["exit"], receipt["exit"])
        cleanup = next(row for row in gates if row["event"] == "cleanup_bound")
        self.assertEqual(cleanup["returned_end"], cleanup["end"])
        self.assertTrue(all(row["cleanup_end"] == cleanup["end"] for row in waits))
        if scenario == "verification_reap_transition":
            self.assertEqual(alarm["cleanup_end"], cleanup["end"])
        self.assertTrue(all(0 < row["timeout"] <= cleanup["end"] - 0.5 - row["time"] + 0.05 for row in waits))
        self.assertLessEqual(cleanup["end"], waits[0]["time"] + 1.55)
        self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
        self.assertEqual(report["resources"]["verification_cleanup"], "REMOVED")
        self.assertEqual(report["resources"]["private_root_cleanup"], "REMOVED")
        self.assertLess(report["elapsed_seconds"], 5.1)

    def test_partial_frames_git_tests_and_cancel_obey_original_deadline(self):
        for scenario in ["partial_frame_trickle", "partial_frame_no_id", "git_stall", "test_stall"]:
            with self.subTest(scenario=scenario):
                # Each fault must reach its real phase after coding preparation.
                budgets = (*CODING_BUDGETS[:2], 1.5, 0.75)
                run, report, events = self.run_fixture(scenario, coding=True, budgets=budgets)
                self.assertEqual(run.returncode, 1, report)
                self.assertLess(report["elapsed_seconds"], budgets[1] + 0.1)
                self.assertIn("deadline", report["stages"][-1]["reason"])
                expected = {"partial_frame_trickle": "observation", "partial_frame_no_id": "Send", "git_stall": "source_fixture", "test_stall": "result_tests"}
                self.assertEqual(report["stages"][-1]["stage"], expected[scenario], report)
                if scenario.startswith("partial_frame"):
                    self.assertTrue(any(e.get("stall") == scenario for e in events), events)
                else:
                    phase = json.loads((self.case_root / "verification-stall.json").read_text())
                    self.assertEqual(phase["phase"], "unittest_stall" if scenario == "test_stall" else "git_fetch_stall")
                    self.assertIn(phase["pid"], [p["pid"] for p in report["resources"]["verification_processes"]])
                self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
                self.assertTrue(all(p["exit"] != "UNRESOLVED" for p in report["resources"].get("verification_processes", [])))
                if scenario == "partial_frame_no_id":
                    self.assertNotIn("CancelRun", [e.get("method") for e in events])

    def test_success_qualifies_exact_inputs_and_removes_owned_token(self):
        run, report, events = self.run_fixture()
        self.assertEqual(run.returncode, 0, report)
        self.assertEqual(report["preflight"], "PASS")
        self.assertEqual(report["resources"]["token_cleanup"], "REMOVED")
        self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
        launch = events[0]
        self.assertTrue(launch["env_key_matches"])
        self.assertTrue(launch["argv_excludes_secrets"])
        self.assertFalse(Path(launch["token_file"]).exists())
        self.assertFalse(Path(launch["root"]).exists())
        methods = [e["method"] for e in events if "method" in e]
        self.assertEqual(methods, ["Ping", "GetVersion", "Ping", "Me", "ListModels", "ListRepositories", "Shutdown"])
        self.assertTrue(all(e["key_matches"] for e in events if e.get("method") in ("Me", "ListModels", "ListRepositories")))

    def test_loopback_peer_startup_does_not_depend_on_reverse_dns(self):
        for scenario in ("dns_stall", "dns_error"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario)
                self.assertEqual(run.returncode, 0, {"report": report, "events": events})
                self.assertEqual(report["preflight"], "PASS")
                self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
                self.assertEqual(report["resources"]["token_cleanup"], "REMOVED")
                self.assertFalse(any("dns_lookup" in event for event in events), events)

    def test_missing_or_bad_manifest_never_starts_bridge(self):
        cases = [
            *[lambda m, field=field: m.pop(field) for field in [
                "credential_ref", "repository_policy", "repository", "disposable", "source_sha",
                "model", "max_cost_usd", "allowed_effects", "limits"]],
            lambda m: m.update(model="auto"),
            lambda m: m.update(disposable=False),
            lambda m: m.update(source_sha="main"),
            lambda m: m.update(max_cost_usd="NaN"),
            lambda m: m.update(max_cost_usd="0"),
            lambda m: m.update(allowed_effects=["merge"]),
            lambda m: m.update(repository_policy={"owner": "other", "repository": m["repository"]}),
            lambda m: m.update(limits={"CreateAgent": 2, "Send": 3}),
        ]
        for mutate in cases:
            with self.subTest(mutate=cases.index(mutate)):
                run, report, events = self.run_fixture(mutate=mutate)
                self.assertEqual(run.returncode, 1)
                self.assertEqual(report["stages"][-1]["stage"], "inputs")
                self.assertEqual(events, [])

    def test_selected_key_is_required_despite_ambient_login(self):
        run, report, events = self.run_fixture(env_change=lambda env: env.pop("EXPLICIT_FIXTURE_KEY"))
        self.assertEqual(run.returncode, 1)
        self.assertEqual(report["stages"][-1]["stage"], "inputs")
        self.assertEqual(events, [])

    def test_checksum_mismatch_never_launches(self):
        run, report, events = self.run_fixture("checksum_mismatch")
        self.assertEqual(run.returncode, 1)
        self.assertEqual(report["stages"][-1]["stage"], "pin")
        self.assertEqual(events, [])

    def test_archive_manifest_mismatch_never_launches(self):
        for field, value in [("sdkVersion", "latest"), ("protocol", "sdk.v2"), ("arch", "wrong"), ("entrypoint", "../other")]:
            with self.subTest(field=field):
                run, report, events = self.run_fixture(pin_change=lambda m: m.update({field: value}))
                self.assertEqual(run.returncode, 1)
                self.assertEqual(report["stages"][-1]["stage"], "pin")
                self.assertEqual(events, [])

    def test_runtime_auth_model_and_repository_mismatch_fail_closed(self):
        for scenario, stage in [("version_mismatch", "GetVersion"), ("bad_bearer_accepted", "bad_bearer"),
                                ("key_rejected", "Me"), ("model_mismatch", "ListModels"), ("repo_mismatch", "ListRepositories")]:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario)
                self.assertEqual(run.returncode, 1)
                self.assertEqual(report["preflight"], "FAIL")
                self.assertEqual(report["stages"][-1]["stage"], stage)
                self.assertEqual(report["resources"]["token_cleanup"], "REMOVED")
                self.assertFalse(Path(events[0]["token_file"]).exists())

    def test_stalls_and_trickle_do_not_renew_absolute_deadline(self):
        for scenario, stage in [("startup_stall", "startup"), ("startup_trickle", "startup"),
                                ("headers_stall", "Me"), ("headers_trickle", "Me"),
                                ("body_stall", "Me"), ("body_trickle", "Me"),
                                ("error_stall", "Me"), ("error_trickle", "Me")]:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario)
                self.assertEqual(run.returncode, 1)
                self.assertEqual(report["stages"][-1]["stage"], stage)
                self.assertIn("deadline", report["stages"][-1]["reason"])
                self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
                self.assertLess(report["elapsed_seconds"], 4.7)
                self.assertTrue(any(event.get("stall") == scenario for event in events), events)

    def test_stalled_shutdown_reserves_time_to_force_owned_process(self):
        run, report, events = self.run_fixture("shutdown_stall")
        self.assertEqual(run.returncode, 0, report)
        self.assertTrue(report["resources"]["forced_shutdown"])
        self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
        self.assertEqual(report["resources"]["token_cleanup"], "REMOVED")
        self.assertFalse(Path(events[0]["root"]).exists())

    def test_unknown_or_nonprivate_token_is_retained_without_touching_other_paths(self):
        for scenario in ["token_mode", "token_symlink", "foreign_token"]:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario)
                self.assertEqual(run.returncode, 1)
                self.assertEqual(report["preflight"], "FAIL")
                self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
                self.assertEqual(report["resources"]["token_cleanup"], "UNRESOLVED")
                self.assertNotIn("deadline", report["stages"][-1]["reason"])
                self.assertTrue(Path(events[0]["token_file"]).exists())
                self.assertTrue(Path(events[0]["root"]).exists())

    def test_replaced_token_retains_uncertainty_after_confirmed_exit(self):
        run, report, events = self.run_fixture("token_replaced")
        self.assertEqual(run.returncode, 1)
        self.assertEqual(report["preflight"], "PASS")
        self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
        self.assertEqual(report["resources"]["token_cleanup"], "UNRESOLVED")
        self.assertTrue(Path(events[0]["token_file"]).is_symlink())

    def test_cleanup_is_truncated_by_original_total_deadline(self):
        run, report, events = self.run_fixture("body_trickle", budgets=(3, 3.8, 60, 0.5))
        self.assertEqual(run.returncode, 1)
        self.assertLess(report["elapsed_seconds"], 3.9)
        self.assertTrue(any(event.get("stall") == "body_trickle" for event in events))
        self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
        self.assertEqual(report["resources"]["token_cleanup"], "REMOVED")

    def test_interrupted_launch_without_handle_retains_uncertain_resources(self):
        run, report, events = self.run_fixture("launch_uncertain")
        self.assertEqual(run.returncode, 1)
        self.assertEqual(report["resources"]["process_exit"], "UNRESOLVED")
        self.assertEqual(report["resources"]["token_cleanup"], "UNRESOLVED")
        self.assertTrue(Path(report["resources"]["private_root"]).exists())
        if events:
            self.assertTrue(Path(events[0]["token_file"]).exists())

    def test_malformed_discovery_shape_returns_redacted_failure_and_cleanup(self):
        for scenario in ["ready_array", "ready_null", "ready_scalar"]:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario)
                self.assertEqual(run.returncode, 1)
                self.assertEqual(report["preflight"], "FAIL")
                self.assertEqual(report["stages"][-1]["stage"], "startup")
                self.assertEqual(report["stages"][-1]["reason"], "bridge discovery must be a JSON object")
                self.assertEqual(report["resources"]["process_exit"], "CONFIRMED")
                self.assertEqual(report["resources"]["token_cleanup"], "UNRESOLVED")
                self.assertNotIn("deadline", report["stages"][-1]["reason"])
                self.assertTrue(Path(events[0]["token_file"]).exists())

    def test_missing_inputs_fail_closed_with_redacted_report(self):
        run = subprocess.run([sys.executable, str(CLIENT)], capture_output=True, text=True, timeout=3)
        self.assertEqual(run.returncode, 1)
        report = json.loads(run.stdout)
        self.assertEqual(report["preflight"], "FAIL")
        self.assertEqual(report["stages"][-1]["stage"], "inputs")
        self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
        self.assertEqual(report["coding"], "NOT RUN")
        self.assertEqual(run.stderr, "")


    def test_expired_entry_deadline_still_returns_a_redacted_failure_report(self):
        run, report, events = self.run_fixture(budgets=(0, 0, 60, 10))
        self.assertEqual(run.returncode, 1)
        self.assertEqual(report["preflight"], "FAIL")
        self.assertEqual(report["resources"]["process_exit"], "NOT STARTED")
        self.assertEqual(events, [])


    def test_expired_cleanup_budget_retains_process_and_token_uncertainty(self):
        run, report, events = self.run_fixture("body_trickle", budgets=(3, 3, 60, 10))
        self.assertEqual(run.returncode, 1)
        self.assertEqual(report["stages"][-1]["stage"], "Me")
        self.assertEqual(report["resources"]["process_exit"], "UNRESOLVED")
        self.assertEqual(report["resources"]["token_cleanup"], "UNRESOLVED")
        self.assertTrue(Path(report["resources"]["token_file"]).exists())
        self.assertLess(report["elapsed_seconds"], 3.2)


class ConnectReadTests(unittest.TestCase):
    """Small framing checks complement the normal-entry transport matrix."""
    def test_short_reads_preserve_exact_frame_bytes(self):
        api = runpy.run_path(str(CLIENT))
        class Split(io.BytesIO):
            def read(self, size):
                return super().read(min(size, 2))
        self.assertEqual(api["read_exact"](Split(b"\x00\x00\x00\x00\x02{}"), 7, api["Deadlines"]()), b"\x00\x00\x00\x00\x02{}")

    def test_partial_eof_does_not_become_successful_stream_end(self):
        api = runpy.run_path(str(CLIENT))
        with self.assertRaisesRegex(api["Failure"], "complete frame/end receipt"):
            api["read_exact"](io.BytesIO(b"\x00\x00"), 5, api["Deadlines"]())


if __name__ == "__main__":
    unittest.main()
