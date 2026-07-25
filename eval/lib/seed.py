"""Seed fixture + case extras into a running TestServer via HTTP POST.

Ported from crates/eval-runner/src/seed.rs. Routes verified against
crates/neomind-api/src/server/router.rs.
"""
from __future__ import annotations

import time

import requests


def _post(server, path: str, body):
    return server.post(path, body)


def _is_template_race(resp_text: str) -> bool:
    """True for the startup-race signature: 'template ... not found'.

    `spawn()` returns as soon as /health goes green, but the backend seeds
    built-in device-type templates slightly after — a device POST landing in
    that window 500s with "Device type template 'ne101_camera' not found".
    Flaky across runs (same fixture passes/fails). Retry just this signature.
    """
    t = (resp_text or "").lower()
    return "not found" in t and "template" in t


def _seed_devices(server, items: list):
    for d in items or []:
        # Bounded retry on the template-seeding race (see _is_template_race).
        # Other failures (real 400/500) raise immediately.
        deadline = time.monotonic() + 5.0
        while True:
            r = _post(server, "/devices", d)
            if r.ok:
                break
            if _is_template_race(r.text) and time.monotonic() < deadline:
                time.sleep(0.5)
                continue
            raise RuntimeError(
                f"seed device {d.get('device_id') or d.get('id')} -> "
                f"{r.status_code}: {r.text}"
            )


def _seed_metrics(server, items: list):
    # WriteMetricRequest expects field "metric".
    for m in items or []:
        device_id = m.get("device_id")
        if not device_id:
            raise RuntimeError(f"metric missing device_id: {m}")
        body = {
            "metric": m.get("metric"),
            "value": m.get("value"),
        }
        r = _post(server, f"/devices/{device_id}/metrics", body)
        if not r.ok:
            raise RuntimeError(
                f"seed metric -> {r.status_code}: {r.text}"
            )


def _seed_simple(server, items: list, path: str, kind: str):
    for x in items or []:
        r = _post(server, path, x)
        if not r.ok:
            raise RuntimeError(
                f"seed {kind} -> {r.status_code}: {r.text}"
            )


def seed_fixture(server, fixture: dict):
    _seed_devices(server, fixture.get("devices"))
    _seed_metrics(server, fixture.get("metrics"))
    _seed_simple(server, fixture.get("rules"), "/rules", "rule")
    _seed_simple(server, fixture.get("agents"), "/agents", "agent")
    _seed_simple(server, fixture.get("transforms"), "/automations", "transform")
    _seed_simple(server, fixture.get("dashboards"), "/dashboards", "dashboard")
    _seed_simple(server, fixture.get("channels"), "/messages/channels", "channel")
    # extensions omitted — Tier 1 doesn't ship .nep binaries


def seed_extras(server, extras: dict):
    seed_fixture(server, extras)
