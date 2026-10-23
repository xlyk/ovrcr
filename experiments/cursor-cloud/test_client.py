"""Offline acceptance at the standalone client entry point; no real credentials."""
import hashlib
import io
import json
import os
import platform
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
    def run_fixture(self, scenario="success", mutate=None, pin_change=None, env_change=None, budgets=(3, 5, 1.5, 0.75)):
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
        if mutate:
            mutate(manifest)
        manifest_path = root / "manifest.json"
        manifest_path.write_text(json.dumps(manifest))
        code = Path(__file__).with_name("fake_bridge.py").read_text().replace(
            "FIXTURE_CONFIG", repr({"audit": str(audit), "unrelated": str(unrelated), "scenario": scenario}))
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
globals_ = module["main"].__globals__
globals_.update(PINS={sys.argv[2]:sys.argv[3]}, WORK_SECONDS=%r, TOTAL_SECONDS=%r,
                CLEANUP_SECONDS=%r, SHUTDOWN_RESERVE=%r)
original_spawn = globals_["subprocess"].Popen
owned = []
def tracked_spawn(*args, **kwargs):
    process = original_spawn(*args, **kwargs)
    owned.append(process)
    if %r == "launch_uncertain":
        time.sleep(0.2)
        raise TimeoutError("synthetic interrupted process operation")
    return process
globals_["subprocess"].Popen = tracked_spawn
try:
    result = module["main"](sys.argv[4:])
finally:
    for process in owned:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=1)
sys.exit(result)
""" % (*budgets, scenario)
        env = dict(os.environ, EXPLICIT_FIXTURE_KEY="API-SECRET-SENTINEL-363", CURSOR_API_KEY="AMBIENT-MUST-NOT-WIN")
        if env_change:
            env_change(env)
        start = time.monotonic()
        run = subprocess.run([sys.executable, "-I", "-c", runner, str(CLIENT), host + "-" + arch,
                              digest, "--manifest", str(manifest_path), "--archive", str(archive)],
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
        report = json.loads(run.stdout)
        self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
        methods = [e["method"] for e in events if "method" in e]
        self.assertFalse(set(methods) & {"CreateAgent", "Send"})
        self.assertEqual(unrelated.read_text(), "UNRELATED-SECRET")
        # Only the test owns this private root; preserve the report before fixture cleanup.
        if report["resources"].get("private_root"):
            self.addCleanup(shutil.rmtree, report["resources"]["private_root"], True)
        return run, report, events

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

    def test_missing_inputs_fail_closed_with_redacted_report(self):
        run = subprocess.run([sys.executable, str(CLIENT)], capture_output=True, text=True, timeout=3)
        self.assertEqual(run.returncode, 1)
        report = json.loads(run.stdout)
        self.assertEqual(report["preflight"], "FAIL")
        self.assertEqual(report["stages"][-1]["stage"], "inputs")
        self.assertEqual(report["attempts"], {"CreateAgent": 0, "Send": 0})
        self.assertEqual(report["coding"], "NOT RUN")
        self.assertEqual(run.stderr, "")


if __name__ == "__main__":
    unittest.main()
