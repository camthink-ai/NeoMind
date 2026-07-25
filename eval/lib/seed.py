"""Seed fixture + case extras into a running TestServer via HTTP POST.

Ported from crates/eval-runner/src/seed.rs. Routes verified against
crates/neomind-api/src/server/router.rs.
"""
from __future__ import annotations

import time

import requests


def _post(server, path: str, body):
    return server.post(path, body)


def _seed_device_types(server, items: list):
    """Register custom device-type templates (POST /device-types) BEFORE devices.

    Required for vertical-scenario fixtures whose devices use domain types
    (soil_moisture_probe, irrigation_valve, …) that aren't built-in. A
    re-register of an existing type returns a conflict — treat as success so
    re-seeding the same server is idempotent.
    """
    for t in items or []:
        r = _post(server, "/device-types", t)
        if not r.ok:
            tlow = (r.text or "").lower()
            if "exist" in tlow or "duplicate" in tlow or "already" in tlow:
                continue
            raise RuntimeError(
                f"seed device_type {t.get('device_type')} -> "
                f"{r.status_code}: {r.text}"
            )


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
    # Register custom device-type templates FIRST — devices below reference
    # them, and a missing template 500s the device POST (see _seed_devices
    # race-retry for the built-in seeding window).
    _seed_device_types(server, fixture.get("device_types"))
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
