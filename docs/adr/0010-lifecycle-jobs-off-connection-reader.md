# Lifecycle jobs run off the Dashboard connection reader

Decided 2026-10-06, from issue #297 grilling (shape A).

Create workspace, create session, remove workspace, and close terminal used to run to completion inside `handle_request_with_id` on the Dashboard connection reader. While that work held the reader (git, process stop, spawn), the Dashboard could not read the next frame, so Input and SetView froze for the duration.

The Server now accepts those four requests immediately when the caller is the Dashboard, runs at most one Lifecycle job on a dedicated worker under the existing `mutation_lock`, and publishes a typed `LifecycleCompleted` event correlated by the accepting request's client token, plus `HierarchyChanged` as today. A second Lifecycle job while one runs is refused with a soft Conflict. Esc dismisses only the Dashboard status strip; there is no cancel in v1. The Dashboard may show a Provisional row (`Creating…` / `Removing…`) and a failed sticky row; it must not invent a SessionId. CLI-only RemoveSession and DeleteArchived stay synchronous.

Rejected: client-only optimism without a Server job (reconnect and multi-client truth would diverge); Esc-abort; a jobs tray; a soft progress card.
