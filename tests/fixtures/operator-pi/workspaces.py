# Controlled-provider tool body for the production Operator acceptance fixture.
# The test supplies one fixed action name through argv.
import json
import os
import subprocess
import sys
import time
from typing import Any

action = sys.argv[1]
cli = os.environ["OVRCR_EXECUTABLE"]
repo = os.path.join(os.environ["HOME"], "repo")
root = os.path.join(os.environ["HOME"], "fixture-workspaces")


def call(*args):
    return json.loads(subprocess.check_output([cli, "--json", *args], text=True))


def refusal(*args):
    result = subprocess.run([cli, "--json", *args], text=True, capture_output=True)
    assert result.returncode != 0, (args, result.stdout)
    return {"code": result.returncode, "stdout": result.stdout, "stderr": result.stderr}


def git(path, *args):
    return subprocess.check_output(["git", "-C", path, *args], text=True).strip()


def inventory():
    return {"projects": call("project", "list"), "workspaces": call("workspace", "list"),
            "sessions": [row for row in call("terminal", "list") if row["scope"] == "Workspace"]}


before = inventory()
result: dict[str, Any] = {"executable": cli, "instance": os.environ["OVRCR_HOME"], "before": before}
if action == "inspect":
    result.update(repo=git(repo, "rev-parse", "--show-toplevel"),
                  branch=git(repo, "branch", "--show-current"),
                  git_worktrees=git(repo, "worktree", "list", "--porcelain"))
elif action == "missing-base":
    result["refusal"] = refusal("workspace", "create", "--project", "fixture",
                                "--new-branch", "must-not-exist")
elif action == "provision":
    assert not before["projects"]
    call("project", "add", "fixture", repo, "--workspace-root", root)
    call("workspace", "create", "--project", "fixture", "--new-branch", "feature/setup", "--base", "main")
    workspace = call("workspace", "get", "--project", "fixture", "--branch", "feature/setup")
    result.update(workspace=workspace, head=git(workspace["path"], "rev-parse", "HEAD"),
                  base=git(repo, "rev-parse", "main"))
elif action == "reuse":
    project = call("project", "get", "fixture")
    assert os.path.realpath(project["repo"]) == os.path.realpath(repo)
    workspace = call("workspace", "get", "--project", "fixture", "--branch", "feature/setup")
    result["workspace"] = workspace
elif action in ("terminal", "agent"):
    workspace = call("workspace", "get", "--project", "fixture", "--branch", "feature/setup")
    assert git(workspace["path"], "branch", "--show-current") == "feature/setup"
    name = "requested-" + action
    matches = [row for row in before["sessions"] if row["name"] == name and row["cwd"] == workspace["path"]]
    if matches:
        session = matches[0]
    else:
        argv = ["/bin/sh", "-c", "printf 'TERMINAL_CWD='; pwd; exec /bin/sh"]
        if action == "agent":
            argv = [cli, "agent", "run", "pi", "--", os.environ["OVRCR_PI_EXECUTABLE"]]
        session = call("terminal", "create", "--project", "fixture", "--path", workspace["path"], "--name", name, "--", *argv)
    identity = str(session["id"])
    deadline = time.monotonic() + 20
    while True:
        row = next(row for row in call("terminal", "list") if str(row["id"]) == identity)
        screen = call("terminal", "read", identity)
        text = screen["text"]
        assert row["cwd"] == workspace["path"] and row["phase"] == "running", row
        if action == "terminal" and "TERMINAL_CWD=" + workspace["path"] in text:
            assert row["kind"] == "terminal"
            break
        editor_borders = sum(bool(line.strip()) and set(line.strip()) == {"─"} for line in text.splitlines())
        if (action == "agent" and row["kind"] == {"agent": "pi"} and row.get("agent")
                and "operator-fixture" in text and "pi v0.87.1" in text and editor_borders == 2):
            break
        assert time.monotonic() < deadline, (row, screen)
        time.sleep(0.05)
    result.update(session=row, screen=screen, startup_screen=screen)
    if action == "agent":
        call("terminal", "send", identity, "--text", "WORKSPACES_AGENT_PROBE: Reply with the requested readiness receipt.")
        while True:
            screen = call("terminal", "read", identity)
            row = next(row for row in call("terminal", "list") if str(row["id"]) == identity)
            if "WORKSPACES_AGENT_RESPONSE" in screen["text"] and row.get("unread"):
                break
            assert time.monotonic() < deadline, (row, screen)
            time.sleep(0.05)
        result.update(session=row, screen=screen)
elif action == "ambiguous":
    result["refusal"] = refusal("workspace", "get", "--project", "fixture", "--branch", "feature/setup")
    result["candidates"] = [row for row in before["workspaces"] if row["branch"] == "feature/setup"]
elif action == "detached":
    row = next(row for row in before["workspaces"] if row["branch"].startswith("detached @ "))
    result.update(workspace=row, head=git(row["path"], "rev-parse", "HEAD"),
                  refusal=refusal("workspace", "get", "--project", "fixture", "--branch", row["branch"]))
elif action == "root":
    result["refusal"] = refusal("terminal", "create", "--project", "fixture", "--path", repo, "--name", "forbidden", "--", "/bin/sh")
    result["branch"] = git(repo, "branch", "--show-current")
elif action == "trust":
    row = next(row for row in before["sessions"] if row["name"] == "trust-screen")
    result.update(session=row, screen=call("terminal", "read", str(row["id"])))
else:
    raise AssertionError(action)
result["after"] = inventory()
print("WORKSPACES_RESULT=" + json.dumps(result))
