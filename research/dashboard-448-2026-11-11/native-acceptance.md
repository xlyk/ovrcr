# Native acceptance for #448

Sources: full walkthrough at `d10803a898a13407f283754bf9d929d6d69b66e3`, earlier missing-Pi check at `77ff96862e59fff518e223b6d23e572fbd955104`, and final targeted correction checks at `0ce4ca2c8f5b091eecb55eb4280925a26e8df6b7`. Captures 45–50 use the final code.

The actual `just gui` app ran from the integration checkout. Every launch used its own HOME, config, Server, repositories, workspaces, PTYs and socket. The installed Pi used only a private synthetic profile and the existing loopback provider acceptance helper. No paid inference or personal account was used. CUA supplied native input, screenshots and accessibility observations.

## Observed passes

- With an explicitly absent Pi executable, opening the Operator by mouse and keyboard retained Not set up and its setup guidance. Shell input produced `CUA_448_FIXED_SETUP_INPUT_OK`.
- With Pi available, the sidebar started Off, became Idle after opening, then Responding with elapsed time and activity. Its three rows omitted the harness and model.
- A 40-line thinking stream showed only lines 35–40 with the capped header. Thinking was peach and italic. Stop collapsed it. A completed 40-line block expanded by keyboard and collapsed by a body click without moving its header; expanding a second block left the first collapsed.
- The reasoning picker offered Default and only supported levels, and named unsupported levels. Medium changed to Default next turn while the turn remained held. Stop preserved Default, and the next turn reported the private Pi profile's Low default. A later local model change preserved stored Medium, reported actual Off beside it, and offered only Default and Off. Both Ctrl-t and clicking the header opened the picker.
- Closing the Operator before releasing its held local reply produced Reply ready with one unread reply and the latest text. Opening and reviewing the loaded tail cleared the unread state and showed Idle with the last reply's age.
- Usage showed all eight counters, honest zero reported tokens and Server uptime. A real private hook report changed input to 1 and unknown to 11. A fixture Agent's exit changed Exited to 1. CPU started unknown and then displayed measured bars; memory displayed used/total. The three sidebar settings hid/restored their sections while attention remained visible.
- Recent activity used project groups and two-line Agent rows. A real hook state change moved the same selected Agent to the top immediately. A needs-input title remained highlighted after selecting another row, and a folded project retained `?1`. The 80-column compact view was inspected. Recent activity persisted across Dashboard reattachment.
- Project and workspace hover controls opened the specified creation menus. Their forms retained row context. Changing Project scoped Branch; typing a new name offered Create, with default Main base and destination preview. Space expanded Advanced options and exposed editable Base. One empty-prompt launch created only a disposable branch/workspace; its shell stand-in exited without inference. The New terminal form retained its workspace.
- Mixed right/down splits reached four panes; a fifth was refused with its reason. A split at narrow size was refused below 20 columns. The chooser opened for new panes. Panes exposed directional focus, swap, resize, cycle, zoom and close. Directional focus, swap, Tab/Shift-Tab, zoom/unzoom, keyboard width/height changes and a focused shell marker (`CUA_448_PANE_INPUT_OK`) were observed. Right-click focused a pane and opened Panes. All three dividers of a nested layout dragged. Narrowing showed only the focused pane; widening restored the layout.
- Closing three panes left all twelve original fixture sessions running. Detaching the Dashboard and clicking Restart dashboard started one pane, retained Recent activity, and preserved all thirteen session IDs, runs and PIDs (twelve running, one exited).

## Final correction checks

At `0ce4ca2c`, a fresh available-Pi launch displayed Off before Open and Idle immediately after opening with no replies. The Panes menu displayed Shift-Tab / Previous pane; pressing it moved focus from the second pane to the first. With the pointer stationary on the selected auth workspace, creating an earlier-sorted disposable workspace caused a real HierarchyChanged update: `[+][x]` disappeared while the auth workspace remained selected. A separate explicitly missing-Pi launch remained Not set up after Open. No prompt or paid inference was sent in these final launches. Both exited 0; all recorded processes/groups, Server roots and copied Dashboard images were gone, then their ownership-checked private homes were removed.

## Limits

- CUA refused Terminal.app for safety reasons. No alternate desktop-control route was used. Native Terminal.app visual acceptance is unavailable; real PTY acceptance is separate evidence.
- CUA header double-click attempts did not produce zoom. Its held-key input produced one resize step, so native repeat delivery is unverified. No production threshold was changed for automation. The extended real-app PTY acceptance passed header double-click zoom/unzoom, divider double-click restoration, repeated resize and expected kernel sizes. The complete terminal acceptance suite passed 34 tests with 1 ignored.
- The disposable GUI's minimum window size prevents the shortest terminal layouts. Shorter section-collapse stages are covered by frame tests, not claimed as native passes.
- An earlier normal launch stopped exposing fresh CUA state after its reasoning checks. Its Server remained responsive, both launcher/provider exited successfully, and the entire fixture was cleaned. A fresh launch completed the remaining paths. The cause is unconfirmed; no application-deadlock claim is made.
- Hosted checks are tracked on the PR at its current head; local and native evidence do not substitute for hosted CI.

## Ownership and cleanup

The earlier normal launch made two loopback requests; the completed normal launch made three. Both manual provider helpers executed exactly one selected helper test and exited 0; both GUI launchers exited 0. Initial and reattached recorded processes/groups, Server roots, copied Dashboard images and ownership-checked private HOME directories were verified gone. Private auth, provider request payloads and profiles were not retained in public evidence. The original checkout, live user sessions and unrelated worktrees were untouched.
