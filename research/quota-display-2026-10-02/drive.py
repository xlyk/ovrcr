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
GUARD = 'tell application "System Events" to if (unix id of (first process whose frontmost is true)) is not {pid} then error "focus moved"'
def guarded(action):
    # Check focus and send the key in one AppleScript run, so a key never
    # lands in another fixture's window that took focus between two calls.
    for _ in range(20):
        front()
        try:
            osa(GUARD.format(pid=PID), f'tell application "System Events" to {action}')
            time.sleep(0.6); return
        except subprocess.CalledProcessError:
            time.sleep(2)
    raise SystemExit("OVRCR GUI window lost focus; refusing to type")
def keys(text):
    for ch in text: guarded(f'keystroke "{ch}"')
def code(n):
    guarded(f"key code {n}")
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
        elif kind == "ctrl": guarded(f'keystroke "{arg}" using control down')
        elif kind == "shot": print(f"=== {arg}"); print(capture(arg))
        elif kind == "size":
            w, h = arg.split("x")
            osa(f'tell application "System Events" to set size of window 1 of (first process whose unix id is {PID}) to {{{w}, {h}}}'); time.sleep(1.5)
        elif kind == "wait": time.sleep(float(arg))
