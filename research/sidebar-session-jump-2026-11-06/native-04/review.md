# Control and single-digit user trial

Tested source: `6095a236f6b1e92e699abec8208f0af58bfb45c2`, branch `codex/sidebar-session-jump-integration`, through the actual `just gui` launcher from this checkout. Product source was clean; the task-owned target symlink was untracked. The fixture used private HOME/config/socket/workspaces, six ordinary terminals and ten Agent-kind local shell stand-ins. No provider turn was started.

Kyle tested the updated GUI directly after requesting Control instead of Cmd and single digits only. Instructions were to hold Control, press a displayed `1`–`9` digit, and release Control before typing. His result: **“it works great”**. This is user acceptance of the revised native interaction; it is not a separate claim that he exercised every cancellation, retained-session or Operator permutation. Those contracts have focused application-path regression coverage.

Final accessibility observation showed `Control test 08` selected in the sidebar and pane, PID 12086, `CONTROL_FIXTURE_READYsh-3.2$` in the terminal, and Terminal mode in the footer. Screenshots were omitted at Kyle's explicit request. No held-image or other-terminal compatibility claim is made.

The native close button ended the GUI normally. Launcher exited 0; the private fixture root/socket disappeared; all 16 recorded session process groups returned ProcessLookupError. See [launcher metadata](launcher.json), [fixture](fixture.json), [owned groups](owned-groups.json), [cleanup receipt](cleanup.json) and [binary provenance](binaries.json). An initial Cmd-W left the window open; the native close button completed cleanup.

Full revised-source workspace checks and independent follow-up reviews are recorded in PR #406. Main remains unmerged.
