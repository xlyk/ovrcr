# The Bridge displays alerts; it does not decide them

macOS attributes an `osascript` notification to Script Editor, and a click on that banner cannot select the session. The Bridge is the signed macOS app that displays an alert the Dashboard has already accepted. The Dashboard remains the only decider of Ready and input-needed alerts. The Bridge does not own sessions or panes, and it does not create iTerm sessions.

A click asks the Server to tell the active Dashboard to select that session. That is not the existing terminal `Select` request, which attaches a terminal. If no Dashboard is attached, the click is dropped. A click does not mark the response reviewed. When the Dashboard is inside iTerm, the Bridge may select that existing iTerm session. Otherwise it may activate the parent terminal application. It does not color tabs, send input, or read the iTerm screen.

The Bridge is installed at a stable signed path. `just run` does not replace it. Linux continues to use `notify-send`. The Bridge has nothing to show until reporting produces an eligible alert. This decision does not build the app.

Rejected alternatives: keep `osascript`, because the sender stays Script Editor and there is no click callback; let iTerm own a tab per session, because that makes iTerm a second pane owner; let the Bridge decide which alerts to show, because those rules already live in the Dashboard.
