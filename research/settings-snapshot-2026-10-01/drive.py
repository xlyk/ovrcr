"""Drive the OVRCR GUI window: real keystrokes, AX text and window screenshots."""
import subprocess, sys, time
PID, WIN = 77297, 46004
OUT = "/Users/xlyk/Code/ovrcr-workspaces/fe67772ef071ce69465bf913f3566ae5/research/settings-snapshot-2026-10-01"
def osa(*lines):
    args = []
    for l in lines: args += ["-e", l]
    return subprocess.run(["osascript", *args], capture_output=True, text=True, check=True).stdout
def front():
    osa(f'tell application "System Events" to set frontmost of (first process whose unix id is {PID}) to true')
    time.sleep(0.4)
def keys(text):
    front()
    osa(f'tell application "System Events" to keystroke "{text}"'); time.sleep(0.6)
def code(n):
    front()
    osa(f'tell application "System Events" to key code {n}'); time.sleep(0.6)
def ax():
    out = osa(f'tell application "System Events" to tell (first process whose unix id is {PID}) to set axLines to value of static texts of group 1 of window 1',
              "set AppleScript's text item delimiters to \"\u241e\"",
              'return axLines as text')
    return out
def capture(name):
    front(); time.sleep(0.8)
    rows = [r.rstrip() for r in ax().rstrip("\n").split("\u241e")]
    open(f"{OUT}/{name}.txt", "w").write("\n".join(rows) + "\n")
    subprocess.run(["screencapture", "-x", "-o", "-l", str(WIN), f"{OUT}/{name}.png"], check=True)
    return "\n".join(rows)
if __name__ == "__main__":
    for step in sys.argv[1:]:
        kind, _, arg = step.partition(":")
        if kind == "keys": keys(arg)
        elif kind == "code": code(int(arg))
        elif kind == "shot": print(f"=== {arg}"); print(capture(arg))
