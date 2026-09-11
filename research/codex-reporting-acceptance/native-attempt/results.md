# Isolated native startup: authentication prerequisite

Correction to source checkpoint `d47e99f5f401eb1c9b37af1a1936bcb32b8c9971`, 2026-09-11 UTC. The one permitted native matrix was attempted, with its one repeat used solely for a concrete harness observation failure. No further attempts remain in this bounded task.

Attempt 01 failed in the harness: sandbox denied `/bin/ps`; exit 1. Its missing capture/PID and incomplete cleanup proof are preserved in [failure record](attempt-01-failure.txt). PTY closure and an exact-argv no-match are recorded, but do not prove group absence. Retain `/private/tmp/ovrcr-codex-native-sd67gle4` pending recovery review.

Attempt 02 used the same installed 0.153.0 executable with a fresh empty provider home, home and workspace. The child environment contained only `HOME`, `CODEX_HOME`, `PATH`, `TERM`, and `LANG`; no provider credentials or inherited agent/session config. No hook was installed, login started, key read, trust granted, remote observer connected or thread resumed. The harness responded only to terminal cursor-position query with ESC[1;1R, not to a user prompt.

Exact argv and kernel process identity are in [record.json](record.json); the [PTY byte capture](terminal.txt) retains native output, not echoed commands. It displayed:

```text
Welcome to Codex, OpenAI's command-line coding agent
Sign in with ChatGPT to use Codex as part of your paid plan
or connect an API key for usage-based billing
> 1. Sign in with ChatGPT
2. Sign in with Device Code
3. Provide your own API key
Press enter to continue
```

This text is a readable transcription of the captured cursor-positioned screen. No login selection/Enter was sent. The capture is PTY evidence, not CUA screenshot or accessibility evidence, and does not prove an assistant response or hook event.

The concrete blocker is native authentication onboarding in the empty isolated configuration. Source/root/header/lineage observation cannot proceed in this attempt without selecting an authentication route. No credentials were authorized. Hook trust has not been encountered; do not report it as a failed check. The rollout candidate remains possible but uncertified.

Attempt 02 PID=PGID=TPGID `53183`, PPID `53129`, TTY `ttys016`, state `Ss+`. After 12 seconds the harness sent SIGTERM to its still-owned group, reaped native wait status 15 (terminated by signal 15, **not normal exit 0**), then verified `killpg(53183, 0)` returned process-not-found. Harness exit 0 only means capture/cleanup completed. This is not native normal-exit lifecycle acceptance. Its task-owned disposable directory was removed after these evidence files were copied; attempt 01 remains retained.

| Gate | Outcome |
| --- | --- |
| Actual isolated native executable startup | Observed authentication selector |
| Authentication | Required; not attempted |
| Root startup hook / exact transcript header / paginated lineage | Blocked before observation |
| Two turns, approval wait/allow, cancellation, root failure, child activity, explicit resume | Blocked/not run |
| Passive observer safety/disconnect; metric arithmetic/native reference | Blocked/not run |
| Native GUI/AX; Linux; capacity | Unrun |
| Attempt 02 recorded PID/group cleanup | Verified absent after owned SIGTERM/reap |
| Attempt 01 recorded PID/group cleanup | Unverified; PID not persisted |

The source-comparison findings remain unchanged. This attempt resolves the review's demand for a concrete native prerequisite, while withholding adapter implementation and capability certification. It does not turn unrun cases into passes.
