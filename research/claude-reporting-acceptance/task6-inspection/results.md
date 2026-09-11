# CLI usage inspection

Adds `ovrcr session usage SESSION_ID` JSON and matching fields to terminal inventory: managed binding/activity/metrics/health, reservation epoch, unavailable state and independent component ages. Optional values stay null; cost estimate and scope remain independent of partial token scope. Existing context inspection stays compatible and uses the same session lookup. Neither command starts a server.

Tests run against the working tree based on697149d plus inspection and concurrent independently scoped collector/integration work. The inspection diff is independent of those new routes; final assembled regression remains required.

- Attempt01: regression failed because session usage was absent (CLI exit2).
- Attempt02: fixture accepted socket inherited nonblocking mode; corrected explicitly. This was a fixture failure, not a product RED.
- Attempt03: concurrent receiver implementation was temporarily incomplete; compiler failure, not behavioral evidence.
- Attempt04: focused usage inspection passed1 test.
- Attempt05: expanded same test through both actual CLI usage and terminal-inventory routes for missing cost, explicit estimated zero with different scope, and unbound null values. Full resource_cli suite passed6, including existing real PTY/context/legacy reporting cases.
- Attempt06: root binary/resource CLI all-feature Clippy passed with warnings denied.

The inspection test uses a typed socket fixture to isolate CLI serialization. It does not prove native provider publication; managed native acceptance is separate.
