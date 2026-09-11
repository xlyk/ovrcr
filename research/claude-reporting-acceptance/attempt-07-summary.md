# Native compaction and branch capture

Claude2.1.267, isolated authorized subscription fixture. /compact completed: SessionStart(source=compact) and PostCompact retained the original conversation ID. /branch completed in the foreground with a new conversation ID and SessionStart(source=fork). No programmatic foreground-admission guarantee is inferred from the UI observation alone.

Background /fork was refused by Claude because the copy would not inherit the fixture launch restrictions. Restrictions were not removed to bypass this denial. Background fork remains unverified, not unsupported.

/exit returned0; the temporary credential copy was removed. The hook stream contains prior attempts and must not be treated as one causally ordered launch.
