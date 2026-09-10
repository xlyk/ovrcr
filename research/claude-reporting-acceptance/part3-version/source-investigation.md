# Claude Code Part 3 source investigation: Tasks 3–7

Reviewed 2026-09-10. This is a source-only checkpoint for the [Part 3 plan](../../../plans/2026-09-10-claude-code-part3.md), compared with the existing [capability record](../../../research/claude-reporting-acceptance/provider-capabilities.md). No authenticated provider session or configuration write was performed. Current Anthropic documentation is rolling documentation, so each candidate still needs exact-version capture before implementation.

## Exact CLI baseline

The retained `/Users/xlyk/.local/share/claude/versions/2.1.267 --help` and default `/Users/xlyk/.local/share/claude/versions/2.1.268 --help` outputs were compared byte for byte and are identical. The saved outputs are [2.1.267 help](../../../research/claude-reporting-acceptance/provider-discovery/claude-2.1.267-help.txt) and [2.1.268 help](../../../research/claude-reporting-acceptance/part3-version/2.1.268-help.txt). Both expose `-c, --continue`, optional-value `-r, --resume [value]`, `--fork-session`, `--from-pr [value]`, `--name`, and background sessions. Neither help output defines a foreground transition signal, accounting export schema, final-source boundary, or post-decision completion event.

## Candidate matrix

| Part 3 gate | One candidate | Retained evidence compared | Targeted capture or precise absence | Source verdict |
| --- | --- | --- | --- | --- |
| Task 3: additional launch form | Separate-token `-r <canonical-lowercase-UUIDv4>`. The short flag is the documented alias of `--resume`, and the UUID is available in argv before spawn. | The [resume certification](../../../research/claude-reporting-acceptance/native-resume-discovery/results.md) proves only separate-token `--resume <uuid>` on 2.1.267 and explicitly excludes `-r`. The two exact help outputs describe the same alias. Anthropic's [CLI reference](https://code.claude.com/docs/en/cli-usage) gives `claude -r "<session>"`, while [session management](https://code.claude.com/docs/en/sessions) says a resume value may be an ID or name. | Capture two known conversations and invoke the intended one as exactly `-r UUID`; retain argv, root `SessionStart(source=resume)`, exact matching `session_id`/transcript path, pre-existing history, missing UUID behavior, a foreign/child callback before the root callback, exit, and cleanup. Accept only if it matches the already certified long-form contract without rewriting argv. | Promising and narrowly testable. `--continue`, picker/search/name, `--from-pr`, and `--fork-session` still lack an exact expected resulting UUID before admission. |
| Task 4: in-process foreground intent | The root pair seen for foreground `/branch`: `SessionEnd(reason=resume, prompt_id=P, old=A)` followed by `SessionStart(source=fork, prompt_id=P, new=B)`. The first event precedes the new binding and could be a transition token if it is exclusive to the foreground root. | [Attempt 07](../../../research/claude-reporting-acceptance/attempt-07-summary.md) observed foreground branch and the retained [hook stream](../../../research/claude-reporting-acceptance/attempt-07-live-hooks.jsonl) contains that paired identity. Anthropic's [session guide](https://code.claude.com/docs/en/sessions) says `/branch` switches the running process and prints the new and original IDs. The current [hook reference](https://code.claude.com/docs/en/hooks) documents only `startup`, `resume`, `clear`, and `compact` SessionStart sources; it does not document `source=fork` or this SessionEnd reason. | In one exact-version fixture, record ordered root and child hooks for `A -> /branch B -> /resume A`, a cancelled/failed switch, and a background `/subtask` or fork while A remains foreground. Also retain status-line/current UI identity. The candidate qualifies only if every foreground switch emits the old-root SessionEnd first with a shared unambiguous token and background work never emits that root pair. | Worth one bounded matrix. It remains disabled because the candidate fields are undocumented and background exclusivity was not captured; `SessionStart` alone cannot select foreground ownership. |
| Tasks 5–6: complete categories and final accounting | OpenTelemetry `claude_code.token.usage` is the best category source. Anthropic says the internal counter increments after each API request, labels token type as input/output/cacheRead/cacheCreation, labels `query_source` as main/subagent/auxiliary, and carries `session.id`. It says a streaming response contributes once even when a gateway sends usage in several frames; it does not promise one exported datapoint per request. | Existing collection reads matching non-sidechain assistant rows from one root transcript and remains Partial. [Attempt 10](../../../research/claude-reporting-acceptance/attempt-10-sources/summary.md) proves equal duplicate rows, not auxiliary/child coverage or corrections. The native [`/usage` comparison](../../../research/claude-reporting-acceptance/native-remaining/results.md) showed broader model/category totals and a subagent share. Anthropic's [monitoring reference](https://code.claude.com/docs/en/monitoring-usage) is the first reviewed source that explicitly names all three query sources; the [cost guide](https://code.claude.com/docs/en/costs) says `/usage` and status-line cost are session estimates and reset on clear. | Use process-local telemetry settings and a task-owned local collector. Capture fresh, resume, compact, one root request, one child request, one known auxiliary request, retry/API-failure behavior, and both delta and cumulative exports. Compare exported aggregates with `/usage`, status-line cost, and recognized transcript rows. Determine whether resume exports history or only new process requests, and whether per-session counters survive fork/clear with the documented scopes. | Promising for category coverage only. The docs specify a 60 s default export interval and delta/cumulative temporality but no metric request identity, correction revision, final datapoint, shutdown flush, exporter acknowledgement, or ordering against Stop/SessionEnd/native exit. No reviewed source supplies Task 6 finality; even a successful native exit observation cannot turn this absence into a guarantee. |
| Task 7: post-decision completion | Root `Notification(notification_type=idle_prompt, prompt_id=P)` for the current conversation. The hook cannot block, and Anthropic now documents it as arriving about 60 seconds after Claude finishes responding when the user has not typed. | The [native remaining run](../../../research/claude-reporting-acceptance/native-remaining/results.md) retained matching ordinary-turn idle notifications. Its blocking-Stop path emitted `Stop(false)`, more tool work, then `Stop(true)` for the same prompt; later user input cancelled the chance to observe that prompt's idle timer. [Attempt 06](../../../research/claude-reporting-acceptance/attempt-06-summary.md) found private `turn_duration` rows after successful responses but none for an interrupted prompt. The [hook reference](https://code.claude.com/docs/en/hooks) documents idle timing and also confirms that Stop hooks may continue work. | Run `Stop(A) -> blocked continuation -> work(A) -> final Stop(A)`, then provide no input for at least 70 seconds. Require no idle notification for the provisional Stop and exactly one matching root idle notification after the final Stop. In a second case, start B near A's timer and require no stale A promotion; cover cancellation, denial, child completion/failure, and tool continuation without interpreting silence as completion. | Strongest post-decision candidate. It can support Confirmed only when the matching notification is actually observed; typing cancels it, so many completed turns will remain Observed. Certification still needs the adverse ordering matrix above. |

## Local-only OpenTelemetry discovery recipe for 2.1.268

Use OTLP over HTTP/JSON for the bounded capture. The current [monitoring reference](https://code.claude.com/docs/en/monitoring-usage) officially supports `OTEL_METRICS_EXPORTER=otlp`, `OTEL_EXPORTER_OTLP_METRICS_PROTOCOL=http/json`, and a signal-specific endpoint. It also supports a Prometheus text endpoint at `http://localhost:9464/metrics`, but the documented fixed port is awkward for concurrent sessions and the docs do not state a bind-address or port override. The console exporter is supported but its output format is not specified and it would mix discovery data with the native PTY. Neither is the primary recipe.

Run the receiver below from a task-owned directory. It binds an ephemeral loopback port, accepts at most 2 MiB per request, never persists the raw OTLP body, hashes `session.id`, drops resource identity and high-cardinality labels, and writes only three allowlisted metrics. The exact 2.1.268 capture can copy this code into its evidence directory without adding it to production source.

```sh
OVRCR_OTEL_ROOT="$(rtk mktemp -d /private/tmp/ovrcr-claude-otel.XXXXXX)"
rtk proxy python3 - "$OVRCR_OTEL_ROOT" <<'PY' &
import hashlib, json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

root = Path(sys.argv[1])
keep_metrics = {
    "claude_code.session.count",
    "claude_code.token.usage",
    "claude_code.cost.usage",
}
keep_attributes = {
    "session.id", "app.version", "app.entrypoint", "start_type",
    "type", "model", "query_source", "speed", "effort",
}

def scalar(value):
    for key in ("stringValue", "intValue", "doubleValue", "boolValue"):
        if key in value:
            return value[key]
    return None

def attributes(items):
    out = {}
    for item in items:
        key = item.get("key")
        if key not in keep_attributes:
            continue
        value = scalar(item.get("value", {}))
        if key == "session.id" and isinstance(value, str):
            value = hashlib.sha256(value.encode()).hexdigest()[:12]
        out[key] = value
    return out

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        if self.path != "/v1/metrics" or length > 2 * 1024 * 1024:
            self.send_error(404 if self.path != "/v1/metrics" else 413)
            return
        try:
            document = json.loads(self.rfile.read(length))
            rows = []
            for resource in document.get("resourceMetrics", []):
                for scope in resource.get("scopeMetrics", []):
                    for metric in scope.get("metrics", []):
                        if metric.get("name") not in keep_metrics:
                            continue
                        for point in metric.get("sum", {}).get("dataPoints", []):
                            rows.append({
                                "metric": metric["name"],
                                "attributes": attributes(point.get("attributes", [])),
                                "startTimeUnixNano": point.get("startTimeUnixNano"),
                                "timeUnixNano": point.get("timeUnixNano"),
                                "asInt": point.get("asInt"),
                                "asDouble": point.get("asDouble"),
                            })
            with (root / "sanitized-metrics.jsonl").open("a") as output:
                for row in rows:
                    output.write(json.dumps(row, sort_keys=True) + "\n")
            body = b"{}"
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except Exception:
            self.send_error(400)

    def log_message(self, *_):
        pass

server = HTTPServer(("127.0.0.1", 0), Handler)
(root / "port").write_text(str(server.server_port))
server.serve_forever()
PY
OVRCR_OTEL_RECEIVER_PID=$!
```

After `$OVRCR_OTEL_ROOT/port` appears, pass the following map only to the supervised Claude process. Remove inherited `OTEL_*`, `BETA_TRACING_ENDPOINT`, and `CLAUDE_CODE_ENHANCED_TELEMETRY_BETA` variables before applying it; do not export the map in the parent shell. Keep logs and traces off, omit every OTLP header/certificate variable, and set no `OTEL_RESOURCE_ATTRIBUTES`:

```text
CLAUDE_CODE_ENABLE_TELEMETRY=1
OTEL_METRICS_EXPORTER=otlp
OTEL_LOGS_EXPORTER=none
OTEL_EXPORTER_OTLP_METRICS_PROTOCOL=http/json
OTEL_EXPORTER_OTLP_METRICS_ENDPOINT=http://127.0.0.1:${OVRCR_OTEL_PORT}/v1/metrics
OTEL_METRIC_EXPORT_INTERVAL=1000
OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE=delta
OTEL_METRICS_INCLUDE_SESSION_ID=true
OTEL_METRICS_INCLUDE_VERSION=true
OTEL_METRICS_INCLUDE_ACCOUNT_UUID=false
OTEL_METRICS_INCLUDE_ENTRYPOINT=true
OTEL_METRICS_INCLUDE_RESOURCE_ATTRIBUTES=false
```

Use `/Users/xlyk/.local/share/claude/versions/2.1.268` as the preserved provider argv under the existing OVRCR supervisor. Stop the receiver after the native process and final bounded observation, wait for its PID, verify it is absent, and retain the task-owned port, sanitized JSONL, command, exit, and cleanup record. A second isolated run may change only `OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE` to `cumulative` when needed to distinguish exporter aggregation from provider accounting. Do not treat repeated cumulative exports as repeated usage.

Default privacy needs an active filter even with metrics only. Current docs say `session.id` and account UUID/account ID are included by default; version and entrypoint are excluded by default; resource attributes are included by default. They also say `organization.id`, the installation-scoped `user.id`, signed-in `user.email`, and detected `terminal.type` are always included when available. `prompt.id` is deliberately excluded from metrics for cardinality, so metrics have no turn or request identity. Content logging is disabled by default, and this recipe disables the logs exporter and beta traces entirely. The receiver drops every identity except a hashed session ID before writing evidence.

This evidence capture needs no production dependency. This checkout has Python 3.13.7, `curl`, and `jq`; the standard-library receiver above is sufficient. The Rust workspace has `serde_json` but no HTTP, OTLP, OpenTelemetry, Prometheus, or protobuf transport dependency. A production OTLP receiver therefore needs a separately reviewed transport/helper design or a new dependency; source discovery alone does not establish a zero-dependency product path.

## Result

Task 3 has one low-risk candidate (`-r UUID`). Task 4 has one version-specific candidate pair that needs foreground/background discrimination. Task 5 has a materially better accounting source in OpenTelemetry, but Task 6 still has no provider finality primitive. Task 7 has a documented, already observed delayed candidate whose blocking-Stop and stale-turn behavior remain the deciding tests. No source reviewed here changes the existing supported contract before those captures pass.
