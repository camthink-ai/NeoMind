# Development coordination

Read CLAUDE.md and the relevant module documentation before changes.

Current work is coordinated by Codex (2026-11-02): summary preservation, default chat loop exits, and skill search isolation. Do not start overlapping eval chains or edit these same files concurrently. Keep existing run artifacts; use unique output directories.

Use Spark-1.7B for primary acceptance with fixed model/runtime/sampling settings. Keep hardware-specific baselines separate.

Do not put credentials in source or logs. Do not commit, push, deploy, or publish without an explicit user instruction.

Read [latest handoff](docs/development/mac-glm-takeover-2026-11-03.md) before continuing (it supersedes the 2026-11-02 handoff's process states; the cloud coordinator disconnected on 2026-11-03).

Follow the [agent evaluation method](docs/development/agent-eval-method.md) for ongoing improvements.

## Active comprehensive evaluation (2026-11-03)

A frozen three-arm full-regression study is running. Root /tmp/neomind-comprehensive-20261103-085034, official data main/, controller /tmp/neomind-comprehensive-single-run.py PID38647 (verify before acting). 69 IDs, both languages, two rounds, three Release versions =828 trials. Fixed Spark32K single-slot, thinking enabled, .3/.75/20, 600s case cap. Do not start overlapping inference/builds or revive older queues. Four-slot root data is a capacity pilot, not official results. Binaries/evaluator/fixtures/grader are frozen; live workspace source remains untouched. External GLM export of evaluation answers/evidence for semantic judging was rejected by automatic approval review; sender was not run. Do not bypass it. Use local task/state and trace metrics; do not claim universal factuality judging. No commit, push, deploy or publish.

Comprehensive local audit final helper: PID45773 (verify), analysis/ archived with manifest, 7 audit tests and 6 pairing tests pass. Native hard signals are independently recomputed from frozen case+record, not just copied. Final comprehensive-report.md is emitted only after all828 and hash checks pass. Pilot orphan API processes were cleaned; official controller PID38647 remains active.

## Takeover by Mac-local GLM (2026-11-03, ~14:10) — corrects the stale process notes above

The 828-trial Mac controller (PID38647), audit helper (PID45773) and related PIDs are NOT running — the study was deliberately stopped before the cloud coordinator disconnected (stop-request.json + run-failed.json retained under the study root; artifacts and analysis/ kept). Do not resume that queue. Mac HEAD cead1801 and the 17-file uncommitted diff are intact.

Executed per HANDOFF_EVAL_2026-11-03.md (repo root). Current active work:

- T4 (ubuntu@43.132.189.162, /tmp/neomind-t4-eval-20261103-67vlq1cb): v2 two-arm builds complete and hash-verified; missing acceptance bootstrap reconstructed (neomind-t4-acceptance-bootstrap.py, all checks passed); **552-trial paired acceptance is RUNNING, controller PID 439563 (verify before acting), launched 2026-11-03 06:16:55Z** (acceptance-launch.json). Results accumulate under acceptance/. neomind.service stays paused during the run; the controller restores it on normal AND abnormal exit. Do not start overlapping T4 inference/builds; do not touch the two unrelated Docker GPU processes (PIDs 2725541, 3791136).
- Jetson (box@192.168.93.20, /tmp/neomind-linux-ci-20261103-msim9c25): cli-identity-v2.patch applied over the validated 15-file state, 17-file hashes equal revision-v2-input.json full; `cargo test --locked -p neomind-cli-ops --lib` running (PID 44024 at launch; see cli-identity-v2-launch.json / cli-identity-v2-test.log; note default toolchain resolves to 1.92.0 while the earlier 194-item run recorded 1.99.1).

Details, monitoring commands and remaining follow-ups: docs/development/mac-glm-takeover-2026-11-03.md. No commit/push/publish.

## 552 acceptance verdict and round-1 constraints (2026-11-03 late night, user directive)

Result: cli_only 192/276 (69.6%) vs full 206/276 (74.6%), paired delta +5.07pp, gained 33 / lost 19, zh+en both improved; case-cluster 95% [0.00, 10.51], cohort-cluster [-0.36, 10.87]. Measured improvement, NOT yet statistically confirmed — do not declare victory. Duplicate calls 734→326 supports overall behavior change only, no single-mechanism attribution.

Next-round rules (user):
1. Explain the 19 paired regressions first (done: see docs/development/mac-glm-takeover-2026-11-03.md forensics section; regression-forensics.json + guard quantification on T4). Pick a mechanism only from trace evidence.
2. Do NOT treat "pushing the CI lower bound above zero" as a tuning target; fix reproducible defects and validate with pre-determined INDEPENDENT acceptance; no repeated optimization on the same case set until significant.
3. "Expected-command checking before completion" is DEFERRED/rejected: completion must rest on user goals and actual state, never eval-preset command paths; honest timeout wrap-ups improve UX but unfinished tasks still count as failed.

Baseline freeze: full-v2 (binary sha 30c3cccf…7e8e6; source = the 17-file uncommitted Mac diff; T4 builds-v2.json hashes) is the baseline for the next round. Keep the 552 results intact under T4 acceptance/. Granite-4.2-3B is a SEPARATE baseline on Mac (/tmp/neomind-granite42-3b-20261103-1); never merge its numbers with Spark arms.

## Round-1 outcome (2026-11-04, rule-update contract fix; commits c37b4ab3 + uncommitted metadata hardening)

Rule-update contract fix RETAINED: unknown/no-op/wrong-type bodies -> structured 400 (whole request unwritten, rule+updated_at untouched); idempotent updates succeed with changed=false; metadata keys applied only in full-definition bodies (verified applied, not just whitelisted). neomind-api 605/0. 32-trial paired screen (Mac, Spark, baseline c7ebf999 vs candidate): 14/16 both arms, net 0 — contract fix correct, no eval gain claimed. multi-turn-correction 8/8 both arms switched to sensor-002 with ID retained, recovered from first failed update, zero false success. Open candidates (not fixed): visible-reply </think> marker leak (23 full vs 5 cli, evidence in T4 leakage-review.json); agent resurrection anomaly after successful delete (trial evidence vs correct plain-API repro; hypotheses H1/H2 in /tmp/neomind-screen-r1/NIGHT-LOG.md). Night log: /tmp/neomind-screen-r1/NIGHT-LOG.md. No 552 rerun this round per user budget directive.

## Iteration protocol after the 552 comparison (user directive, 2026-11-03)

After the full two-arm comparison completes, do NOT stop at the verdict. If there is no reliable improvement: analyze the dominant failure types from full traces (request, tool calls, raw results, expected state, final reply — scores alone do not explain failures). Each round changes exactly ONE well-defined mechanism. Paired screening first (affected failed cases plus normal-operation controls, zh/en, interleaved arms), then full regression before keeping a change. Retain effective changes, revert harmful ones, iterate. Hard constraints: NEVER modify scoring rules, relax timeouts, or delete/soften hard cases to manufacture gains; grader, case set, model/runtime/sampling stay frozen across arms. Follow docs/development/agent-eval-method.md throughout.

## Latest thinking review outcome (2026-11-04)

Read docs/development/thinking-readiness-result-2026-11-04.md before resuming.
Capability gating and its rollout switch were withdrawn from the worktree;
archive: /tmp/neomind-thinking-readiness/withdrawn-policy/. Retained changes are
parameter consistency and three recovery-channel fixes. The 8-case default-path
screen ended (baseline 1/4, candidate 0/4); no model-quality gain or leak cure is
claimed. Do not restart that queue. Raw trials and observation gaps are retained.
No commit/push/publish; follow the final source/build manifest in the artifact root.

## Scoped recovery fix committed with user authorization (2026-11-04)

User authorized tested leakage fixes to be committed. Commit 7ccbffa7 contains
only the three recovery-channel guards and their tests (two Rust files).
Isolated HEAD plus that patch passed 920 agent tests. No push or publication.
Model/server content misclassification is not fixed by that commit.
Native reasoning-budget and structured-response probes remain experimental;
do not infer acceptance from isolated transport tests or deploy their parameters.

## Image pipeline fixes validated (2026-11-04)

Read docs/development/image-pipeline-fixes-2026-11-04.md. Mac candidate fixes cache image extraction/selection, dynamic vision registration, extension input validation, and repeated-image-failure progress. Workspace gate 3737/0/69; zero-inference registration integration passed; one isolated Spark+MiniCPM task completed in 38.520s with 7 effective-success calls and one vision call, no API restart. Artifacts /tmp/neomind-image-fixes-v2/. Own API/tunnel exited; T4 production unchanged. Not a paired score/regression claim. Preserve existing unrelated llm changes. No commit/push/deploy/publish performed.

## Context meter and auto-compaction fix (2026-11-04)

Read docs/development/context-meter-auto-compaction-2026-11-04.md. Two independent
defects fixed in the worktree (no commit/push/publish). Frontend ring now shows only
the last measured model request (new per-request ContextUsage event); without a
measurement it shows a waiting state, never a transcript estimate; inconsistent
per-part estimates (system/tools) are hidden, the measured total is kept. Backend:
short sessions may summarize first-turn tool history while the context builder keeps
the last 3 user messages verbatim (still budgeted); context-builder truncation now
marks history_truncated so the summarizer's 60% threshold bypass actually fires;
ContextCompacted WS event toasts completion. Frontend vitest 340/340 + tsc clean;
workspace 3743/0; real-task replay (12 request snapshots, peak 24,188/32,768,
summary triggered then injected) under /tmp/neomind-context-fix/ with API/tunnel
cleanup confirmed. Not a paired score/regression claim; visible </think> leak
remains unfixed; unrelated llm/thinking/image worktree changes preserved.

## 1.2.6 release prep (2026-11-04, uncommitted)

CHANGELOG [1.2.6] entry written (covers all commits since the 1.2.5 gate plus
the worktree fixes) and version bumped to 1.2.6 across root Cargo.toml,
web/package.json(+lock), src-tauri Cargo.toml/tauri.conf.json/Cargo.lock; both
locks pass --locked. All CI gates verified locally: eslint error-level + warning
ratchet (473<=474), tsc, vitest 340/340, cargo fmt (fixed 4 committed files),
clippy --workspace --all-targets -D warnings (fixed 6 errors in
neomind-storage/agents.rs + 3 in neomind-agent stream_core tests), skill-cli
drift, llama pin sync. Extract_changelog full+short modes verified. Nothing
committed, no tag pushed; a release-gate eval round is a user decision per
agent-eval-method (552 acceptance measured the guards arm; image/context/thinking
fixes carry targeted tests + replay, no full regression).

## Builtin engine stop + exposed controls (2026-11-04, uncommitted)

User request. New POST /api/builtin-llm/stop: kills main+companion llama-server
WITHOUT deleting files, persists EngineOverrides.stopped (bootstrap AND watchdog
skip it; from_env applies persisted overrides via the startup-set overrides
path); any restart clears the flag and spawns (forced full restart, not
companion-only). Status reports deliberately_stopped. UI: Stop/Start buttons
(confirmed) + warning badge in the LLM tab; engine dialogs gained free-form ctx
(1024..1M, off-preset opens in custom mode) and ngl/vlm_ngl pin/auto (env/API
pins prefill; reset sets auto). Delete already existed (wizard + tab). apiSurface
ratchet 282->283. Gates: neomind-api clippy -D warnings OK, api lib 477/0
(builtin_llm 95/0 incl. 3 new stop tests), vitest 340/340, tsc, eslint, fmt OK.
Post-implementation review fixed two gaps (2026-11-05): all four post-download/
import/switch spawn sites now respect the stop flag (files + pointer kept,
no spawn; download completion re-reads from_env to close the DL_ACTIVE-drop
race), and stop gained the runtime-upgrade 409 guard mirroring restart.
Review round 2 (2026-11-05) fixed two more: the runtime-upgrade respawn
(restart_builtin_engine) also respects the stop flag now (runtime applied,
engine stays down), and the model wizard skips its 90s "starting" window when
deliberately_stopped (lands on ready with the start button directly).

## T4 138 gate: 1.2.6 candidate vs 1.2.5 (2026-11-05)

Single-arm frozen gate on T4 (/tmp/neomind-t4-126gate-20261105-a; binary sha
715e2dec…a9703 = the full Mac worktree incl. engine-stop and all 1.2.6 fixes,
built cargo build --release --locked -p neomind-cli on T4). Frozen 138 = 69
ids x zh/en, 1 round, same shuffle seed as 552 round 0; frozen evaluator/
grader reused from the 552 SNAP + venv (source-sha256 asserts passed); Spark
q4_k_m + b12024-cuda, fresh llama-server per cohort (32K, 1 slot, -ngl 99),
thinking/.3/.75/20, 600s cap; proxy-verified live request params. Result:
94/138 = 68.1% (en 69.6%, zh 66.7%), 0 timeouts, 125.4 min. vs 1.2.5 official
gate 69/138 (50.0%): +25 cases (+18.1pp), far beyond single-round noise. vs
552 full-arm rounds (99/138 r0, 107/138 r1): 94 is below both (net -5/-13);
post-552 fixes show no measurable gain on this text-only set (multimodal
skipped by design) and the gap is near the full arm's own round-to-round
spread (8) — claim neither regression nor gain vs the full arm without another
round. neomind.service paused per contract, auto-restored on completion
(restoration-result rc0, API 200, both production llama-servers back). Local
baseline copy + analysis: /tmp/neomind-126-t4-stage/. No commit/push/publish.
CHANGELOG 1.2.6 gained "Added — builtin engine controls". Deliberately-stopped
chats fail with connection errors by design; no auto-restart until user acts.
No commit/push/publish.
