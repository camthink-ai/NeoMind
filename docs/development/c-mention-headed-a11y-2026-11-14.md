# §12 C — headed Playwright mention a11y / touch / 读屏属性（2026-11-14）

## Tip / 约束
- Start: `cc39e862` on `1.3.0`; no side branch / push.
- Eval **138 still occupying**: controller PID **89912** (`eval/run_eval.py` … `/tmp/neomind-iter-main-20261113/eval-138`), llama-server **89904** on `127.0.0.1:29431`. **Not touched**; no new long-run started.

## What ran (automation)
Headed Chrome Playwright + mocked `/api` (mobile-audit pattern — no live Spark):

```bash
cd web && HEADED=1 npx playwright test e2e/specs/mention-a11y.spec.ts --project=chrome
# or: npm run test:e2e:mention-a11y
```

| Spec | Result |
|------|--------|
| prototype toolbar → listbox ARIA + keyboard insert chip | pass |
| prototype axe wcag2a/aa serious/critical (color-contrast disabled) | pass |
| chat history `contentNodes` → `.nm-mention` + reuse-in-composer | pass |
| chat toolbar `@` opens listbox; Escape closes | pass |
| mobile viewport touch: prototype panel + tap select | pass |
| mobile viewport: history chips visible / ≥16px tall | pass |

**Gates: 6/6 headed** (~22s). Log (tracked): `docs/development/artifacts/c-mention-headed-a11y-playwright-2026-11-14.log`.

## Explicit human gaps (not claimed)
| Item | Status |
|------|--------|
| Physical touch device + soft keyboard + OS back-dismiss | **缺口** — only Playwright `hasTouch` / iPhone-size viewport |
| VoiceOver / TalkBack live read-through | **缺口** — axe + ARIA asserts only |
| 5 users × 4 tasks ≥18/20 | **缺口** — protocol in checklist; **0/20 executed**; do not treat as pass |

Checklist: `docs/development/mention-a11y-usability-checklist-2026-11-14.md` (updated).

## Next
1. Human: recruit 5×4 + VoiceOver + one real phone (when available).
2. After **138 ends**: live Spark pairing (do not start while `:29431` busy).
3. Plan D/E/F remaining.
