// Explicitly loaded only for OVRCR task runs; discovered extensions stay disabled.
import { spawn } from "node:child_process";
import { createBashToolDefinition } from "@earendil-works/pi-coding-agent";

// Keep the process-group leader alive after the command returns. FD3 reports the
// command result; FD4 belongs to Pi. Its EOF watchdog owns cleanup even if Pi
// dies before OVRCR's first descendant scan. User commands cannot inherit either.
const anchorScript = `
anchor=$$
( IFS= read -r _ <&4; kill -KILL -- "-$anchor" ) 3>&- &
/bin/bash -c "$1" 3>&- 4>&-
result=$?
printf '%s\n' "$result" >&3
wait
`;

export default function (pi) {
  const anchors = new Map();
  function kill(child) {
    if (!child.pid || !anchors.has(child)) return;
    try { process.kill(-child.pid, "SIGKILL"); }
    catch (error) { if (error.code !== "ESRCH") throw error; }
  }
  pi.on("session_shutdown", async () => {
    const exiting = [...anchors.values()];
    for (const child of anchors.keys()) kill(child);
    await Promise.all(exiting);
  });
  pi.registerTool(createBashToolDefinition(process.cwd(), {
    operations: {
      exec(command, cwd, { onData, signal, timeout, env }) {
        if (signal?.aborted) return Promise.reject(new Error("aborted"));
        if (timeout !== undefined && (!Number.isFinite(timeout) || timeout <= 0 || timeout * 1000 > 2_147_483_647)) {
          return Promise.reject(new Error("timeout must be positive and at most 2147483.647 seconds"));
        }
        return new Promise((resolve, reject) => {
          const child = spawn("/bin/bash", ["-c", anchorScript, "ovrcr-task-bash", command], {
            cwd, env, detached: true,
            stdio: ["ignore", "pipe", "pipe", "pipe", "pipe"],
          });
          let exited;
          anchors.set(child, new Promise(resolve => { exited = resolve; }));
          let timer;
          let outputIdleTimer;
          let exitCode;
          let timedOut = false;
          let completed = false;
          let result = "";
          const finish = (error, exitCode) => {
            if (completed) return;
            completed = true;
            clearTimeout(timer);
            clearTimeout(outputIdleTimer);
            signal?.removeEventListener("abort", abort);
            // As with Pi's normal backend, background output after command exit
            // does not keep the completed tool open. Drain it without updates.
            child.stdout?.removeListener("data", output);
            child.stderr?.removeListener("data", output);
            child.stdout?.resume();
            child.stderr?.resume();
            if (error) reject(error); else resolve({ exitCode });
          };
          const abort = () => {
            try { kill(child); }
            catch (error) { finish(error); }
          };
          child.once("error", error => {
            anchors.delete(child);
            exited();
            finish(error);
          });
          child.once("exit", (_code, _signal) => {
            anchors.delete(child);
            exited();
            child.stdio[3].destroy();
            child.stdio[4].destroy();
            finish(new Error(timedOut ? `timeout:${timeout}` : signal?.aborted ? "aborted" : "task shell anchor exited before command completion"));
          });
          // Match Pi 0.84.4's post-exit stdout grace: every chunk resets
          // the idle window, so a result on FD3 cannot truncate a busy pipe.
          const drain = () => {
            clearTimeout(outputIdleTimer);
            outputIdleTimer = setTimeout(() => finish(null, exitCode), 100);
          };
          const output = bytes => {
            onData(bytes);
            if (exitCode !== undefined) drain();
          };
          child.stdout.on("data", output);
          child.stderr.on("data", output);
          child.stdio[3].on("data", bytes => {
            result += bytes.toString();
            if (result.includes("\n")) {
              exitCode = Number(result.trim());
              drain();
            }
          });
          signal?.addEventListener("abort", abort, { once: true });
          if (signal?.aborted) abort();
          if (timeout !== undefined) {
            timer = setTimeout(() => { timedOut = true; abort(); }, timeout * 1000);
          }
        });
      },
    },
  }));
}
