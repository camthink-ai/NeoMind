"""State queries against the test server (spec §4a).

12 query types — all HTTP GET. Ported faithfully from
crates/eval-runner/src/state_query.rs so cases behave identically under the
Python runner.
"""
from __future__ import annotations

import requests


def _unwrap_envelope(body):
    """Many NeoMind routes return {success, data}; return data if present."""
    if isinstance(body, dict) and "data" in body:
        return body["data"]
    return body


def _get_json(base: str, key: str, path: str):
    r = requests.get(f"{base}{path}", headers={"Authorization": f"Bearer {key}"}, timeout=30)
    body = r.json()
    return _unwrap_envelope(body)


def _exists(base: str, key: str, path: str) -> bool:
    r = requests.get(f"{base}{path}", headers={"Authorization": f"Bearer {key}"}, timeout=30)
    return r.status_code == 200


def _field(base: str, key: str, path: str, field: str):
    v = _get_json(base, key, path)
    if isinstance(v, dict):
        return v.get(field)
    return None


def _count(base: str, key: str, path: str) -> int:
    v = _get_json(base, key, path)
    if isinstance(v, list):
        return len(v)
    if isinstance(v, dict):
        for k in ("devices", "rules", "agents", "channels", "messages",
                  "dashboards", "automations", "data", "items"):
            arr = v.get(k)
            if isinstance(arr, list):
                return len(arr)
        for k in ("count", "total"):
            n = v.get(k)
            if isinstance(n, int):
                return n
    return 0


def _sid(params: dict, key: str) -> str:
    v = params.get(key)
    if not isinstance(v, str) or not v:
        raise ValueError(f"missing param {key}")
    return v


def _find_by_name(base: str, key: str, list_path: str, name: str) -> bool:
    """List collection + match by `name` field. Returns True on any match.

    Used by `*_exists` queries when the case specifies `name` instead of an
    explicit id — agents often create entities with auto-generated UUIDs that
    can't be predicted at case-authoring time.
    """
    v = _get_json(base, key, list_path)
    items: list = []
    if isinstance(v, list):
        items = v
    elif isinstance(v, dict):
        for k in ("devices", "rules", "agents", "channels", "messages",
                  "dashboards", "automations", "transforms", "data", "items"):
            arr = v.get(k)
            if isinstance(arr, list):
                items = arr
                break
    for it in items:
        if not isinstance(it, dict):
            continue
        n = it.get("name")
        if isinstance(n, str) and n == name:
            return True
        if isinstance(n, dict):
            # Localized name {en: "...", zh: "..."} — match any locale value.
            if any(v == name for v in n.values() if isinstance(v, str)):
                return True
    return False


def _id_or_name_exists(base: str, key: str, list_path: str,
                       get_path_template: str, params: dict) -> bool:
    """Existence check with transparent name fallback.

    Order:
    1. If `params.id` is given, try `GET /<resource>/{id}` directly.
       If 200, return True. If the value isn't a real UUID (e.g. a
       human-readable name like "rule-battery-low" that case-authors
       often pass as `id`), this GET returns 400/404 — fall through.
    2. Treat the value (id or name) as a display name and search the
       collection list for an entity whose `name` field matches.

    This mirrors how real users recover from "Invalid rule ID" errors and
    keeps existing case files working without modification.
    """
    val = params.get("id") or params.get("name")
    if not val:
        raise ValueError("state_query requires `id` or `name` param")
    if params.get("id"):
        if _exists(base, key, get_path_template.format(id=params["id"])):
            return True
    return _find_by_name(base, key, list_path, val)


def _resolve_id_or_name(base: str, key: str, list_path: str,
                        collection_key: str, params: dict) -> str | None:
    """Return an id suitable for GET /<resource>/{id}.

    Used by queries that need a full record (rule_enabled, agent_status,
    etc.). If `params.id` is a real UUID that resolves, return it as-is.
    Otherwise (no id, OR id is actually a name, OR id doesn't resolve),
    list the collection and find the entity whose `name` matches, then
    return its UUID.
    """
    val = params.get("id") or params.get("name")
    if not val:
        raise ValueError("state_query requires `id` or `name` param")
    if params.get("id"):
        # Probe: if a direct GET succeeds, the id is real.
        r = requests.get(
            f"{base}{list_path}/{params['id']}",
            headers={"Authorization": f"Bearer {key}"}, timeout=30,
        )
        if r.status_code == 200:
            return params["id"]
        # Else fall through and treat the value as a name.
    v = _get_json(base, key, list_path)
    items: list = []
    if isinstance(v, list):
        items = v
    elif isinstance(v, dict):
        arr = v.get(collection_key) if collection_key else None
        if isinstance(arr, list):
            items = arr
        else:
            for k in ("devices", "rules", "agents", "channels", "messages",
                      "dashboards", "automations", "transforms", "data", "items"):
                a = v.get(k)
                if isinstance(a, list):
                    items = a
                    break
    for it in items:
        if isinstance(it, dict) and it.get("name") == val:
            for id_key in ("id", "uuid", "automation_id", "rule_id", "agent_id"):
                if it.get(id_key):
                    return it[id_key]
    return None


def _get_with_name_fallback(base: str, key: str, list_path: str,
                            get_path_template: str, collection_key: str,
                            params: dict):
    """GET a single record by id (or by name→id resolution).

    Returns the unwrapped JSON dict (or {} if not found).
    """
    resolved = _resolve_id_or_name(base, key, list_path, collection_key, params)
    if not resolved:
        return {}
    v = _get_json(base, key, get_path_template.format(id=resolved))
    if isinstance(v, dict):
        # Many NeoMind endpoints wrap the record: {rule: {...}}, {agent: {...}}...
        for k in ("rule", "agent", "device", "dashboard", "automation", "channel"):
            inner = v.get(k)
            if isinstance(inner, dict):
                return inner
    return v if isinstance(v, dict) else {}


def _field_with_name_fallback(base: str, key: str, list_path: str,
                              get_path_template: str, collection_key: str,
                              params: dict, field: str):
    v = _get_with_name_fallback(base, key, list_path, get_path_template,
                                collection_key, params)
    return v.get(field) if isinstance(v, dict) else None


def run_query(q: dict, base: str, key: str) -> dict:
    """Run one state_query; returns {type, params, expected, actual, passed}.

    Supported assertion shapes:
    - `expected: <value>` → exact equality (default).
    - `expected_min: <int>` → numeric `actual >= expected_min`. Useful for
      "at least one message was sent" / "agent ran at least once".

    `*_exists` queries accept either `id` (direct lookup) or `name` (list
    + match by name). The name fallback exists because agents create
    entities with auto-generated UUIDs.
    """
    t = q["type"]
    params = q.get("params", {}) or {}
    expected = q.get("expected")
    expected_min = q.get("expected_min")

    if t == "device_exists":
        actual = _id_or_name_exists(base, key, "/devices", "/devices/{id}", params)
    elif t == "rule_exists":
        actual = _id_or_name_exists(base, key, "/rules", "/rules/{id}", params)
    elif t == "agent_exists":
        actual = _id_or_name_exists(base, key, "/agents", "/agents/{id}", params)
    elif t == "transform_exists":
        actual = _id_or_name_exists(base, key, "/automations", "/automations/{id}", params)
    elif t == "dashboard_exists":
        actual = _id_or_name_exists(base, key, "/dashboards", "/dashboards/{id}", params)
    elif t == "channel_exists":
        actual = _exists(base, key, f"/messages/channels/{_sid(params, 'name')}")
    elif t == "rule_enabled":
        actual = _field_with_name_fallback(base, key, "/rules", "/rules/{id}", "rules", params, "enabled")
    elif t == "agent_status":
        actual = _field_with_name_fallback(base, key, "/agents", "/agents/{id}", "agents", params, "status")
    elif t == "agent_execution_count":
        # GET /agents/:id returns stats.total_executions (line 2074 of agents.rs)
        v = _get_with_name_fallback(base, key, "/agents", "/agents/{id}", "agents", params)
        stats = v.get("stats") if isinstance(v, dict) else None
        actual = stats.get("total_executions") if isinstance(stats, dict) else 0
    elif t == "push_enabled":
        actual = _field(base, key, f"/data-push/{_sid(params, 'id')}", "enabled")
    elif t == "device_count":
        actual = _count(base, key, "/devices")
    elif t == "message_count":
        actual = _count(base, key, "/messages")
    elif t == "rule_count":
        actual = _count(base, key, "/rules")
    elif t == "transform_count":
        # GET /automations returns all automations (transforms included);
        # the response carries either an `automations` array or a top-level list.
        actual = _count(base, key, "/automations")
    elif t == "dashboard_component_count":
        v = _get_with_name_fallback(base, key, "/dashboards", "/dashboards/{id}", "dashboards", params)
        comps = v.get("components") if isinstance(v, dict) else None
        actual = len(comps) if isinstance(comps, list) else 0
    elif t == "dashboard_component_bound":
        # Verify a dashboard component's data_source references the expected
        # device + metric (tests correct data binding, not just existence).
        # data_source format: {type:"telemetry", source:"device", id:"<dev>",
        # field:"<metric>", sourceId, metricId, ...} or array for multi-series.
        v = _get_with_name_fallback(base, key, "/dashboards", "/dashboards/{id}", "dashboards", params)
        comps = v.get("components") if isinstance(v, dict) else None
        want_dev = str(params.get("device_id", "")).lower()
        want_metric = str(params.get("metric", "")).lower()
        actual = False
        if isinstance(comps, list):
            for comp in comps:
                if not isinstance(comp, dict):
                    continue
                ds = comp.get("data_source")
                sources = ds if isinstance(ds, list) else ([ds] if isinstance(ds, dict) else [])
                for src in sources:
                    if not isinstance(src, dict):
                        continue
                    src_dev = str(src.get("id") or src.get("sourceId") or "").lower()
                    src_metric = str(src.get("field") or src.get("metricId") or "").lower()
                    dev_ok = (not want_dev) or (want_dev in src_dev)
                    metric_ok = (not want_metric) or (want_metric in src_metric)
                    if dev_ok and metric_ok:
                        actual = True
                        break
                if actual:
                    break
    elif t == "extension_installed":
        actual = _id_or_name_exists(base, key, "/extensions", "/extensions/{id}", params)
    elif t == "widget_exists":
        actual = _id_or_name_exists(base, key, "/frontend-components",
                                    "/frontend-components/{id}", params)
    elif t == "llm_backend_exists":
        actual = _id_or_name_exists(base, key, "/llm-backends", "/llm-backends/{id}", params)
    elif t == "settings_value":
        # GET /settings/<key> → {success, data}; extract a sub-field if `field`
        # param given, else compare the whole value.
        v = _get_json(base, key, f"/settings/{_sid(params, 'key')}")
        if isinstance(v, dict) and params.get("field"):
            actual = v.get(params["field"])
        else:
            actual = v
    elif t == "device_type_has_metric":
        # A device-type template defining params.metric exists, matched by id
        # OR name (normalized — ignores case and -/_/space separators, so an
        # auto-generated id like 'mystery_wibration_sensor' matches an asserted
        # 'mystery-vibration-sensor' / name 'Mystery Vibration Sensor').
        # For the "AI infers a template from raw samples and creates it"
        # scenario: assert the created template defines the expected metric.
        import re as _re
        want = _re.sub(r"[^a-z0-9]", "", _sid(params, "id").lower())
        v = _get_json(base, key, "/device-types")
        templates = []
        if isinstance(v, list):
            templates = v
        elif isinstance(v, dict):
            arr = v.get("device_types") or v.get("data") or []
            if isinstance(arr, list):
                templates = arr
        tpl = None
        for tm in templates:
            if not isinstance(tm, dict):
                continue
            for key_field in ("device_type", "name"):
                val = _re.sub(r"[^a-z0-9]", "", str(tm.get(key_field, "")).lower())
                if val and val == want:
                    tpl = tm
                    break
            if tpl:
                break
        names = []
        if isinstance(tpl, dict):
            ms = tpl.get("metrics")
            if isinstance(ms, list):
                names = [m.get("name") for m in ms if isinstance(m, dict)]
        actual = params.get("metric") in names
    elif t == "device_command_sent":
        # GET /devices/:id/commands → command history; does it include
        # params.command? Downlink assertion. The platform records every
        # command in history BEFORE routing (service.rs send_command), so this
        # works even for offline simulated devices.
        want_id = _sid(params, "id")
        v = _get_json(base, key, f"/devices/{want_id}/commands")
        records = []
        if isinstance(v, list):
            records = v
        elif isinstance(v, dict):
            for k in ("commands", "data", "history", "items"):
                arr = v.get(k)
                if isinstance(arr, list):
                    records = arr
                    break
        names = [
            str(c.get("command_name") or c.get("command") or c.get("name") or "").lower()
            for c in records if isinstance(c, dict)
        ]
        actual = str(params.get("command", "")).lower() in names
    elif t == "rule_references_source":
        # Depth assertion: does a rule's condition reference the expected data
        # source (e.g. a transform output, or a specific device metric)?
        # Walks comparison.source + recurses logical.conditions.
        rule = _get_with_name_fallback(base, key, "/rules", "/rules/{id}", "rules", params)
        want = str(params.get("source", "")).lower()
        sources = []

        def _walk(cond):
            if isinstance(cond, dict):
                s = cond.get("source")
                if isinstance(s, str):
                    sources.append(s.lower())
                for sub in cond.get("conditions") or []:
                    _walk(sub)

        _walk(rule.get("condition") if isinstance(rule, dict) else None)
        actual = (any(want in s for s in sources) if want else len(sources) > 0)
    elif t == "rule_action_type":
        # Depth assertion: does a rule have an action of the expected type
        # (notify / execute / trigger_agent)? For the rule→agent handoff case.
        rule = _get_with_name_fallback(base, key, "/rules", "/rules/{id}", "rules", params)
        want = str(params.get("action_type", "")).lower()
        acts = rule.get("actions") if isinstance(rule, dict) else None
        types = [
            str(a.get("type", "")).lower()
            for a in acts if isinstance(a, dict)
        ] if isinstance(acts, list) else []
        actual = (want in types) if want else (len(types) > 0)
    else:
        raise ValueError(f"unknown state_query type: {t}")

    return {
        "type": t,
        "params": params,
        "expected": expected,
        "expected_min": expected_min,
        "actual": actual,
        "passed": (
            actual >= expected_min
            if expected_min is not None
            else actual == expected
        ),
    }
