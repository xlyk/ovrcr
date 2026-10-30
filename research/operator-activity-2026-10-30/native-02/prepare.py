"""Reuse the exact frozen, reviewed native artifacts; no Cargo or target writes."""
import gzip, hashlib, json, pathlib, shutil, subprocess, tempfile

evidence = pathlib.Path(__file__).resolve().parent
repo = evidence.parents[2]
previous = evidence.parent / "native-01"
state = json.loads((previous / "state.json").read_text())
for name, digest in state["sha256"].items():
    assert hashlib.sha256((pathlib.Path(state["bundle"]) / "Contents/MacOS" / name).read_bytes()).hexdigest() == digest
for path, digest in state["source_sha256"].items():
    assert hashlib.sha256((repo / path).read_bytes()).hexdigest() == digest
artifacts = [json.loads(line) for line in gzip.decompress((previous / "holder-build.log.gz").read_bytes()).decode().splitlines() if line.startswith("{")]
holder = pathlib.Path(next(item["executable"] for item in artifacts if item.get("reason") == "compiler-artifact" and item.get("target", {}).get("name") == "operator_e2e"))
assert hashlib.sha256(holder.read_bytes()).hexdigest() == state["holder_sha256"]
subprocess.run(["git", "diff", "--exit-code", state["source_revision"], "--", "crates", "tests"], cwd=repo, check=True)
root = pathlib.Path(tempfile.mkdtemp(prefix="ovrcr-activity-native2-", dir="/private/tmp"))
root.chmod(0o700)
for name in ("home", "temp", "control"):
    (root / name).mkdir(mode=0o700)
(root / "gitconfig").write_text("[user]\nname = OVRCR Fixture\nemail = fixture@ovrcr.invalid\n")
shutil.copy2(holder, root / "operator_e2e")
state.update(root=str(root), holder=str(root / "operator_e2e"), evidence_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(), reused_frozen_artifacts=True)
state.pop("demo", None)
state.pop("owner_tool_sessions", None)
(evidence / "state.json").write_text(json.dumps(state, indent=2) + "\n")
print(json.dumps(state, indent=2))
