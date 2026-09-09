---
name: ovrcr-ux-pass
description: Explore OVRCR through its disposable native GUI, capture a bounded set of screenshots, and review or improve the observed UX. Use for screenshot reviews and iterative dashboard UX passes in this project.
---

# OVRCR UX pass

Use this repository's [computer-use guide](../../../docs/testing-computer-use.md) for launch, native input and cleanup details. Read the current checkout's `AGENTS.md`; it owns implementation, test and delivery rules.

## Choose the next useful slice

Read the latest review and identify the tested commit. Compare with current source before repeating a finding: an old screenshot can describe behavior already fixed upstream. Record checkout, branch, commit and working-tree state.

Default to 3–5 feature screenshots and about ten minutes of initial exploration. Let the user's requested count or scope override this default. Show findings before expanding the tour. A follow-up pass should cover states missing from the earlier review, such as empty results, validation, confirmations, task history or narrow windows.

## Capture the actual app

Run `rtk proxy just gui` from the reviewed checkout with the required execution permission, retaining its launcher session. Connect CUA to that checkout's absolute `target/OVRCR GUI.app` path. Read the current tool documentation; do not copy stale accessibility indices, coordinates or window geometry from a prior run.

Save screenshot bytes and full accessibility text together under distinct state/attempt filenames. Use the image's actual format (current native captures are JPEG); do not label JPEG bytes `.png`. Record the tested revision alongside them. Inspect the pixels as well as the text: text alone cannot prove contrast, clipping, cursor position or wide-glyph spacing.

If exercising terminal input, look for a separate marker output line, not the echoed command. Keep changes inside the disposable fixture. Record its root, socket, GUI/server PIDs and owned session process groups while alive. After closing via CUA, verify launcher exit and cleanup as described in the guide. Do not query the closed app: CUA may relaunch it.

## Turn observations into improvements

For each finding, state the user-visible problem, screenshot evidence, smallest useful correction and acceptance condition. Distinguish stable empty state from a request still loading. Inspect the owning renderer and request/response path before changing status text. Preserve errors, pending mutations and actual action semantics.

If implementation was requested, fix a small coherent set and recapture the same states. Add meaningful regressions through the owning entry point. When titles or footers move, find all acceptance waits using the old text: a negative title assertion can silently become ineffective. Establish that a form is visible before asserting dismissal; retain shell output, request, lifecycle and geometry assertions.

Keep the compact terminal design and existing ownership boundaries. Do not introduce a sidebar redesign, new configuration or dependencies merely to fill the next pass. Record hypotheses that did not justify a change.

## Review artifact and closeout

Write a short `review.md` with the tested revisions, linked screenshots/AX, prioritized findings, implemented outcomes, actual check results and remaining gaps. Keep original evidence and append dated follow-ups. Link failed attempts alongside corrected verification when they explain a defect.

Report native GUI evidence separately from automated tests. A screenshot or emitted OSC52 does not prove clipboard delivery, Linux rendering or real-provider restoration. Extend the existing task diary under the repository rules. Push, PR and merge only within the user's authorization.
