"""Tests for case_precheck.py — run: `python3 eval/lib/test_case_precheck.py`."""
from __future__ import annotations

import json
import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, os.path.dirname(__file__))
import case_precheck as cp  # noqa: E402
import validate  # noqa: E402


def _write_fixture_tree(root: Path, fixtures: dict[str, dict]) -> Path:
    fdir = root / "eval" / "fixtures"
    fdir.mkdir(parents=True)
    for name, body in fixtures.items():
        (fdir / f"{name}.json").write_text(
            json.dumps(body), encoding="utf-8"
        )
    return root


def test_rejects_missing_fixture():
    with tempfile.TemporaryDirectory() as td:
        root = _write_fixture_tree(Path(td), {"seed-default": {"devices": []}})
        case = {
            "id": "x",
            "setup": {"fixture": "no-such"},
            "turns": [{"user": "hi"}],
            "expectations": {"per_turn": ["hi"]},
            "state_queries": [],
        }
        errs = cp.precheck_case(case, fixtures_root=root)
        assert any("not found" in e for e in errs), errs


def test_rejects_device_command_sent_without_id_or_name():
    errs = cp.check_state_query_params(
        {"type": "device_command_sent", "params": {"command": "open_valve"}}
    )
    assert any("need one of" in e for e in errs), errs


def test_rejects_name_or_id_alias():
    errs = cp.check_state_query_params(
        {"type": "device_exists", "params": {"name_or_id": "green-01"}}
    )
    assert any("rejected alias" in e for e in errs), errs


def test_accepts_device_command_sent_by_name():
    errs = cp.check_state_query_params(
        {
            "type": "device_command_sent",
            "params": {"name": "valve-01", "command": "open_valve"},
        }
    )
    assert errs == [], errs


def test_flags_assumed_green01_on_seed_default():
    with tempfile.TemporaryDirectory() as td:
        root = _write_fixture_tree(
            Path(td),
            {
                "seed-default": {
                    "devices": [
                        {"device_id": "sensor-001", "name": "Living Room Sensor"}
                    ]
                }
            },
        )
        case = {
            "id": "main-objective-rule-create-explain",
            "category": "main-objective",
            "workflow": "main-objective",
            "setup": {"fixture": "seed-default"},
            "turns": [
                {
                    "user": (
                        "Create rule high-temp-green: notify me when "
                        "green-01's temperature exceeds 30."
                    )
                }
            ],
            "expectations": {"per_turn": ["x"]},
            "state_queries": [
                {
                    "type": "rule_exists",
                    "params": {"name": "high-temp-green"},
                    "expected": True,
                }
            ],
        }
        errs = cp.precheck_case(case, fixtures_root=root)
        assert any("green-01" in e and "missing" in e for e in errs), errs


def test_seed_main_objective_satisfies_assumed_green01():
    # Live repo fixtures — the new seed must clear the scar.
    repo = Path(__file__).resolve().parents[2]
    case = {
        "id": "main-objective-dashboard-card-config",
        "category": "main-objective",
        "workflow": "main-objective",
        "setup": {"fixture": "seed-main-objective"},
        "turns": [
            {
                "user": (
                    "Create a dashboard named Greenhouse Overview with one "
                    "card showing green-01's temperature live value."
                )
            }
        ],
        "expectations": {"per_turn": ["x"]},
        "state_queries": [
            {
                "type": "dashboard_component_bound",
                "params": {
                    "name": "Greenhouse Overview",
                    "device_id": "green-01",
                    "metric": "temperature",
                },
                "expected": True,
            }
        ],
    }
    errs = cp.precheck_case(case, fixtures_root=repo)
    assert errs == [], errs


def test_create_turn_does_not_require_seeded_device():
    with tempfile.TemporaryDirectory() as td:
        root = _write_fixture_tree(
            Path(td), {"seed-default": {"devices": []}}
        )
        case = {
            "id": "main-objective-onboard-describe",
            "category": "main-objective",
            "workflow": "main-objective",
            "setup": {"fixture": "seed-default"},
            "turns": [
                {
                    "user": (
                        "Create the device type greenhouse-temp and register "
                        "device green-01, then tell me its state."
                    )
                }
            ],
            "expectations": {"per_turn": ["x"]},
            "state_queries": [
                {
                    "type": "device_exists",
                    "params": {"name": "green-01"},
                    "expected": True,
                }
            ],
        }
        errs = cp.precheck_case(case, fixtures_root=root)
        assert errs == [], errs


def test_validate_case_includes_precheck():
    # validate_case must surface the green-01 scar, not only shape errors.
    case = {
        "id": "main-objective-rule-create-explain",
        "category": "main-objective",
        "workflow": "main-objective",
        "scenario_type": "single_turn",
        "applies": ["tool_accuracy"],
        "setup": {"fixture": "seed-default"},
        "turns": [
            {
                "user": (
                    "Create rule high-temp-green: notify when green-01's "
                    "temperature exceeds 30."
                )
            }
        ],
        "expectations": {"per_turn": ["x"]},
        "state_queries": [
            {
                "type": "rule_exists",
                "params": {"name": "high-temp-green"},
                "expected": True,
            }
        ],
    }
    errs = validate.validate_case(case)
    assert any("green-01" in e for e in errs), errs


def main() -> int:
    tests = [v for k, v in sorted(globals().items()) if k.startswith("test_")]
    failed = 0
    for fn in tests:
        try:
            fn()
            print(f"ok  {fn.__name__}")
        except Exception as e:  # noqa: BLE001
            failed += 1
            print(f"FAIL {fn.__name__}: {e}")
    print(f"{len(tests) - failed}/{len(tests)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
