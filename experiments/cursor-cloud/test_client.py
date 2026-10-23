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


class PreflightEntryPointTests(unittest.TestCase):
    def run_fixture(self, scenario="success", mutate=None, pin_change=None, env_change=None, budgets=(3, 5, 1.5, 0.75), coding=False):
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
                (repo / "addition.py").write_text("import time\ntime.sleep(30)\ndef add(a,b): return a+b\n")
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
kind = 'credential' if 'git-credential' in sys.argv else 'status'
with open(""" + repr(str(root / "github-audit.jsonl")) + r""", 'a') as audit:
    audit.write(json.dumps({'kind': kind, 'config_matches': os.environ.get('GH_CONFIG_DIR') == config, 'ambient_excluded': not (os.environ.get('GH_TOKEN') or os.environ.get('GITHUB_TOKEN'))}) + '\n')
if kind == 'status':
    print(json.dumps([{'login': 'other' if scenario == 'github_wrong_login' else 'fixture', 'active': True, 'state': 'error' if scenario == 'github_auth_bad' else 'success', 'tokenSource': 'keyring'}]))
else:
    request = sys.stdin.read()
    if scenario == 'github_helper_refused' or 'host=github.com' not in request:
        sys.exit(1)
    print('username=fixture\npassword=GITHUB-SECRET-SENTINEL-364\n')
""")
            gh_peer.chmod(0o700)
            shim = root / "git-peer"
            shim.write_text("#!" + sys.executable + "\n" + r"""import json,os,subprocess,sys
args = sys.argv[1:]
if 'fetch' in args:
    options = args[:args.index('fetch')]
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
            if scenario == "git_stall":
                shim.write_text("#!" + sys.executable + "\nimport time\ntime.sleep(30)\n")
            shim.chmod(0o700)
            git_override = "g['GIT']=" + repr(str(shim)) + "; g['GH']=" + repr(str(gh_peer)) + "; "
            config.update(source_sha=selected)
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
import runpy, sys, time
module = runpy.run_path(sys.argv[1])
g = module["main"].__globals__
g.update(PINS={sys.argv[2]:sys.argv[3]}, WORK_SECONDS=%r, TOTAL_SECONDS=%r,
         CLEANUP_SECONDS=%r, SHUTDOWN_RESERVE=%r)
%s
original_spawn = g["subprocess"].Popen
owned = []
scenario = %r
def tracked_spawn(*args, **kwargs):
    process = original_spawn(*args, **kwargs)
    owned.append(process)
    if scenario == "launch_uncertain" or (scenario == "verification_launch_uncertain" and args[0][0] == g["GH"]):
        time.sleep(0.2)
        raise TimeoutError("synthetic interrupted process operation")
    return process
g["subprocess"].Popen = tracked_spawn
try:
    result = module["main"](sys.argv[4:])
finally:
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
            for name, data in items.items():
                fd = os.open(target / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                with os.fdopen(fd, "wb") as output:
                    output.write(data)
        return run, report, events

    def test_coding_verifies_patch_and_exact_write_ahead_order(self):
        run, report, events = self.run_fixture(coding=True, budgets=(2, 2.6, 0.4, 0.15))
        self.assertEqual(run.returncode, 0, report)
        self.assertEqual(report["coding"], "PASS")
        self.assertEqual(report["attempts"], {"CreateAgent": 1, "Send": 1})
        methods = [event["method"] for event in events if "method" in event]
        self.assertEqual(methods, ["Ping", "GetVersion", "Ping", "Me", "ListModels", "ListRepositories", "CreateAgent", "GetAgent", "Send", "Shutdown"])
        self.assertTrue(all(event["write_ahead"] for event in events if event.get("method") in {"CreateAgent", "Send"}))
        self.assertEqual(report["result"]["tests"], "PASS; 3 tests")
        self.assertEqual(report["control_recovery"], "NOT RUN")
        journal = self.case_root / "journal"
        rows = [json.loads(line) for line in (journal / "events.jsonl").read_text().splitlines()]
        claims = [row for row in rows if row["event"] == "claim"]
        self.assertEqual(len({row["operation_key"] for row in claims}), 2)
        self.assertFalse(list(journal.glob("*.request.json")))
        self.assertEqual([row["method"] for row in rows if row["event"] == "receipt"], ["CreateAgent", "Send"])

    def test_lost_receipts_close_admission_and_retain_exact_original_request(self):
        for scenario, expected, retained in [("creation_lost", {"CreateAgent": 1, "Send": 0}, "CreateAgent"),
                                               ("send_lost", {"CreateAgent": 1, "Send": 1}, "Send")]:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True, budgets=(2, 2.6, 0.4, 0.15))
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
            "missing_done": "coding lacks", "wrong_done": "done identity/order", "end_error": "Connect end-stream error",
            "missing_end": "Connect stream ended", "bad_flags": "invalid/oversized Connect frame", "bad_json": "invalid input",
            "duplicate_result": "duplicate terminal result", "after_done": "message after done", "result_bad_git": "result repository/branch",
            "result_bad_branch": "result repository/branch", "sdk_bad_payload": "SDK envelope payload", "probe_bool_exit": "initial source probe differs",
            "wrong_ancestry": "independent Git/fixture command failed",
        }
        for scenario in cases:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True, budgets=(2.2, 2.8, 0.4, 0.15))
                self.assertEqual(run.returncode, 1, report)
                self.assertNotEqual(report["coding"], "PASS")
                self.assertIn(reasons[scenario], report["stages"][-1]["reason"], report)
                methods = [e["method"] for e in events if "method" in e]
                self.assertEqual(methods.count("CreateAgent"), 1)
                self.assertLessEqual(methods.count("Send"), 1)
                self.assertFalse(set(methods) & {"ListRuns", "ObserveRun", "ResumeAgent"})
                self.assertTrue((self.case_root / "journal" / "events.jsonl").exists())

    def test_selected_github_helper_authenticates_private_fetch_and_refuses_bad_auth(self):
        for scenario in ("success", "github_wrong_login", "github_auth_bad", "github_helper_refused"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True, budgets=(3, 3.6, 0.4, 0.15),
                                                       env_change=lambda env: env.update(GH_TOKEN="AMBIENT_GH_MUST_NOT_WIN", GITHUB_TOKEN="AMBIENT_GITHUB_MUST_NOT_WIN"))
                self.assertEqual(run.returncode, 0 if scenario == "success" else 1, report)
                gh_rows = [json.loads(line) for line in (self.case_root / "github-audit.jsonl").read_text().splitlines()]
                self.assertTrue(all(row["ambient_excluded"] and row["config_matches"] for row in gh_rows))
                if scenario == "success":
                    self.assertEqual(sum(row["kind"] == "credential" for row in gh_rows), 3)
                    git_rows = [json.loads(line) for line in (self.case_root / "git-auth-audit.jsonl").read_text().splitlines()]
                    self.assertTrue(all(row["credential_supplied"] and row["other_remote_refused"] for row in git_rows))
                    self.assertEqual(report["github_auth"]["login"], "fixture")
                else:
                    self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
                    self.assertEqual(report["stages"][-1]["stage"], "source_fixture" if scenario == "github_helper_refused" else "github_auth")
                self.assertEqual((self.case_root / "github-config" / "owned-marker").read_text(), "preserve selected config")

    def test_independent_fixture_source_admission_precedes_any_creation(self):
        for scenario in ("unpublished_source", "same_default_source", "wrong_baseline"):
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True, budgets=(2.2, 2.8, 0.4, 0.15))
                self.assertEqual(run.returncode, 1, report)
                self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
                self.assertEqual(report["stages"][-1]["stage"], "source_fixture")
                self.assertFalse(set(e.get("method") for e in events) & {"CreateAgent", "Send"})

    def test_private_owner_storage_and_capacity_fail_before_new_effect(self):
        for scenario, creates in [("competing_owner", 0), ("retained_journal", 0), ("request_conflict", 0), ("capacity_before_create", 0), ("receipt_capacity", 1)]:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True, budgets=(2.2, 2.8, 0.4, 0.15))
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
                run, report, events = self.run_fixture(scenario, coding=True, budgets=(2.2, 2.8, 0.4, 0.15))
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

    def test_only_exact_known_active_run_can_be_cancelled_and_ack_is_not_terminal(self):
        for scenario, outcome in [("cancel_known", "RUN_LIFECYCLE_STATUS_CANCELLED"), ("cancel_unconfirmed", "UNRESOLVED"), ("cancel_stall", "UNRESOLVED")]:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True, budgets=(2.2, 2.8, 0.4, 0.15))
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

    def test_partial_frames_git_tests_and_cancel_obey_original_deadline(self):
        for scenario in ["partial_frame_trickle", "partial_frame_no_id", "git_stall", "test_stall"]:
            with self.subTest(scenario=scenario):
                run, report, events = self.run_fixture(scenario, coding=True, budgets=(1.6, 2.1, 0.4, 0.15))
                self.assertEqual(run.returncode, 1, report)
                self.assertLess(report["elapsed_seconds"], 2.2)
                self.assertIn("deadline", report["stages"][-1]["reason"])
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
