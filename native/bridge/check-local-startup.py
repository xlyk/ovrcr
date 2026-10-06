#!/usr/bin/env python3
"""Exercise the actual just run shell recipe using private fake external tools.

No Cargo compiler, Bridge builder, signer, native app or user installation runs.
The only copied executable is a private fake CLI under a private HOME.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import textwrap
from pathlib import Path

assert __debug__, "Run python3 -I without -O"
repo = Path(__file__).resolve().parents[2]
lines = (repo / "justfile").read_text().splitlines()
body = []
for line in lines[lines.index("run *args:") + 1:]:
    if line and not line.startswith("    "):
        break
    body.append(line[4:] if line.startswith("    ") else "")
recipe = "\n".join(body) + "\n"
assert recipe.startswith("#!/usr/bin/env bash\n")
assert "{{" not in recipe, "Fixture needs to account for Just interpolation"
fixture = Path(tempfile.mkdtemp(prefix="ovrcr-local-startup-"))
checks = 0
try:
    bin_dir = fixture / "bin"
    bin_dir.mkdir()
    script = fixture / "run-recipe"
    script.write_text(recipe)
    cli_source = (
        f"#!{sys.executable}\n"
        "import json, os, sys\n"
        "from pathlib import Path\n"
        "with Path(os.environ['OVRCR_STARTUP_FIXTURE_EVENTS']).open('a') as log:\n"
        " log.write(json.dumps({'kind':'cli','argv':sys.argv[1:],"
        "'mode':os.environ.get('OVRCR_BRIDGE_LOCAL_DEVELOPMENT'),"
        "'executable':sys.argv[0]})+'\\n')\n"
    )
    fake_rtk = f"#!{sys.executable}\n" + textwrap.dedent(
        """
        import json, os, sys
        from pathlib import Path
        argv = sys.argv[1:]
        assert argv.pop(0) == "proxy"
        with Path(os.environ["OVRCR_STARTUP_FIXTURE_EVENTS"]).open("a") as log:
            log.write(json.dumps({"kind":"tool", "argv":argv})+"\\n")
        if argv[:2] == ["cargo", "build"]:
            assert argv[2:6] == ["-p", "ovrcr", "--bin", "ovrcr"]
            assert argv[6] == "--target-dir" and len(argv) == 8
            binary = Path(argv[7]) / "debug/ovrcr"
            binary.parent.mkdir(parents=True, exist_ok=True)
            binary.write_text(os.environ["OVRCR_STARTUP_FIXTURE_CLI"])
            binary.chmod(0o755)
        elif argv[:2] == ["sh", "scripts/package-startup.sh"]:
            args = argv[2:]
            local = args[:1] == ["--local-development"]
            if local:
                args = args[1:]
            assert len(args) == 2
            output, callback = map(Path, args)
            assert callback.is_file()
            (output / "native/bridge").mkdir(parents=True, exist_ok=True)
            (output / "native/bridge/expected-contract.json").write_text(
                json.dumps({"local_development":local}))
            (output / "fixture-profile").write_text("local" if local else "production")
        else:
            raise AssertionError("Unapproved fixture command: "+repr(argv))
        """
    )
    (bin_dir / "rtk").write_text(fake_rtk)
    (bin_dir / "rtk").chmod(0o755)
    (bin_dir / "uname").write_text(
        f"#!{sys.executable}\nimport os\n"
        "print(os.environ.get('OVRCR_STARTUP_FIXTURE_KERNEL', 'Darwin'))\n"
    )
    (bin_dir / "uname").chmod(0o755)
    arguments = ["--fixture", "a value with spaces", "literal $value"]

    def run(home, target, mode, kernel="Darwin", expected_exit=0):
        home.mkdir(parents=True, exist_ok=True)
        events = fixture / "events.jsonl"
        events.write_text("")
        env = dict(os.environ)
        env.update(
            HOME=str(home), CARGO_TARGET_DIR=str(target),
            PATH=str(bin_dir) + os.pathsep + os.defpath,
            OVRCR_STARTUP_FIXTURE_EVENTS=str(events),
            OVRCR_STARTUP_FIXTURE_CLI=cli_source,
            OVRCR_STARTUP_FIXTURE_KERNEL=kernel,
        )
        env.pop("OVRCR_BRIDGE_LOCAL_DEVELOPMENT", None)
        if mode is not None:
            env["OVRCR_BRIDGE_LOCAL_DEVELOPMENT"] = mode
        result = subprocess.run(
            ["/bin/bash", str(script), *arguments], cwd=repo, env=env,
            capture_output=True, text=True, timeout=20,
        )
        assert result.returncode == expected_exit, (
            result.returncode, expected_exit, result.stdout, result.stderr
        )
        return result, [json.loads(line) for line in events.read_text().splitlines()]

    home = fixture / "private home"
    target = fixture / "target with spaces"
    result, observed = run(home, target, None)
    assert observed[1]["argv"] == [
        "sh", "scripts/package-startup.sh",
        str(target / "debug/ovrcr-startup"), str(target / "debug/ovrcr"),
    ]
    assert observed[-1]["kind"] == "cli" and observed[-1]["argv"] == arguments
    assert observed[-1]["mode"] is None
    production = home / ".local/lib/ovrcr"
    local = home / ".local/lib/ovrcr-local-development"
    assert (production / "fixture-profile").read_text() == "production"
    assert not local.exists() and not (home / "Applications").exists()
    checks += 1

    (production / "preserve-production").write_bytes(b"production remains untouched")
    result, observed = run(home, target, "1")
    assert observed[1]["argv"] == [
        "sh", "scripts/package-startup.sh", "--local-development",
        str(target / "debug/ovrcr-startup-local-development"),
        str(target / "debug/ovrcr"),
    ]
    assert observed[-1]["mode"] == "1" and observed[-1]["argv"] == arguments
    assert (local / "fixture-profile").read_text() == "local"
    assert (production / "preserve-production").read_bytes() == b"production remains untouched"
    assert not (home / "Applications").exists()
    checks += 1

    (local / "preserve-local").write_bytes(b"local remains untouched")
    for mode in ["0", ""]:
        result, observed = run(home, target, mode)
        assert "--local-development" not in observed[1]["argv"]
        assert (local / "preserve-local").read_bytes() == b"local remains untouched"
        assert observed[-1]["mode"] == mode
        checks += 1

    installed_cli = home / ".local/bin/ovrcr"
    before = installed_cli.read_bytes()
    for invalid in ["true", "local-development", " 1", "1\n"]:
        result, observed = run(home, target, invalid, expected_exit=64)
        assert observed == [], "Invalid opt-in invoked a build or installed client"
        assert installed_cli.read_bytes() == before
        assert (local / "preserve-local").read_bytes() == b"local remains untouched"
        checks += 1

    unmanaged_home = fixture / "unmanaged home"
    unmanaged = unmanaged_home / ".local/lib/ovrcr-local-development"
    unmanaged.mkdir(parents=True)
    (unmanaged / "owned-by-user").write_bytes(b"do not replace")
    result, observed = run(unmanaged_home, fixture / "unmanaged target", "1", expected_exit=1)
    assert (unmanaged / "owned-by-user").read_bytes() == b"do not replace"
    assert not (unmanaged_home / ".local/bin/ovrcr").exists()
    assert all(item["kind"] != "cli" for item in observed)
    checks += 1

    symlink_home = fixture / "symlink home"
    assets = symlink_home / ".local/lib/ovrcr-local-development"
    assets.parent.mkdir(parents=True)
    assets.symlink_to(fixture / "missing target", target_is_directory=True)
    result, observed = run(symlink_home, fixture / "symlink target", "1", expected_exit=1)
    assert assets.is_symlink() and not assets.exists()
    assert not (symlink_home / ".local/bin/ovrcr").exists()
    assert all(item["kind"] != "cli" for item in observed)
    checks += 1

    linux_home = fixture / "Linux home"
    result, observed = run(linux_home, fixture / "Linux target", "1", kernel="Linux")
    assert len(observed) == 2 and observed[0]["argv"][0] == "cargo"
    assert observed[1]["kind"] == "cli" and observed[1]["argv"] == arguments
    assert not (linux_home / ".local/lib").exists() and not (linux_home / "Applications").exists()
    checks += 1
except BaseException:
    print(f"Failed private startup fixture retained at {fixture}", file=sys.stderr)
    raise
else:
    shutil.rmtree(fixture)
    print(f"{checks} actual startup recipe checks passed; fake tools only, no native install or launch")
