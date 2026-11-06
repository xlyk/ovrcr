# Native GUI red checkpoint

Reviewed source: `2ae54eee3c65d7faa11c734d65774c41e698ed34`, checkout `/private/tmp/ovrcr-401-integration`, launched through `just gui`.

The GUI starts a private Server, local shells and six Agent-kind shell stand-ins. Declined optional status integration; no provider turn or personal-profile write. Disabled quota collection only inside the disposable fixture.

- `00-idle.jpg` / `.ax.txt`: actual Dashboard, original local shell #1 selected.
- `01-cmd-held.jpg` / `.ax.txt`: retained unsupported CUA bare-super attempt; not a successful hold.
- `02-cmd-titlebar-held.jpg` / `.ax.txt`: modifier-held titlebar click; no labels observed. Event delivery/timing unproven.
- `03-native-cmd1-no-selection.jpg` / `.ax.txt`: Cmd+1 leaves original shell selected. Existing Cmd+K did open Commands; Escape closed it. Repeated from Browse and Terminal, without a successful selection.

Meaningful acceptance remains red; automated protocol tests cannot substitute for this native result. GUI adapter correction and recapture required.

Cleanup: CUA closed native window; retained launcher exited 0. Private root removed; all 12 recorded owned process groups absent (ProcessLookupError), recorded in owned-groups.json. No resources remain from this attempt.
