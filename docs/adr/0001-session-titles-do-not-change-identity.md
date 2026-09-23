# Session titles do not change session identity

Users must be able to control and recognize the same terminal session. A title change must never retarget an operation or turn a session into a different resource.

The original decision allowed application-following Automatic titles alongside pinned manual titles. Frequent application updates made sessions harder to recognize. Issue #159 replaces that policy: all sessions ignore application titles and retain the user's creation name or the original generated name. Users can rename a session; clearing the manual title restores its original name.

Manual titles survive reconnect, server restart, and reopen. Previously saved application titles no longer affect display. This trades application-provided context for predictable user-controlled names; activity remains available through separate status indicators. Session identity remains unchanged.
