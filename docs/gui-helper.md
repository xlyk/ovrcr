# macOS GUI test helper

An optional development helper that runs the real dashboard inside a native macOS
window, against a disposable demo server. It exists so a human or a computer-use
agent can check rendering, input, and lifecycle behaviour that a headless test
cannot prove. Agents should follow the
[computer-use testing guide](testing-computer-use.md) for the smoke check and
cleanup evidence.

```sh
just gui
```

This builds `target/OVRCR GUI.app` and opens it. Each launch creates temporary
repositories, configuration, socket, and server with two projects, four
workspaces, and ten shell sessions. The agent and model labels and the example
output are demo fixtures; every session runs a local shell. The helper inherits
your shell environment while overriding the demo paths and terminal capabilities,
and uses an installed JetBrains Mono Nerd Font Mono when available, with a
monospace fallback.

## Using the window

Click the terminal to focus it. Select a session and press `Enter`, then type
commands or paste with `Cmd-V`. `Ctrl-g` returns to Browse mode, where sidebar
clicks select sessions and `q` detaches. `Cmd-K` opens the key popup anywhere in
the window and moves keyboard focus into it. Click **Restart dashboard** to
reattach to the same demo. Resizing the window resizes the selected session.

Computer-use tools can address the app as `dev.ovrcr.gui` or by its bundle path.
The accessibility tree exposes the terminal and its individual screen rows.

## Cleanup

Closing the window stops the demo server and sessions and removes the temporary
fixture. A cleanup failure reports the retained fixture path. Changes made in the
demo are disposable; the helper never opens your normal OVRCR instance.

## Building it

The GUI dependency is enabled only by the `gui` feature. Ordinary builds and
`cargo run` still use the CLI.

```sh
cargo test -p ovrcr --features gui --test gui
```
