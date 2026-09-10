#!/usr/bin/env python3
"""Local-only, sanitized OTLP/HTTP JSON metrics capture for OVRCR research."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import Any


MAX_BODY_BYTES = 2 * 1024 * 1024
MAX_REQUESTS = 512
MAX_POINTS = 8192
MAX_ATTRIBUTE_STRING_BYTES = 256
MAX_REJECTED_VALUE_BYTES = 64
MAX_REJECTED_VALUE_BUCKETS = 16
METRICS = {
    "claude_code.session.count",
    "claude_code.token.usage",
    "claude_code.cost.usage",
}
ATTRIBUTES = {
    "session.id",
    "app.version",
    "app.entrypoint",
    "start_type",
    "type",
    "model",
    "query_source",
    "speed",
    "effort",
}
STANDARD_ATTRIBUTES = {"session.id", "app.version", "app.entrypoint"}
METRIC_ATTRIBUTES = {
    "claude_code.session.count": STANDARD_ATTRIBUTES | {"start_type"},
    "claude_code.token.usage": STANDARD_ATTRIBUTES
    | {"type", "model", "query_source", "speed", "effort"},
    "claude_code.cost.usage": STANDARD_ATTRIBUTES
    | {"model", "query_source", "speed", "effort"},
}
TOKEN_TYPES = {"input", "output", "cacheRead", "cacheCreation"}
QUERY_SOURCES = {"main", "subagent", "auxiliary"}


def scalar(value: Any) -> str | int | float | bool | None:
    if not isinstance(value, dict):
        return None
    for key in ("stringValue", "intValue", "doubleValue", "boolValue"):
        candidate = value.get(key)
        if isinstance(candidate, str):
            if len(candidate.encode("utf-8")) <= MAX_ATTRIBUTE_STRING_BYTES:
                return candidate
        elif isinstance(candidate, (int, float, bool)):
            return candidate
    return None


def selected_attributes(*groups: Any) -> dict[str, str | int | float | bool | None]:
    selected: dict[str, str | int | float | bool | None] = {}
    for group in groups:
        if not isinstance(group, list):
            continue
        for item in group:
            if not isinstance(item, dict):
                continue
            key = item.get("key")
            if key in ATTRIBUTES:
                selected[key] = scalar(item.get("value"))
    session_id = selected.get("session.id")
    if isinstance(session_id, str) and session_id:
        selected["session.id"] = hashlib.sha256(
            b"ovrcr-part3-otel\0" + session_id.encode("utf-8")
        ).hexdigest()
    else:
        selected.pop("session.id", None)
    return selected


def numeric_value(point: dict[str, Any]) -> int | float | None:
    present = [key for key in ("asInt", "asDouble") if key in point]
    if len(present) != 1:
        return None
    value = point[present[0]]
    if present[0] == "asInt":
        if isinstance(value, bool):
            return None
        try:
            return int(value)
        except (TypeError, ValueError):
            return None
    if isinstance(value, bool):
        return None
    try:
        parsed = float(value)
    except (TypeError, ValueError):
        return None
    return parsed if math.isfinite(parsed) else None


def rejection_bucket(value: Any) -> str:
    """Distinguish bad enum values without persisting their raw contents."""
    if value is None:
        return "<missing>"
    if not isinstance(value, str):
        return "<non-string>"
    encoded = value.encode("utf-8")
    if len(encoded) > MAX_REJECTED_VALUE_BYTES:
        return "<too-long>"
    digest = hashlib.sha256(b"ovrcr-part3-rejected-enum\0" + encoded).hexdigest()
    return f"sha256:{digest[:16]}"


def sum_metadata(sum_value: dict[str, Any]) -> tuple[str | int | None, bool | None]:
    temporality = sum_value.get("aggregationTemporality")
    if isinstance(temporality, bool) or not isinstance(temporality, (str, int)):
        temporality = None
    elif isinstance(temporality, str) and len(temporality.encode("utf-8")) > 64:
        temporality = None
    monotonic = sum_value.get("isMonotonic")
    if not isinstance(monotonic, bool):
        monotonic = None
    return temporality, monotonic


class Capture:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.rows_path = root / "sanitized-metrics.jsonl"
        self.stats_path = root / "receiver-stats.json"
        self.stats: dict[str, Any] = {
            "requests_total": 0,
            "accepted_requests": 0,
            "rejected_bad_path": 0,
            "rejected_missing_length": 0,
            "rejected_oversized": 0,
            "rejected_malformed_json": 0,
            "rejected_schema": 0,
            "rejected_capacity_requests": 0,
            "accepted_points": 0,
            "rejected_points": 0,
            "rejected_capacity_points": 0,
            "ignored_metrics": 0,
            "ignored_non_sum_metrics": 0,
            "empty_allowlisted_requests": 0,
            "rejected_token_type_values": {},
            "rejected_query_source_values": {},
            "last_request_unix_ns": 0,
        }
        self.write_stats()

    def increment(self, key: str, amount: int = 1) -> None:
        self.stats[key] += amount

    def mark_request(self) -> None:
        self.increment("requests_total")
        self.stats["last_request_unix_ns"] = time.time_ns()

    def record_rejected_value(self, key: str, value: Any) -> None:
        counts = self.stats[key]
        bucket = rejection_bucket(value)
        if bucket not in counts and len(counts) >= MAX_REJECTED_VALUE_BUCKETS:
            bucket = "<other>"
        counts[bucket] = counts.get(bucket, 0) + 1

    def write_stats(self) -> None:
        temporary = self.stats_path.with_suffix(".json.tmp")
        temporary.write_text(json.dumps(self.stats, indent=2, sort_keys=True) + "\n")
        os.replace(temporary, self.stats_path)

    def append_rows(self, rows: list[dict[str, Any]]) -> None:
        if not rows:
            return
        with self.rows_path.open("a") as output:
            for row in rows:
                output.write(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n")

    def parse(self, body: bytes) -> list[dict[str, Any]] | None:
        try:
            document = json.loads(body)
        except (UnicodeDecodeError, json.JSONDecodeError):
            self.increment("rejected_malformed_json")
            return None
        if not isinstance(document, dict) or not isinstance(
            document.get("resourceMetrics"), list
        ):
            self.increment("rejected_schema")
            return None

        rows: list[dict[str, Any]] = []
        for resource_metrics in document["resourceMetrics"]:
            if not isinstance(resource_metrics, dict):
                self.increment("rejected_points")
                continue
            resource = resource_metrics.get("resource", {})
            resource_attributes = (
                resource.get("attributes", []) if isinstance(resource, dict) else []
            )
            scopes = resource_metrics.get("scopeMetrics", [])
            if not isinstance(scopes, list):
                self.increment("rejected_points")
                continue
            for scope_metrics in scopes:
                if not isinstance(scope_metrics, dict):
                    self.increment("rejected_points")
                    continue
                metrics = scope_metrics.get("metrics", [])
                if not isinstance(metrics, list):
                    self.increment("rejected_points")
                    continue
                for metric in metrics:
                    if not isinstance(metric, dict):
                        self.increment("rejected_points")
                        continue
                    name = metric.get("name")
                    if name not in METRICS:
                        self.increment("ignored_metrics")
                        continue
                    sum_value = metric.get("sum")
                    if not isinstance(sum_value, dict) or not isinstance(
                        sum_value.get("dataPoints"), list
                    ):
                        self.increment("ignored_non_sum_metrics")
                        continue
                    aggregation_temporality, is_monotonic = sum_metadata(sum_value)
                    for point in sum_value["dataPoints"]:
                        if not isinstance(point, dict):
                            self.increment("rejected_points")
                            continue
                        attributes = selected_attributes(
                            resource_attributes, point.get("attributes", [])
                        )
                        attributes = {
                            key: value
                            for key, value in attributes.items()
                            if key in METRIC_ATTRIBUTES[name]
                        }
                        value = numeric_value(point)
                        token_type = attributes.get("type")
                        query_source = attributes.get("query_source")
                        token_type_valid = (
                            name != "claude_code.token.usage"
                            or token_type in TOKEN_TYPES
                        )
                        query_source_valid = (
                            name == "claude_code.session.count"
                            or query_source in QUERY_SOURCES
                        )
                        if not token_type_valid:
                            self.record_rejected_value(
                                "rejected_token_type_values", token_type
                            )
                        if not query_source_valid:
                            self.record_rejected_value(
                                "rejected_query_source_values", query_source
                            )
                        valid = (
                            "session.id" in attributes
                            and value is not None
                            and value >= 0
                            and token_type_valid
                            and query_source_valid
                        )
                        if not valid:
                            self.increment("rejected_points")
                            continue
                        if self.stats["accepted_points"] + len(rows) >= MAX_POINTS:
                            self.increment("rejected_capacity_points")
                            continue
                        rows.append(
                            {
                                "metric": name,
                                "attributes": attributes,
                                "aggregationTemporality": aggregation_temporality,
                                "isMonotonic": is_monotonic,
                                "startTimeUnixNano": point.get("startTimeUnixNano"),
                                "timeUnixNano": point.get("timeUnixNano"),
                                "value": value,
                            }
                        )
        self.increment("accepted_points", len(rows))
        if not rows:
            self.increment("empty_allowlisted_requests")
        return rows


def handler_for(capture: Capture) -> type[BaseHTTPRequestHandler]:
    class Handler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:
            capture.mark_request()
            if capture.stats["requests_total"] > MAX_REQUESTS:
                capture.increment("rejected_capacity_requests")
                capture.write_stats()
                self.send_error(429)
                return
            if self.path != "/v1/metrics":
                capture.increment("rejected_bad_path")
                capture.write_stats()
                self.send_error(404)
                return
            raw_length = self.headers.get("Content-Length")
            try:
                length = int(raw_length) if raw_length is not None else -1
            except ValueError:
                length = -1
            if length < 0:
                capture.increment("rejected_missing_length")
                capture.write_stats()
                self.send_error(411)
                return
            if length > MAX_BODY_BYTES:
                capture.increment("rejected_oversized")
                capture.write_stats()
                self.send_error(413)
                return
            rows = capture.parse(self.rfile.read(length))
            if rows is None:
                capture.write_stats()
                self.send_error(400)
                return
            capture.append_rows(rows)
            capture.increment("accepted_requests")
            capture.write_stats()
            response = b"{}"
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(response)))
            self.end_headers()
            self.wfile.write(response)

        def log_message(self, *_: Any) -> None:
            pass

    return Handler


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--port", default=0, type=int)
    args = parser.parse_args()
    args.root.mkdir(mode=0o700, parents=True, exist_ok=True)
    capture = Capture(args.root)
    server = HTTPServer(("127.0.0.1", args.port), handler_for(capture))
    endpoint = f"http://127.0.0.1:{server.server_port}/v1/metrics"
    (args.root / "endpoint.txt").write_text(endpoint + "\n")
    print(endpoint, flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
        capture.write_stats()


if __name__ == "__main__":
    main()
