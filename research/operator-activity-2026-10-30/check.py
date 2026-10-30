import gzip, hashlib, json, os, pathlib, subprocess, sys, tempfile

repo = pathlib.Path(__file__).resolve().parents[2]
evidence = pathlib.Path(__file__).resolve().parent / "checks"
name, *command = sys.argv[1:]
assert not (evidence / (name + ".json")).exists(), "preserve original attempts"
with tempfile.TemporaryDirectory(prefix="ovrcr-activity-check-", dir="/private/tmp") as private:
    root = pathlib.Path(private)
    for directory in ("home", "temp", "instance"):
        (root / directory).mkdir(mode=0o700)
    env = os.environ.copy()
    for key in list(env):
        if any(part in key for part in ("API_KEY", "TOKEN", "SECRET", "PROXY")) or key.startswith(("OVRCR_", "XDG_", "CLAUDE_", "CODEX_")):
            env.pop(key)
    env.update(HOME=str(root / "home"), TMPDIR=str(root / "temp") + "/", OVRCR_HOME=str(root / "instance"), OVRCR_SOCKET=str(root / "server.sock"), RUSTUP_HOME="/Users/xlyk/.rustup", CARGO_HOME="/Users/xlyk/.cargo", NO_PROXY="*", no_proxy="*")
    child = subprocess.Popen(command, cwd=repo, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    output = bytearray()
    while data := child.stdout.read1(65536):
        output.extend(data)
        sys.stdout.buffer.write(data)
        sys.stdout.buffer.flush()
    code = child.wait()
    raw = bytes(output)
    (evidence / (name + ".log.gz")).write_bytes(gzip.compress(raw, mtime=0))
    (evidence / (name + ".json")).write_text(json.dumps({"command": command, "exit": code, "source_revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(), "isolated_root_removed": private, "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}, indent=2) + "\n")
sys.exit(code)
