# Rule「为什么未触发」只读入口 (1.3 iter D)

Date: 2026-11-14. Version stays `1.2.11`; no push.

## Problem

`GET /api/rules/:id/explain` already returns a live Kleene condition tree, value age, for_duration, cooldown, last execution, and notes — but Rule detail UI only showed static config. Operators could not see why a rule was not firing.

## Fix

- Types: `RuleExplanation` / `ConditionExplanation` / `EvalOutcome` in `web/src/types/rule.ts`.
- API: `automationApi.getRuleExplain(id)` → `GET /rules/:id/explain` (api surface 283→284).
- Pure helper: `deriveRuleBlockingReasons` — engine order disabled → cooldown → condition false/unknown → for_duration; else `ready` (gates clear; fire still depends on trigger path).
- UI: `RuleWhyNotTriggeredPanel` in `RuleDetailDialog` (fetch on open, retry, leaves table, gates, last execution, notes).

## Not in this slice

- Command ACK / physical-effect evidence ladder
- Agent execution evidence panel
- Server-side `blocking_reasons` field (UI derives locally)
- Skill/docs drift for history fields

## Next

**D：命令 ACK 阶梯 / Agent 执行证据面板**（plan §5 剩余）
