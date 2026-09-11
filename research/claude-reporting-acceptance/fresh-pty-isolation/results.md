# Fresh PTY reporting environment isolation

Correction base: db8c829. The runtime fresh-PTY boundary now removes OVRCR_AGENT_SOCKET and OVRCR_AGENT_TOKEN alongside the three existing hook variables before assigning fresh runtime capabilities. This prevents a server started inside a managed native process from passing its outer private channel into unrelated new sessions.

The existing lifecycle test binary runs an isolated helper subprocess with stale outer variables; no global test-process environment is changed. That process starts the actual server fixture and creates a real PTY. Its shell runs the actual generic report CLI. Assertions require its own runtime session to become Busy and both inherited private variables to be absent. The shared spawn boundary covers explicit and autostart servers; separate autostart invocation is not claimed as executed here.

Commands all used `rtk proxy`. Real lifecycle tests used required socket/PTY execution permission.

| Log | Command | Exit/result |
| --- | --- | --- |
| 01-red.log | cargo test -p ovrcr --test server_lifecycle fresh_pty_does_not_inherit_outer_managed_reporting_channel -- --nocapture | 101, product RED: own session remained Unknown instead of Busy; 0 passed/1 failed outer test, isolated helper also failed. Reproduced before implementation; Task4 review correction was then completed separately. |
| 02-green.log | same | 0, 1 passed outer test, which requires successful isolated helper |
| 03-clippy.log | cargo clippy -p ovrcr-runtime -p ovrcr --all-targets --all-features -- -D warnings | 0; owning runtime and affected CLI compilation/lint |
| 04-fmt.log | cargo fmt --all -- --check | 0 |
| 05-diff.log | git diff --check | 0 |

No reporting mappings, private-channel behavior inside managed native processes, user servers, provider configuration, or metrics code changed.
