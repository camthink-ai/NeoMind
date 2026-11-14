# Command delivery ladder UI (1.3 iter D)

Date: 2026-11-14. On branch `1.3.0`. Version stays `1.2.11`; no push.

## Problem

`POST /devices/:id/command/:cmd` already returns `delivery` + `delivery_note` (`CommandDelivery`), but the UI treated `sent: true` as ordinary green success — no adapter / extension / device ACK / physical-effect ladder, so operators could not see that dispatch ≠ ACK.

## Fix

- `web/src/lib/commandDeliveryLadder.ts` — derive four tiers; adapter/extension are alternate dispatch proofs; upper tiers stay `not_reached`/`unknown`; `allowVerifiedSuccess` only for `physical_effect_verified`.
- `CommandDeliveryLadderPanel` in device command dialog; API + store return `delivery`/`delivery_note`.
- CommandButton / VisualDashboard toast prefer `delivery_note` (dispatch-honest).

## Not in this slice

- Backend paths that set `DeviceAcknowledged` / `PhysicalEffectVerified`
- Persisting delivery on command history
- Agent execution evidence panel

## Next

**D：Agent 执行证据面板**（输入时间 / 输出 / 动作结果 / 失败节点；缺证据标未知）
