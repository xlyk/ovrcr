# Queue and buffer inventory

| Boundary | Existing bound | Retained-state evidence |
| --- | --- | --- |
| Raw PTY events | 64 entries | Real startup `sync_channel`; output payload is one 8,192-byte PTY read. Diagnostics count queued payload bytes, not heap allocation. Blocking producers preserve output and exit ordering. |
| Dispatcher | 64 entries | Real startup `sync_channel`; report/command admission uses `try_send` and returns `Conflict` on full. Lifecycle and bridged events retain blocking delivery. Diagnostics count serialized `AgentReport` payload bytes and raw session-output bytes; completion channels and enum/container heap overhead are outside that byte count. |
| Dashboard | 64 ordinary messages | The sink separately retains up to one terminal frame and dirty markers for the current view. Protocol validation limits the view to two panes; view replacement clears dirty markers. Snapshot tests count serialized message, terminal, and dirty-frame bytes separately. |
| Collector IPC | One request/response exchange | Request and response bodies are each limited to 65,536 bytes plus a four-byte frame. The controller charges the complete retained request vector until its full frame is written, then clears it before reading the response. Response reads are capped to the remaining frame allowance; trailing bytes are probed on the stack and rejected. Counts exclude vector capacity and kernel pipe buffers. The 256KiB scan chunk and transcript accounting limits apply only to collector file processing. |
| Private callback accepted stream | One synchronously processed stream | Authentication is 65 bytes; request and response are each limited to 65,536 bytes. Read authentication/payload shares the existing 200ms absolute deadline; handler work keeps the existing 800ms deadline. |
| Private callback kernel backlog | `UnixListener::bind` calls `listen(SOMAXCONN)` on the checked Rust 1.98 standard library | macOS reports `kern.ipc.somaxconn: 128`. This is kernel-buffered and is not included in application queue item/byte counters. Linux effective backlog remains a coordinator-owned host check. |

These counts prove application admission limits and retained payload bytes. They
do not claim exact heap use, socket send/receive buffer use, allocator overhead,
or total process RSS. The instrumented 50-session acceptance test records RSS
and these queue counters as separate evidence.

For diagnostics-enabled runtime channels, each successful standard-library
channel operation and its item/byte counter update are serialized by one
instrumentation mutex. Blocking calls retry actual channel operations after
wakeup; counters do not grant admission. The default runtime delegates directly
to the standard channel and does not calculate payload weights.
