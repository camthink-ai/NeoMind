"""Static precheck for eval cases: fixture existence + state_query param contracts.

Catches the class of defects that previously polluted main-objective runs
(missing green-01 in seed-default; device_command_sent with `name` while the
runner only accepted `id`; rejected aliases like name_or_id/dashboard/device).

This is intentionally offline — no live server. Run via:
  python3 eval/run_eval.py validate-all --root eval/cases
or:
  python3 eval/lib/test_case_precheck.py
"""
from __future__ import annotations

import json
import re
from pathlib import Path

# Param keys the runner actually reads. Aliases outside these sets are errors
# (they look green in schema validate but false-fail or raise at grade time).
_ID_OR_NAME = frozenset({"id", "name"})
_QUERY_PARAM_CONTRACT: dict[str, dict[str, frozenset[str]]] = {
    # required_any: at least one of these keys must be present (when non-empty)
    # required: all of these keys must be present
    # allowed: union of every accepted key (extras → error)
    "device_exists": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "rule_exists": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "agent_exists": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "transform_exists": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "dashboard_exists": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "extension_installed": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "connector_exists": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "widget_exists": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "llm_backend_exists": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "push_enabled": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "rule_enabled": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "agent_status": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "agent_execution_count": {"required_any": _ID_OR_NAME, "allowed": _ID_OR_NAME},
    "agent_config": {
        "required_any": _ID_OR_NAME,
        "required": frozenset({"field"}),
        "allowed": _ID_OR_NAME | frozenset({"field"}),
    },
    "channel_exists": {
        "required": frozenset({"name"}),
        "allowed": frozenset({"name"}),
    },
    "device_type_has_metric": {
        "required": frozenset({"id", "metric"}),
        "allowed": frozenset({"id", "metric"}),
    },
    "device_command_sent": {
        "required_any": _ID_OR_NAME,
        "required": frozenset({"command"}),
        "allowed": _ID_OR_NAME | frozenset({"command"}),
    },
    "rule_action_type": {
        "required_any": _ID_OR_NAME,
        "required": frozenset({"action_type"}),
        "allowed": _ID_OR_NAME | frozenset({"action_type"}),
    },
    "rule_references_source": {
        "required_any": _ID_OR_NAME,
        "allowed": _ID_OR_NAME | frozenset({"source"}),
    },
    "dashboard_component_count": {
        "required_any": _ID_OR_NAME,
        "allowed": _ID_OR_NAME,
    },
    "dashboard_component_bound": {
        "required_any": _ID_OR_NAME,
        "allowed": _ID_OR_NAME | frozenset({"device_id", "metric"}),
    },
    "dashboard_component_expr": {
        "required_any": _ID_OR_NAME,
        "allowed": _ID_OR_NAME | frozenset({"expr_contains"}),
    },
    "latest_telemetry": {
        "required": frozenset({"device_id", "metric"}),
        "allowed": frozenset({"device_id", "metric", "limit"}),
    },
    "settings_value": {
        "required": frozenset({"key"}),
        "allowed": frozenset({"key", "field"}),
    },
    "response_contains": {
        "allowed": frozenset({"text"}),
    },
    # count queries take no identity params
    "device_count": {"allowed": frozenset()},
    "rule_count": {"allowed": frozenset()},
    "transform_count": {"allowed": frozenset()},
    "message_count": {"allowed": frozenset()},
}

# Rejected aliases seen in withdrawn case drafts (2026-11-13).
_REJECTED_ALIASES = frozenset({
    "name_or_id", "device", "dashboard", "rule", "source",  # source ok only on rule_references
})

# Device-like tokens referenced in prompts / assertions.
_DEVICE_TOKEN = re.compile(r"\b([a-z][a-z0-9]*(?:-[a-z0-9]+)+)\b", re.I)

# Phrases that mean the case itself creates the *device* (not a dashboard/rule).
_CREATE_HINT = re.compile(
    r"(create(?:\s+the)?\s+device(?:\s+type)?|register(?:\s+(?:a|the)\s+)?device|"
    r"onboard|创建设备(?:类型)?|注册(?:一个)?(?:该类型)?设备|接入)",
    re.I,
)


def _repo_root() -> Path:
    # eval/lib/case_precheck.py → repo root
    return Path(__file__).resolve().parents[2]


def fixtures_dir(root: Path | None = None) -> Path:
    return (root or _repo_root()) / "eval" / "fixtures"


def load_fixture_names(root: Path | None = None) -> set[str]:
    return {p.stem for p in fixtures_dir(root).glob("*.json")}


def load_fixture(name: str, root: Path | None = None) -> dict:
    path = fixtures_dir(root) / f"{name}.json"
    if not path.is_file():
        raise FileNotFoundError(str(path))
    return json.loads(path.read_text(encoding="utf-8"))


def fixture_device_ids(fixture: dict) -> set[str]:
    ids: set[str] = set()
    for d in fixture.get("devices") or []:
        if not isinstance(d, dict):
            continue
        for k in ("device_id", "id", "name"):
            v = d.get(k)
            if isinstance(v, str) and v:
                ids.add(v)
    return ids


def check_state_query_params(q: dict) -> list[str]:
    """Return errors for one state_query's param contract."""
    errors: list[str] = []
    if not isinstance(q, dict):
        return ["state_query is not an object"]
    t = q.get("type", "")
    params = q.get("params") or {}
    if not isinstance(params, dict):
        return [f"state_query {t}: params must be an object"]
    contract = _QUERY_PARAM_CONTRACT.get(t)
    if contract is None:
        # Unknown types are already caught by validate.KNOWN_QUERY_TYPES.
        return []
    keys = set(params.keys())
    allowed = set(contract.get("allowed", frozenset()))
    # `source` is allowed on rule_references_source only — strip from reject set there.
    rejected = _REJECTED_ALIASES - (
        frozenset({"source"}) if t == "rule_references_source" else frozenset()
    )
    bad_alias = keys & rejected
    if bad_alias:
        errors.append(
            f"state_query {t}: rejected alias param(s) {sorted(bad_alias)} "
            f"(use id/name — see eval/lib/state_query.py)"
        )
    extra = keys - allowed
    # Don't double-report aliases already flagged.
    extra -= bad_alias
    if extra:
        errors.append(
            f"state_query {t}: unknown param(s) {sorted(extra)}; "
            f"allowed={sorted(allowed) or ['(none)']}"
        )
    for req in contract.get("required", frozenset()):
        if req not in params or params.get(req) in (None, ""):
            errors.append(f"state_query {t}: missing required param '{req}'")
    req_any = contract.get("required_any", frozenset())
    if req_any and not any(params.get(k) not in (None, "") for k in req_any):
        errors.append(
            f"state_query {t}: need one of {sorted(req_any)}"
        )
    return errors


def _turn_text(case: dict) -> str:
    parts = []
    for t in case.get("turns") or []:
        if isinstance(t, dict) and isinstance(t.get("user"), str):
            parts.append(t["user"])
    return "\n".join(parts)


def _asserted_device_ids(case: dict) -> set[str]:
    ids: set[str] = set()
    for q in case.get("state_queries") or []:
        if not isinstance(q, dict):
            continue
        params = q.get("params") or {}
        if not isinstance(params, dict):
            continue
        for k in ("device_id", "id", "name"):
            if q.get("type") == "device_command_sent" and k in ("id", "name"):
                v = params.get(k)
                if isinstance(v, str) and v:
                    ids.add(v)
            if k == "device_id":
                v = params.get(k)
                if isinstance(v, str) and v:
                    ids.add(v)
    return ids


def _prompt_device_tokens(text: str) -> set[str]:
    # Heuristic: kebab-case tokens that look like device ids (green-01, valve-01).
    return {m.group(1) for m in _DEVICE_TOKEN.finditer(text)}


def _creates_device(text: str, device_id: str) -> bool:
    # True when a device create/register/onboard phrase appears near the token.
    # Deliberately ignores "Create a dashboard/rule/transform … green-01".
    for m in _CREATE_HINT.finditer(text):
        window = text[max(0, m.start() - 20): m.end() + 100]
        if device_id in window:
            return True
    if re.search(
        rf"(create(?:\s+the)?\s+device(?:\s+type)?|register(?:\s+(?:a|the)\s+)?device|"
        rf"onboard|创建设备(?:类型)?|注册(?:一个)?(?:该类型)?设备|接入)"
        rf".{{0,80}}{re.escape(device_id)}",
        text,
        re.I,
    ):
        return True
    return False


def precheck_case(case: dict, *, fixtures_root: Path | None = None) -> list[str]:
    """Return list of precheck errors for one case (empty = ok)."""
    errors: list[str] = []
    setup = case.get("setup") or {}
    fixture_name = setup.get("fixture") if isinstance(setup, dict) else None
    known = load_fixture_names(fixtures_root)
    fixture: dict | None = None
    if not fixture_name:
        errors.append("setup.fixture is missing")
    elif fixture_name not in known:
        errors.append(f"fixture '{fixture_name}' not found under eval/fixtures/")
    else:
        try:
            fixture = load_fixture(fixture_name, fixtures_root)
        except Exception as e:  # noqa: BLE001 — surface as precheck error
            errors.append(f"fixture '{fixture_name}' unreadable: {e}")

    for q in case.get("state_queries") or []:
        errors.extend(check_state_query_params(q if isinstance(q, dict) else {}))

    # Assumed-device presence is scoped to main-objective: that family is where
    # missing seed entities (green-01 on seed-default) polluted scores. Broader
    # case families often name connectors/pushes/widgets with kebab ids, or rely
    # on extras/simulators — do not false-fail the whole library here.
    category = str(case.get("category") or "")
    workflow = str(case.get("workflow") or "")
    if fixture is not None and (
        category == "main-objective" or workflow == "main-objective"
    ):
        seeded = fixture_device_ids(fixture)
        turns = _turn_text(case)
        for did in sorted(_asserted_device_ids(case)):
            if did in seeded or _creates_device(turns, did):
                continue
            errors.append(
                f"asserted device '{did}' is neither in fixture '{fixture_name}' "
                f"nor created in turns — case will false-fail or force clarification"
            )
        for tok in sorted(_prompt_device_tokens(turns)):
            if not re.search(r"-\d+$", tok):
                continue
            if tok in seeded or _creates_device(turns, tok):
                continue
            if re.search(
                rf"\b{re.escape(tok)}\b.{{0,40}}(temperature|detections|command|metric|"
                rf"温度|命令|指标)|"
                rf"(device|camera|sensor|actuator|设备|摄像头|传感器)\s+{re.escape(tok)}\b|"
                rf"\b{re.escape(tok)}('s|’s)?\s+(temperature|detections)|"
                rf"{re.escape(tok)}\s+的",
                turns,
                re.I,
            ):
                errors.append(
                    f"prompt device '{tok}' missing from fixture '{fixture_name}' "
                    f"and not created in turns"
                )

    return errors


def precheck_path(path: Path, *, fixtures_root: Path | None = None) -> list[str]:
    case = json.loads(path.read_text(encoding="utf-8"))
    return precheck_case(case, fixtures_root=fixtures_root)
