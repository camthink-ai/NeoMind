# Agent execution evidence panel (1.3 iter D)

Date: 2026-11-14. On branch `1.3.0`. Version stays `1.2.11`; no push.

## Problem

Execution detail UI showed reasoning/actions but did not surface a clear evidence contract: input **times**, output, action **results**, and **failure nodes**. Missing fields were silent; `stop_reason` / `tokens_used` existed in storage but were dropped by the API DTO.

## Fix

- API: `DecisionProcessDto` now includes `stop_reason` (omit if empty) and `tokens_used` (omit if 0).
- `deriveAgentExecutionEvidence` — known / empty / unknown for inputs, output, actions; failure from error → failed action → stop_reason → unknown; timestamp ≤0 → time unknown.
- `AgentExecutionEvidencePanel` wired into `ExecutionDetailDialog` and expanded `AgentExecutionTimeline`.
- Input Data rows also show timestamp or “Time unknown”.

## Not in this slice

- Journal DTO `stop_reason`
- Desktop/mobile representative walkthrough / 5-person usability
- Inventing device ACK from agent command action text

## Next

**E：首个可运行方案 10 次预注册验收**
