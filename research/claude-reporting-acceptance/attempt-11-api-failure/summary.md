# Attempt11: native model failure

Native Claude Code2.1.267 launched in the disposable source fixture with an explicitly invalid model name, synchronous capture hooks and the previously authorized isolated subscription session. No policy or live user-settings changes. Tool session32079, native PID/PGID17253.

After one short prompt, native UI reported the selected model unavailable. Capture contains UserPromptSubmit followed by StopFailure(error=model_not_found) with the same conversation and prompt ID, and no agent_id. A later Notification(idle_prompt) is retained as a generic observation; it does not erase the error. Normal /exit completed0, and recorded PID/PGID were verified absent.

This supports the typed native model-not-found failure mapping. It does not certify every network, authentication or provider error, or claim an independently captured HTTP exchange. Prompt/transcript bodies and credentials are excluded; identity aliases are consistent within the capture.
