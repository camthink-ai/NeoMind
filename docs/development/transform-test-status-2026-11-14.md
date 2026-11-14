# Transform test status honesty (1.3 iter D first slice)

Date: 2026-11-14. Tip at commit of this slice. Version stays `1.2.11`; no push.

## Problem

`POST /automations/transforms/:id/test` and `test-code` already return `warnings` and `unverified`, but the UI painted every HTTP 200 as green success — including JS failures (warnings + zero metrics) and dry-run extension blocks (`unverified` non-empty with optional metrics).

## Fix

- `web/src/lib/transformTestStatus.ts` — derive `failed | no_output | partial_success | unverified | verified_success`; green only for `verified_success`.
- `TransformTestResultPanel` — metrics, unverified deps, and warnings are all visible (expandable); unverified never uses success border/color alone.
- Wired into `TransformTestDialog` and builder `TestStrip`.
- API client types include `unverified`.

## Not in this slice

- Rule “why not triggered” UI (`GET /rules/:id/explain` already exists)
- Command ACK / physical-effect evidence ladder
- Agent execution evidence panel

## Next

**D：Rule「为什么未触发」只读入口**（API 已有；前端接线 + 阻断原因推导）
