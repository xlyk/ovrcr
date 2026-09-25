# Session titles do not change session identity

The identity rule stands. ADR 0005 supersedes the display rule for Agent launches: a conversation subject may replace the displayed name until the user renames the session. Application titles remain ignored. A title change must still never retarget an operation or turn a session into a different resource.

Users must be able to control and recognize the same terminal session.

The original decision allowed application-following Automatic titles alongside pinned manual titles. Frequent application updates made sessions harder to recognize. Issue #159 replaced that policy: all sessions ignore application titles. Users can rename a session; clearing the manual title restores its original name. ADR 0005 later allows a conversation subject to replace the displayed name on an Agent launch. The identity rule is unchanged.

Manual titles survive reconnect, server restart, and reopen. Previously saved application titles no longer affect display. This trades application-provided context for predictable user-controlled names; activity remains available through separate status indicators. Session identity remains unchanged.
