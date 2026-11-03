"""Compile scoped device observations from captured shell results, without judging prose.

Only direct neomind device get/history/list invocations are recognized. All payload
strings (including telemetry, names and suggestions) are untrusted data. The
packet describes observations, never diagnoses or lifetime/transition claims.
"""
import json
import shlex

_UNKNOWN = ["root_cause", "previous_transport_history", "lifetime_telemetry", "disconnect_time"]
_CURRENT = ("status", "online", "is_online", "connected", "is_connected", "transport_connected",
            "last_seen", "transport_changed_at", "connection_status", "data_status")
_ENVELOPE = {"success", "data", "meta", "message", "suggestion", "suggestions", "error", "errors"}


def _command(command):
    if not isinstance(command, str):
        return None
    try:
        parts = shlex.split(command)
    except ValueError:
        return None
    if len(parts) < 3 or parts[:2] != ["neomind", "device"]:
        return None
    verb = parts[2]
    if verb not in ("get", "history", "list"):
        return None
    # No shell compositions, expansion, redirection or wrappers are supported.
    if any(c in command for c in "\n\r;$`|&<>()"):
        return None
    tail = parts[3:]
    entity = None
    if verb != "list":
        if not tail or tail[0].startswith("-"):
            return None
        entity, tail = tail[0], tail[1:]
    options = {}
    valued = {"--metric", "--time-range", "--limit"} if verb == "history" else ({"--metric"} if verb == "get" else {"--page", "--limit", "--status", "--device-type"})
    flags = {"--compress"} if verb == "history" else ({"--summary"} if verb == "list" else set())
    while tail:
        flag, *tail = tail
        if flag in flags:
            options[flag[2:]] = True
        elif flag in valued and tail and not tail[0].startswith("--"):
            options[flag[2:]], tail = tail[0], tail[1:]
        else:
            return None
    return verb, entity, options


def _unwrap(value):
    for _ in range(8):
        if not isinstance(value, dict):
            raise ValueError("non_object_payload")
        if value.get("success") is False:
            raise ValueError("payload_failure")
        if "data" in value and set(value) <= _ENVELOPE:
            value = value["data"]
            continue
        return value
    raise ValueError("envelope_depth_exceeded")


def _identity(payload, requested):
    ids = [payload[k] for k in ("id", "device_id") if payload.get(k) not in (None, "")]
    if not ids or any(not isinstance(i, str) or i != requested for i in ids):
        raise ValueError("entity_missing_or_mismatch")
    return ids[0]


def _inventory(payload):
    """Keep list status/counts in inventory scope; type examples aren't telemetry checks."""
    keys = ("id", "device_id", "name", "device_type", "status", "online")
    def entities(values):
        if not isinstance(values, list) or any(not isinstance(d, dict) for d in values):
            raise ValueError("invalid_inventory")
        return [{k: d[k] for k in keys if k in d} for d in values]
    if isinstance(payload.get("devices"), list):
        return {"entities": entities(payload["devices"])}
    summary = payload.get("summary")
    if not isinstance(summary, dict):
        raise ValueError("invalid_inventory")
    result = {"summary": {k: summary[k] for k in ("total", "online", "offline", "type_count") if k in summary},
              "type_groups": [], "entities": []}
    types = payload.get("types", [])
    if not isinstance(types, list):
        raise ValueError("invalid_inventory")
    for group in types:
        if not isinstance(group, dict) or not isinstance(group.get("devices"), dict):
            raise ValueError("invalid_inventory")
        listed = entities(group["devices"].get("list"))
        typed = {k: group[k] for k in ("type", "online", "offline", "metric_fields", "command_fields") if k in group}
        typed["entities"] = listed
        result["type_groups"].append(typed)
    result["entities"] = entities(payload.get("ungrouped", []))
    return result


def compile_evidence(record):
    """Return a JSON-serializable packet; source indexes are zero based.

    Unknown dimensions remain unknown even when a query returns zero rows.
    Pagination.total is endpoint-provided (possibly first-metric scoped), not
    a count of all device telemetry. Each history call remains a separate window.
    """
    packet = {"schema_version": 1, "observations": [], "source_errors": [], "unhandled_sources": [],
              "unverified_dimensions": list(_UNKNOWN)}
    if not isinstance(record, dict) or not isinstance(record.get("turn_records"), list):
        packet["source_errors"].append({"reason": "invalid_turn_records"})
        return packet
    for ti, turn in enumerate(record["turn_records"]):
        if not isinstance(turn, dict) or not isinstance(turn.get("tool_calls", []), list):
            packet["source_errors"].append({"turn": ti, "reason": "invalid_turn"})
            continue
        for ci, call in enumerate(turn.get("tool_calls", [])):
            source = {"turn": ti, "call": ci}
            if not isinstance(call, dict):
                packet["source_errors"].append(dict(source, reason="invalid_call"))
                continue
            if call.get("name") != "shell":
                packet["unhandled_sources"].append(dict(source, reason="unsupported_tool_type"))
                continue
            args = call.get("arguments")
            parsed = _command(args.get("command") if isinstance(args, dict) else None)
            if parsed is None:
                packet["unhandled_sources"].append(dict(source, reason="unsupported_or_ambiguous_shell_command"))
                continue
            verb, requested, options = parsed
            source["command"] = args["command"]
            result = payload = None
            try:
                result = call.get("result")
                result = json.loads(result) if isinstance(result, str) else result
                if not isinstance(result, dict):
                    raise ValueError("invalid_shell_result")
                if type(result.get("exit_code")) is not int or result["exit_code"] != 0 or result.get("timed_out") not in (None, False):
                    raise ValueError("shell_failure")
                if "command" in result and result["command"] != args["command"]:
                    raise ValueError("result_command_mismatch")
                stdout = result.get("stdout")
                if not isinstance(stdout, str):
                    raise ValueError("invalid_stdout")
                payload, _ = json.JSONDecoder().raw_decode(stdout.lstrip())
                payload = _unwrap(payload)
                observation = {"source": source, "endpoint": "device." + verb,
                               "requested_entity_id": requested, "query_options": options}
                if verb == "get":
                    device = payload.get("device", payload)
                    if not isinstance(device, dict):
                        raise ValueError("non_object_payload")
                    observation["entity_id"] = _identity(device, requested)
                    # An outer explicit identity must agree with the nested device.
                    if any(payload.get(k) not in (None, "") for k in ("id", "device_id")):
                        _identity(payload, requested)
                    fields = {k: device[k] for k in _CURRENT if k in device}
                    observation["device_fields"] = {k: device[k] for k in
                        ("id", "device_id", "name", "device_type") + _CURRENT if k in device}
                    if "metrics" in payload:
                        observation["current_metrics"] = payload["metrics"]
                        observation["current_metrics_scope"] = "returned_current_snapshot_not_history"
                    if not fields:
                        raise ValueError("missing_current_fields")
                    observation.update(kind="current_snapshot", fields=fields,
                                       last_seen_semantics="status_timestamp_not_disconnect_proof")
                elif verb == "history":
                    observation["entity_id"] = _identity(payload, requested)
                    if "data" not in payload or not isinstance(payload["data"], dict):
                        raise ValueError("invalid_history_data")
                    observation.update(kind="telemetry_window", fields={k: payload[k] for k in
                        ("start", "end", "metrics", "aggregated", "pagination", "data") if k in payload},
                        scope="returned_page_within_reported_window", total_semantics="endpoint_defined_metric_scope",
                        outside_window="unknown", lifetime="unknown")
                else:
                    inventory = _inventory(payload)
                    observation.update(kind="inventory", **inventory,
                                       per_device_telemetry_checked=False)
                packet["observations"].append(observation)
            except (ValueError, TypeError, RecursionError) as exc:
                reason = str(exc) if str(exc) in {"non_object_payload", "payload_failure", "envelope_depth_exceeded", "entity_missing_or_mismatch", "invalid_shell_result", "shell_failure", "result_command_mismatch", "invalid_stdout", "missing_current_fields", "invalid_history_data", "invalid_inventory"} else "malformed_output"
                error = dict(source, reason=reason,
                             kind="measurement_failed" if reason in {"shell_failure", "payload_failure"} else "unusable_source")
                if error["kind"] == "measurement_failed" and isinstance(result, dict):
                    error["outcome"] = {k: result[k] for k in ("exit_code", "timed_out", "stderr") if k in result}
                    if isinstance(payload, dict) and "error" in payload:
                        error["outcome"]["reported_error"] = payload["error"]
                packet["source_errors"].append(error)
    return packet


compile_semantic_evidence = compile_evidence
