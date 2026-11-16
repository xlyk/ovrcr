---
name: ovrcr-session-care
description: Diagnose an OVRCR Agent that appears stuck, is waiting for input, has unavailable reporting, or exited or was interrupted. Read-only session diagnosis with evidence and recovery blockers.
---

# Diagnose a session

## Installation

Bundled with OVRCR for Operator on macOS and Linux; no checkout or personal
skill installation is needed. Use the inherited `"$OVRCR_EXECUTABLE" --version`
and shared CLI guide, keeping the current instance configuration and socket.

## Usage

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
     or `"$OVRCR_EXECUTABLE" --json terminal screen ID`. These are bounded current
     terminal observations, not a complete saved conversation.
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

This procedure diagnoses only. Leave processes, Unread, Input requests and
recovery acknowledgements unchanged. Do not send Paste or Keystrokes, mark a
response reviewed, acknowledge stopped ownership, pause/resume/stop/reopen,
create a conversation, replay a prompt, or add watches. A next step is not an
operation completed. A refused inventory/read is a reported gap, never a reason
to switch instances or clear a guard. Finish without changing resources.
