"""Drive the OVRCR GUI window: real keystrokes, AX text and window screenshots."""
import subprocess, sys, time
import os
PID, WIN = int(os.environ["GUI_PID"]), int(os.environ["GUI_WIN"])
OUT = os.path.dirname(os.path.abspath(__file__))
def osa(*lines):
    args = []
    for l in lines: args += ["-e", l]
    return subprocess.run(["osascript", *args], capture_output=True, text=True, check=True).stdout
def front():
    for _ in range(60):
        osa(f'tell application "System Events" to set frontmost of (first process whose unix id is {PID}) to true')
        time.sleep(0.4)
        front_name = osa('tell application "System Events" to get unix id of first process whose frontmost is true').strip()
        if front_name == str(PID):
            return
    raise SystemExit("OVRCR GUI window is not frontmost; refusing to type")
def keys(text):
    front()
    quoted = text.replace('"', '" & quote & "')
    osa(f'tell application "System Events" to keystroke "{quoted}"'); time.sleep(0.6)
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
