# Development coordination

Read CLAUDE.md and the relevant module documentation before changes.

Current work is coordinated by Codex (2026-11-02): summary preservation, default chat loop exits, and skill search isolation. Do not start overlapping eval chains or edit these same files concurrently. Keep existing run artifacts; use unique output directories.

Use Spark-1.7B for primary acceptance with fixed model/runtime/sampling settings. Keep hardware-specific baselines separate.

Do not put credentials in source or logs. Do not commit, push, deploy, or publish without an explicit user instruction.

Follow the [agent evaluation method](docs/development/agent-eval-method.md) for ongoing improvements.

## Active comprehensive evaluation (2026-11-03)

A frozen three-arm full-regression study is running. Root /tmp/neomind-comprehensive-20261103-085034, official data main/, controller /tmp/neomind-comprehensive-single-run.py PID38647 (verify before acting). 69 IDs, both languages, two rounds, three Release versions =828 trials. Fixed Spark32K single-slot, thinking enabled, .3/.75/20, 600s case cap. Do not start overlapping inference/builds or revive older queues. Four-slot root data is a capacity pilot, not official results. Binaries/evaluator/fixtures/grader are frozen; live workspace source remains untouched. External GLM export of evaluation answers/evidence for semantic judging was rejected by automatic approval review; sender was not run. Do not bypass it. Use local task/state and trace metrics; do not claim universal factuality judging. No commit, push, deploy or publish.

Comprehensive local audit final helper: PID45773 (verify), analysis/ archived with manifest, 7 audit tests and 6 pairing tests pass. Native hard signals are independently recomputed from frozen case+record, not just copied. Final comprehensive-report.md is emitted only after all828 and hash checks pass. Pilot orphan API processes were cleaned; official controller PID38647 remains active.

## Takeover by Mac-local GLM (2026-11-03, ~14:10) — corrects the stale process notes above

The 828-trial Mac controller (PID38647), audit helper (PID45773) and related PIDs are NOT running — the study was deliberately stopped before the cloud coordinator disconnected (stop-request.json + run-failed.json retained under the study root; artifacts and analysis/ kept). Do not resume that queue. Mac HEAD cead1801 and the 17-file uncommitted diff are intact.

Executed per HANDOFF_EVAL_2026-11-03.md (repo root). Current active work:

- T4 (ubuntu@43.132.189.162, /tmp/neomind-t4-eval-20261103-67vlq1cb): v2 two-arm builds complete and hash-verified; missing acceptance bootstrap reconstructed (neomind-t4-acceptance-bootstrap.py, all checks passed); **552-trial paired acceptance is RUNNING, controller PID 439563 (verify before acting), launched 2026-11-03 06:16:55Z** (acceptance-launch.json). Results accumulate under acceptance/. neomind.service stays paused during the run; the controller restores it on normal AND abnormal exit. Do not start overlapping T4 inference/builds; do not touch the two unrelated Docker GPU processes (PIDs 2725541, 3791136).
- Jetson (box@192.168.93.20, /tmp/neomind-linux-ci-20261103-msim9c25): cli-identity-v2.patch applied over the validated 15-file state, 17-file hashes equal revision-v2-input.json full; `cargo test --locked -p neomind-cli-ops --lib` running (PID 44024 at launch; see cli-identity-v2-launch.json / cli-identity-v2-test.log; note default toolchain resolves to 1.92.0 while the earlier 194-item run recorded 1.99.1).

Details were recorded in the 2026-11-03 takeover handoff (since removed with the other superseded process docs); that workstream is concluded and committed. No push.

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

Image pipeline fixes (2026-11-04, committed): Mac candidate fixes cache image extraction/selection, dynamic vision registration, extension input validation, and repeated-image-failure progress. Workspace gate 3737/0/69; zero-inference registration integration passed; one isolated Spark+MiniCPM task completed in 38.520s with 7 effective-success calls and one vision call, no API restart. Artifacts /tmp/neomind-image-fixes-v2/. Own API/tunnel exited; T4 production unchanged. Not a paired score/regression claim. Preserve existing unrelated llm changes. No commit/push/deploy/publish performed.

## Context meter and auto-compaction fix (2026-11-04)

Context meter and auto-compaction fix (2026-11-04, committed): Two independent
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

## Thinking-leak fix round leakfix-r2 (2026-11-05, uncommitted)

Thinking-leak fix round leakfix-r2 (2026-11-05, committed): Spark-X2.6 dynamic-toggle
leak fixed at the llama.cpp backend boundary: new think_filter.rs state machine
(stream + non-stream) reclassifies stray-closer prefixes (GRACE_CHARS=2048,
chosen by pre-registered rule from a 9-marked-request capture corpus; all
thinking=false, no opener in content) and strips later stray markers; clean
streams pass byte-identical; repairs log `think_filter_strip`. Plus the four
stream_multimodal is_thinking guards (7ccbffa7 pattern) and three in-passing
repairs (llm.rs doc-comment placement, fmt drift in extension_tools.rs +
builtin_llm/handlers.rs, missing POST /api/builtin-llm/stop ROUTES entry that
made the api_docs gate fail on HEAD). Gates: agent lib 953/0, workspace green,
clippy/fmt clean, web untouched. Acceptance (pre-registered
/tmp/neomind-leakfix-r2/acceptance.json; base 59ac1b78=bb496d0f vs cand
7d9d8028): smoke 4/4 criteria (candidate 13 wire-marked -> 13 repairs -> 0
live/persisted leaks; baseline reproduced both languages); 32-trial screen CUT
AT 6/32 by user decision (太久了) — partial subset: candidate 0/3 leak trials,
38 repairs 0 escapes; baseline 3/3 leaked (35 wire-marked -> 35 live markers,
100% passthrough). Combined: 51 repairs, 0 escapes. Claim: leak eliminated on
measured surfaces; NO score gain claimed; statistical pass-rate non-regression
NOT established (screen cut). Independent-case check (user-requested,
readiness8, pre-registered /tmp/neomind-leakfix-r2/readiness8/): the
thinking-readiness 8-trial set re-run with the leakfix arms — candidate 0 leak
trials / 12 repairs 0 escapes; baseline 11 wire-marked -> 11 live leaks;
pass-rate pairs candidate >= baseline in all 4 units (no claim at n=1).
Cumulative: candidate 63 repairs / 0 escapes; baseline 57/57 marked requests
leaked live. Hardening round (same day, user-directed review): markers in
markdown code context (fence/inline, spanning chunks, blank-line inline
reset) are now literal text; the grace hold activates only when the FINAL
SENT enable_thinking==false, so thinking-on replies stream with only the
<=7-char holdback. Direct first-chunk evidence added (3 paced timing tests
at NeoMind's emission boundary). smoke2 mechanism regression PASS
(candidate-v2 875831db: 29 wire-marked -> 29 repairs -> 0 live/0 persisted;
baseline 22/22 live leaks). Claim wording per user: "reduces measured leaks
and reduces part of the streaming impact"; residuals on record: plain-prose
marker mentions still swallow the preceding answer text (pinned by test,
unsolved), thinking-on stray-closer prefixes stripped-only, prefixes >2048
partially visible. Frozen binaries preserved at /tmp/neomind-leakfix-r2/bin/
(baseline bb496d0f, candidate-v1 7d9d8028, candidate-v2 875831db); disk-full
incident resolved by deleting the session's own target dirs after moving
binaries out (9.5GB freed, user disk now ~29GB free). Known follow-ups
(deliberately not bundled):
detect_thinking misses sparkx2.6-1.7b; mid-session effort PATCH without
backend_id keeps a stale snapshot. No commit/push/publish.

## On-demand chat memory (2026-11-05, committed with user authorization)

Read docs/development/on-demand-chat-memory-2026-11-05.md. User authorized removing
implicit long-term memory injection while keeping explicit memory-tool access.
Chat-turn extraction now defaults off (auto_extract=false). Current conversation
history/compaction and scheduled-agent memory are unchanged. Test artifacts:
/tmp/neomind-memory-demand-20261105/. Takeover round (same evening): workspace gate
+ clippy verified, and live-use tool gaps fixed (memory rewrite action, 200-char
list previews, file tools barred from <data_dir>/memory). The unrelated
model-override hunk in capability_providers.rs stays uncommitted. No push.
(Note 2026-11-08: that hunk landed same evening as baf22a02 and shipped in 1.2.7.)

## 1.2.8 round (2026-11-08, user-approved checklist)

Scope approved by the user ("做吧" on the reviewed list): mobile commits already
on main (composer tap-to-focus, collapsed-actions alignment), eval failure
taxonomy (committed ee55fd2f), detect_thinking sparkx2.6 family fix, effort-PATCH
stale-snapshot fix (snapshots record backend_id; PATCH retunes exactly the
sessions bound to the edited non-active backend), LiteLLM registry refresh to
2026-11-08 snapshot (4871 keys; Claude-3 alias chains gained bedrock-style
fallbacks after upstream pruned the bare dated keys; gemini-1.5/2.0 vision moved
to heuristic_vision_match after upstream deprecated the lineage and stripped
flags), and messages-page live update (frontend-only: MessageCreated over the
existing events WS, 1.5s-coalesced refetch; no backend change needed — the WS
bridge already forwards all event types). Version bumped 1.2.8 across the five
files + both locks (--locked verified); CHANGELOG [1.2.8] written and
extract_changelog both modes verified. All local gates green: eslint errors 0 /
warnings 470 = baseline, tsc, vitest 351/351, fmt, clippy -D warnings,
cargo test --workspace 94 groups 0 failed (incl. new B1/B2/live-refresh tests),
skill-cli drift, llama pin sync b12024. The 1.2.8 release commit is NOT made;
no push/tag.

User decisions: B3 model-override hunk — moot (already committed baf22a02).
D1/D2 — no action (probes stay experimental; web_search keeps the
English-keyword mitigation). D3 deferred to 1.2.9 by design: leak residuals
(plain-prose marker swallow, thinking-on stray-closer stripped-only, >2048
prefixes), OpenAI-compatible backend think filter, model/server content
misclassification — one mechanism per round per the iteration protocol.

T4 138 gate (approved in the same checklist): root
/tmp/neomind-t4-128gate-20261108-a prepared and source shipped (tarball sha
4e9e4a8a…, Mac HEAD ee55fd2f + 1.2.8 worktree); binary built and prelaunch
verified (sha 7aba80e6…95284; model/runtime shas match the frozen asserts).
An active multi-arm study occupies T4 (batch-controller PID 2835094, root
/tmp/neomind-t4-16refresh-20261106-a, ~930 trials done at discovery) — nothing
of it was touched. At the user's question the T4 auto-launcher was DISARMED;
the user then redirected validation to the Mac ("就用mac"), and later cut the
full run for time ("太久了吧 验证"). T4 gate root stays prepared but
UNLAUNCHED — launching it later still requires an explicit user go.

## 1.2.8 Mac paired screen (2026-11-08 night, user-directed)

Mac-local two-arm paired screen replacing the T4 gate: /tmp/neomind-128-mac/
(acceptance.json has the full pre-registration + result). Arms = ee55fd2f
(v1.2.7, bin 12a4a9d6…) vs 1.2.8 worktree (bin 16799a34…), frozen 138 case
set + digests from the T4 552 plan (all verified locally), T4 552 SNAP
evaluator pulled byte-identical (453 source_sha256 asserts pass), same Spark
model file (sha 7abd9859… = T4's), b12024 Mac runtime, .3/.75/20,
thinking=true, 600s cap, llama-server restart per cohort, arm order
alternates. Full 276-trial run launched first, CUT at 15 trials by user
("太久了吧") and rescoped to a 32-trial screen (first 16 shuffled cases × 2
arms, seed 20261103; the 15 finished trials reused verbatim). Screen result:
baseline 12/16 vs candidate 11/16 (0 gained / 1 lost; no signal at n=16).

## 1.2.8 Mac FULL paired regression (2026-11-08 night → 2026-11-09 morning)

User then ordered the full run overnight ("今晚mac跑全量看看"). Same frozen
design, all 276 trials (the 32 screen trials reused; ~9.3 h wall; caffeinate
pinned to the controller; production desktop app untouched; controller and
eval llama-server exited clean). RESULT: baseline 97/138 (70.3%) vs
candidate 89/138 (64.5%), paired delta -5.80pp; 11 gained / 19 lost
(both-pass 78, both-fail 30); exact McNemar two-sided p = 0.2005 — a
MEASURED regression, NOT statistically confirmed. The lean is English-heavy
(en 51→44; zh 46→45) and clusters in device-onboarding and agent-family
cases; agent_error 6→10; 0 timeouts both arms. Wire: sampling rules
identical across arms; candidate max prompt_chars 61.6K vs baseline 72.0K —
the detect_thinking sparkx2.6 65%-vs-70% prompt-budget mechanism is live and
is the eval-visible delta (B2/B4/C1 off-path). Per the iteration protocol
this is evidence against keeping B1's budget mechanism as-is: options
recorded for the user — revert B1 for 1.2.8 (ship B2/B4/C1+mobile), or split
B1 (capability truth yes, budget-ratio change no) as its own next round.
Full verdict in /tmp/neomind-128-mac/acceptance.json (full_run_result). No
commit/push/publish; 1.2.8 release decision pending this result.

USER DECISION (2026-11-09 morning, "回退 B1 整体"): B1 was reverted ENTIRE
from the 1.2.8 worktree (detect_thinking sparkx2.6 heuristic + its tests);
CHANGELOG documents the withdrawal with the measured evidence ("Dropped by
decision"). Final 1.2.8 changeset = B2 + B4 + C1 + mobile commits + eval
taxonomy (committed ee55fd2f) + version bump. Rust gates re-run green after
the revert. sparkx2.6 stays classified non-thinking by the name heuristic;
if the capability-truth half ever matters, it returns as its own round
decoupled from the prompt-budget table.

Post-revert spot check (user-requested, /tmp/neomind-128-mac/run-lost/):
the 19 lost cases re-run on the post-revert binary (sha 6dacc3ff…) — 14/19
recovered. The 5 unrecovered are heavy onboarding cases (two at the 600s
cap); a baseline-binary control re-run passed only 1/3 of them before the
user cut it ("没必要浪费时间了") — case-level pass/fail flakiness, not a
residual defect. Verification accepted by user; artifacts under
run-lost/ + run-lost-base/. Also folded into 1.2.8 (user request): the
extensions page status filter pills removed (search kept; orphaned locale
keys cleaned, i18n frozen baseline 250→249, lint ratchet still 470, vitest
351/351, tsc 0). All Mac eval controllers/llama-servers exited clean.
