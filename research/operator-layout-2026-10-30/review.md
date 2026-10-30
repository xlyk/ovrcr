# Native Operator layout review

Checkout: `/Users/xlyk/.codex/worktrees/operator-layout-design/ovrcr`, branch
`codex/operator-layout-design`. The first frozen bundle used `299c2890`; the
current captures use `abd4793993c1b3d38951309c5e93ece078b0a42e` after the reviewed
style correction `479ceb27`. Neither native run used personal credentials.

## Observed fixes

The first run showed an invisible title-bar Ctrl+O control: its accessibility
text existed, while mauve foreground inherited a mauve header background.
The Operator section heading also used mauve instead of Projects' blue.
[Original pixels](native-03-quick-multiline.jpg) and
[paired AX](native-03-quick-multiline.ax.txt.gz) retain that failure.
The separate product correction sets the control background and heading color.
[Current wide view](native2-01-wide-original.jpg) and
[paired AX](native2-01-wide-original.ax.txt.gz) show both corrections.
Clicking the now-readable title-bar control opened the real quick editor.

## Current native outcomes

The wide Original layout keeps normal-weight, one-line conversations above
Projects. Selected rows have the mauve leading marker and background; the
[60-column cancellation view](native2-06-cancel-hover.jpg) shows hover `[x]`
and the preserved live A turn. Close confirmation identifies the exact run;
Escape cancels, and confirmation archives only A. The normal
[archived-session picker](native2-07-archive.jpg) restores its
[interrupted history](native2-08-restored-history.jpg).

[80×24](native2-02-80x24-quick.jpg), [native 60×24](native2-03-60x24-quick.jpg)
and [nested 60×16](native2-04-nested-60x16.jpg) keep multiline Unicode input and
send/dismiss instructions visible. The nested PTY records kernel size `16 60`
and original Dashboard/shutdown exits 0. Standard Cmd+plus zoom reaches the native
60-column width without a source change. Ctrl+O opens above a close confirmation,
and Escape restores that confirmation before cancellation.

Both provider holders record A=2, B=2, quick=1. Each A/B turn executed real Bash
and the matching frozen CLI before its final stream. Both GUI and holder original
waits are 0. The cleanup JSON files record every captured group's kernel absence,
Demo/nested owner absence and removed sockets/roots. The third quick-only run records the actual quick worker PID/PGID while gated,
then proves its exact group absent after completion. Its GUI/holder original waits
are also 0 and all 13 recorded groups are absent.

Native fast receipts beat Escape in the first focus attempt. The optional third-run
SQLite gate instead produced an explicit database-lock rejection; that attempt
does not qualify delayed successful receipt behavior. A known-failure manual retry
created one conversation and inference, while another terminal remained selected
and printed `NATIVE3_TERMINAL_OK` through background completion. The deterministic
real-socket test proves delayed-receipt focus behavior; no delayed native focus
claim is made. See [the acceptance record](README.md) for command failures,
executed counts and the still-pending hosted/platform delivery gates.

## Lossless archive packaging

Raw AX and command logs are stored as deterministic `.gz` files. The
[manifest](raw-artifacts-manifest.json) maps original logical filenames and SHA256
to stored paths and hashes. All original command/metadata JSON remains unchanged.
The [acceptance record](README.md#raw-capture-archive-format) gives a standard-library
decompression and hash-check command. The original raw-artifact whitespace failure
is retained; conversion does not change any native or application-test outcome.
