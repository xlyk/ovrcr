# Retained verification attempts

Both failures below were acceptance-driver assumptions, not failing application assertions. No application source or existing test expectation was changed.

- Synthetic setup initially asserted `kind == "agent"`. The real CLI encodes Agent launch kind as `{"agent":"codex"}`. All 38 sessions were created successfully before that assertion. The read-only inventory then asserted the exact actual JSON representation, 50 Running sessions, and 38 Agent launches. The original setup was not rerun or allowed to duplicate sessions.
- Compact-size inventory initially asserted 72 columns from a screenshot estimate. Kernel `stty -f /dev/ttys049 size` returned `22 73`. The corrected read-only measurement confirmed **73×22**, and recorded the replacement Dashboard PID/PGID. No terminal dimensions or application assertions were changed.
- Committed-diff whitespace checking identified literal trailing terminal-cell spaces in the raw AX captures and the cargo log's final blank line. Exact raw bytes are now gzip-compressed rather than stripped; decompression was verified byte-for-byte against the originals. No whitespace rule or application assertion was disabled.
- CUA returned `Computer Use is not allowed to use the app` for both `com.googlecode.iterm2` and `com.apple.Terminal`. Neither app was controlled or used to reroute around the denial. The GUI helper minimum remains unchanged.
- Docker context inventory found `desktop-linux`, but its socket read was permission denied. No Docker server/container was started, and no container or hosted CI result is claimed as native Linux terminal evidence.

Raw original driver scripts, failed tool outputs, GUI build log and GitHub issue/PR snapshots remain in `/Users/xlyk/Documents/Codex/2026-09-30/task-6/` on the test host.
