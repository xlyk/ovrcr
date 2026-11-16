---
name: ovrcr-session-care
description: Diagnose a stuck, waiting, exited or interrupted OVRCR session. Carry out explicitly requested pause, resume, stop, close, reopen or supported conversation recovery on the exact session run.
---

# Care for a session

## Installation

Bundled with OVRCR for Operator on macOS and Linux; no checkout or personal
skill installation is needed. Use the inherited `"$OVRCR_EXECUTABLE" --version`
and shared CLI guide, keeping the current instance configuration and socket.

## Diagnose first

1. Resolve the requested session from current metadata:

   ```sh
   "$OVRCR_EXECUTABLE" --json terminal list
   "$OVRCR_EXECUTABLE" --json project list
   "$OVRCR_EXECUTABLE" --json workspace list --project PROJECT
   ```

   Match its numeric `id` and `run`, Project, and registered Workspace id/path
   (`cwd` in the session row). A title, command echo, process cwd or later
   Dashboard selection is not identity. Ask for a target if missing or ambiguous;
   do not inspect unrelated conversations. State the scope and observed run.
2. Inspect `phase`, `activity`, `agent.health`, `reporting_unavailable`,
   `agent.input_requests`, `unread` and `recovery` together:

   | Observation | Diagnosis and next step |
   | --- | --- |
   | Healthy `busy` | Work in progress; report the observed work, not a hang inferred from silence. |
   | `waiting_input` / open Input requests | Human answer needed. Identify the question if visible; leave it unanswered. |
   | `response_ready` / Unread | A response is ready for review, not proof of task success. Leave Unread intact. |
   | Healthy `idle` | No current reported work; not proof that the task finished or ownership is safe. |
   | `paused` | Intentionally suspended; leave it paused. |
   | `exited`, `stopped`, `interrupted` | Report the actual phase and exit details; distinguish process exit from conversation recovery. |
   | Missing Agent data, `unknown`, unavailable or degraded Reporter health | Current activity is uncertain. Report any last observation as historical, not idle/completed. |
   | `recovery.requires_ack` | Prior process ownership is uncertain; a human must verify it. Inspection cannot acknowledge it. |
   | `recovery.unavailable` / `failure` | Report the recovery blocker. A retained conversation id alone does not establish resumability. |

   Terminal silence, an old or missing Last activity time, Ready, and a completion
   trailer cannot independently prove completion or safe ownership.
3. Read content only if it can explain the requested condition. Use the observed
   provider binding, not the session title or launch label:

   - Claude Code / Codex: `"$OVRCR_EXECUTABLE" --json terminal transcript ID`.
     This reads the retained conversation, not the newest file in a profile.
     A page is bounded; prose entries may be clipped at 16 KiB with `…`, and
     oversized records may be skipped. If needed, use
     `"$OVRCR_EXECUTABLE" --json terminal read ID --max-lines 40` for a terminal
     tail. If older entries are necessary, pass the returned `start` as `--before`
     together with the returned `identity` as `--identity` JSON; stop on a changed
     conversation refusal.
   - Pi / Oh My Pi: `"$OVRCR_EXECUTABLE" --json terminal read ID --max-lines 40`
     or `"$OVRCR_EXECUTABLE" terminal screen ID --rows 20 --cols 120`. Prefer text
     for screen inspection; styled-cell JSON can exceed the tool's output limit.
     These are bounded terminal observations, not a complete saved conversation.
   - Other or unknown providers: use the bounded terminal read if available;
     do not claim transcript or recovery support not exposed by OVRCR.

   Missing, empty, refused or truncated reads remain evidence gaps. A terminal
   tail cannot recover missing transcript text by assumption. Never search
   personal profiles, credentials or unrelated histories for a substitute.
4. Refresh `terminal list` after content reads. If the run, provider binding or
   retained conversation changed, discard attribution to the old run and report
   the change; do not combine observations from different runs. Screen reads
   alone do not carry an exact-run guarantee.
5. Finish with **Known**, **Unknown**, and **Next supported action**, identifying
   the instance, Project, Workspace, session id/run and relevant observations.
   Suggest waiting, reviewing the response, answering a visible Input request,
   or explicitly requesting supported control/recovery as appropriate. Human
   ownership verification and unsupported recovery stay explicit blockers.

## Read-only boundary and failures

A diagnosis alone leaves processes, Unread, Input requests and recovery
acknowledgements unchanged. A suggested next step is not authorization. A refused
inventory/read is a reported gap, never a reason to switch instances or clear a
guard. Finish without changing resources unless the user requested an action.

## Explicit control or recovery

1. Resolve the target as above and keep the requested numeric ID and observed RUN.
   A precise instruction authorizes only that action on that target. A precise
   close request needs no additional generic confirmation; a general cleanup
   request is a separate proposal, not permission to close anything.
2. Use the matching inherited executable and instance. Pass the captured run to
   the Server, rather than replacing it with a later run from another read:

   | Requested effect | Command |
   | --- | --- |
   | Pause owned processes | `"$OVRCR_EXECUTABLE" --json pause ID --expected-run RUN` |
   | Resume paused processes | `"$OVRCR_EXECUTABLE" --json resume ID --expected-run RUN` |
   | Stop processes, keep the active Retained record | `"$OVRCR_EXECUTABLE" --json kill ID --expected-run RUN` |
   | Close processes and archive the record | `"$OVRCR_EXECUTABLE" --json terminal close ID --expected-run RUN` |
   | Reopen a Terminal shell or resume a certified Agent conversation | `"$OVRCR_EXECUTABLE" --json terminal reopen ID --expected-run RUN` |

   Process resume is not Conversation resume. Agent reopen requires the certified
   retained Conversation reference and supported native recovery; it never opens
   a native picker or a different conversation. Terminal reopen starts its normal
   shell, not a replay of its original command. Generic control excludes Operator
   conversations; use the Operator's native controls instead.
3. Preserve refusals. Changed runs require a new decision for the replacement,
   not retrying without `--expected-run`. Uncertain/background ownership remains a
   blocker: never use `acknowledge-stopped`, `--ack-stopped` or force to unlock
   recovery. Preserve Dashboard input ownership and phase refusals; do not send
   Paste, Keystrokes or answers as a substitute for a refused control operation.
4. Refresh inventory (use `terminal list --archived` after close). Report the
   resulting phase/run, archive state and recovery evidence. Stop is complete
   only with successful controlled-stop evidence and resolved ownership, not
   merely process exit. Closing a cloud attachment does not cancel remote work
   or establish that charges stopped. A successful reopen starts a process;
   verify the new run's `recovery.attached` and matching conversation before
   claiming Conversation resume. Awaiting attachment is not success.
5. Report completed, refused and uncertain outcomes separately. On a lost response,
   reconcile the original session/run and current inventory before any retry; an
   unknown effect is not permission to repeat it against a successor. Failed
   recovery keeps the Retained record and evidence for a later explicit attempt.
   Never delete history, create a fallback conversation or replay an old prompt.
