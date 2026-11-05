# Thinking-leak fix round (leakfix-r2) — 2026-11-05

## Problem

Dynamic thinking toggle on Spark-X2.6-1.7B leaked reasoning into the visible reply:
a stray `</think>` (opening tag consumed by the chat template / never emitted) plus
the reasoning monologue before it arrived in the `content` channel, flowed verbatim
to the UI, persisted into `AgentMessage.content`, and replayed into later requests
(self-reinforcing). Evidence: 552 acceptance leakage-review (23 full vs 5 cli, all
verified true leaks, pattern "reasoning tail + </think> + body"); leak concentrated
on `enable_thinking=false` auxiliary requests in long trajectories.

## Root cause (verified against code + live capture)

- `LlamaCppRuntime` forwarded `content` deltas verbatim (no marker handling,
  stream and non-stream paths).
- llama-server only routes BALANCED `<think>...</think>` spans to
  `reasoning_content`; an unmatched closer and its prefix stay in `content`.
- 7ccbffa7 guarded three TEXT recovery channels only; the four multimodal
  consumers and the backend content channel were unguarded.

## Fix (one eval-attributed mechanism + deterministic completions)

1. Mechanism A (eval-attributed): `crates/neomind-agent/src/llm_backends/backends/think_filter.rs`
   content-channel state machine inside `LlamaCppRuntime` (stream + non-stream).
   Balanced blocks in content → thinking channel; stray closer within
   `GRACE_CHARS = 2048` of content start → prefix+marker reclassified to
   thinking; stray markers after grace → marker stripped (counted); unclosed
   `<think>` at finish → tail to thinking. Clean streams pass through
   byte-identical; only ≤7-char potential-marker suffixes are held back at
   chunk boundaries; UTF-8 safe; per-response repair logs a
   `think_filter_strip` warn line for acceptance evidence.
2. Mechanism B (deterministic, not eval-attributed): the four stream_multimodal
   consumers now drop `is_thinking` chunks (same pattern as 7ccbffa7):
   continuation buffer, main summary, nudge buffer, post-nudge summary.
3. In passing: llm.rs threshold-fn doc-comment placement restored; pre-existing
   `cargo fmt` drift fixed in extension_tools.rs + builtin_llm/handlers.rs;
   pre-existing failing api_docs gate fixed (missing `POST /api/builtin-llm/stop`
   ROUTES entry from the engine-stop round).

Baseline = HEAD 59ac1b78 (binary sha bb496d0f…); candidate = same + this fix
(binary sha 7d9d8028…). Pre-registration and all artifacts:
`/tmp/neomind-leakfix-r2/` (acceptance.json written before any capture/eval run).

## Phase 0 evidence capture

Baseline ran guided-mqtt-onboarding zh+en behind a wire-context proxy: 29 wire
requests, 9 marked. ALL 9 had `enable_thinking=false` and NO `<think>` opener in
content (pure stray-closer pattern). Prefix lengths 172–1078 chars. Pre-registered
rule → G=2048 (1024 covered only 8/9 = 0.89). En trial reproduced a persisted
leak; zh trial was persisted-clean but had 8 wire-marked requests — confirming
the wire-vs-persisted observation gap (live-event checking is mandatory).

## Phase 2 deterministic gates

- think_filter unit tests 11/11 (cross-chunk marker splits, UTF-8 boundaries,
  grace-budget boundary, byte-identical passthrough, unclosed-open, multi-closer).
- llamacpp integration tests 2/2 (mock SSE stray-closer split across deltas;
  non-stream repair merged with reasoning_content).
- multimodal guard tests 4/4 (nudge_tests::multimodal_recovery_guards).
- cargo fmt --check clean; clippy --workspace --all-targets -D warnings clean;
  cargo test --workspace --locked all green (neomind-agent lib 953/0 with
  test-utils+llamacpp); web untouched (340/340 baseline unchanged).

## Phase 3a smoke — PASS (all four pre-registered criteria)

| trial | arm | pass | leak_turns | live markers | wire-marked | filter fires |
|---|---|---|---|---|---|---|
| gmo zh | candidate | false | [] | 0 | 13 | 13 |
| gmo zh | baseline | false | [0] | 4 | 4 | 0 |
| gmo en | baseline | true | [0] | 7 | 7 | 0 |
| gmo en | candidate | false | [] | 0 | 0 | 0 |

The zh candidate trial is the mechanism proof: 13 server-side misclassifications
on the wire, 13 repairs, zero escapes. The en candidate failure was investigated:
`cmd_ok=false` (model answered from cached info without running the expected
commands), zero filter activity in that trial, replies complete — ordinary
stochastic failure, same class as baseline zh's.

## Phase 3b paired screen — cut at 6/32 by user decision (太久了)

Completed subset (guided-mqtt-onboarding zh+en, 3 trials/arm, interleaved,
fresh llama-server per unit, hash-asserted binaries):

- candidate: 0/3 leak trials; 38 wire-marked requests → 38 filter fires →
  0 live markers → 0 persisted leaks.
- baseline: 3/3 leak trials; 35 wire-marked requests → 35 live markers
  (100% passthrough), one also persisted.
- Pass rate on this flaky leak-hit case: candidate 0/3 vs baseline 1/3 — no
  conclusion either way at n=3 (investigated failure had complete replies and
  zero filter activity). The full statistical non-regression check was NOT run.

## Independent-case check (readiness8) — PASS, 2026-11-05 later

User-requested re-run of the thinking-readiness round's 8-trial default-path set
(command-discovery + building-bms-solution-deploy, zh/en, 1 round, 2 arms,
interleaved, fresh llama-server per unit, same frozen binaries; pre-registered
in /tmp/neomind-leakfix-r2/readiness8/acceptance.json). These cases are
independent of the tuning case. Result (26 min):

| unit | baseline: wire→live | candidate: wire→fires→live |
|---|---|---|
| command-discovery en | 1→1 | 0→0→0 |
| command-discovery zh | 1→1 | 0→0→0 |
| bms-solution-deploy en | 4→4 | 10→10→0 |
| bms-solution-deploy zh | 5→5 | 2→2→0 |

Candidate: 0 persisted leaks, 0 live markers, 12 repairs with 0 escapes.
Baseline: 11 wire-marked requests → 11 live leak events (100% passthrough,
matching every earlier measurement). Pass-rate pairs (secondary, n=1 each,
no claim): candidate ≥ baseline in all 4 units (command-discovery flipped to
pass on both languages; bms failed on both arms). Cumulative across all runs:
candidate 63 repairs / 0 escapes; baseline 57/57 marked requests leaked live.

## Hardening round (same day, user-directed review)

User review flagged two uncovered risks; both were accepted with these
boundaries and implemented as narrowings of the same mechanism:

1. **Code-context literalness.** Markers inside markdown code context (`` `
inline ` ``, ``` fences, ~~~ fences) are now literal text — never protocol.
The CodeState walker spans chunk boundaries (backticks and markers split
across deltas), fences close on a line-leading run of >= the opening length,
inline spans close on an exact-length backtick run and are abandoned at a
blank line (one stray backtick cannot disable repair for the rest of a
reply). Corpus justification: 26/26 observed leaks sat in plain prose.
**Documented residual (pinned by test, NOT solved):** a marker mentioned in
PLAIN prose still swallows the preceding answer text into the thinking
channel.
2. **Conditional grace hold.** The opening-run hold activates only when the
   request's FINAL SENT `chat_template_kwargs.enable_thinking == false`
   (the leak mode: 26/26 corpus). Thinking-on requests stream content with
   only the <=7-char marker-prefix holdback; balanced blocks and unclosed
   opens remain repaired without any hold.

First-chunk behavior now has DIRECT deterministic evidence at NeoMind's own
emission boundary (paced mock SSE, three scenarios):
`thinking_on_short_reply_streams_before_completion`,
`thinking_off_short_reply_holds_until_completion`,
`thinking_off_long_reply_releases_after_grace_budget`.

Gates: 23 think_filter unit tests, 16 llamacpp tests (3 timing), 968 agent
lib, workspace green, clippy/fmt clean. Mechanism regression (smoke2,
pre-registered mechanism-only check, candidate-v2 sha 875831db…): candidate
29 wire-marked -> 29 repairs -> 0 live markers, 0 persisted leaks; baseline
22 wire-marked -> 22 live leaks (zh also persisted). Claim wording per user:
**"reduces measured leaks and reduces part of the streaming impact"** — the
plain-prose false-positive case remains a documented residual until direct
evidence exists.

## Verdict (claim template)

Leak eliminated on every measured surface: deterministic tests, smoke (4/4
criteria), partial paired screen. Across all runs the candidate performed 51
repairs with 0 escapes while every measured baseline trial leaked. No eval
score gain is claimed; statistical pass-rate non-regression is not established
(screen cut by user decision). Nothing committed/pushed.

## Known limitations / follow-ups

- Custom OpenAI-compatible backends pointed at a local llama-server do not get
  the filter (the Spark builtin path is llamacpp).
- GRACE_CHARS=2048 delays the first ~2048 content chars of every llama.cpp
  response by design (bounded; the reasoning channel streams unheld).
- detect_thinking still misclassifies `sparkx2.6-1.7b` (supports_thinking via
  /props writeback) and mid-session effort PATCHes without backend_id keep a
  stale snapshot — separate rounds, deliberately not bundled here.
- Wire-level markers on the SERVER side persist by design (the server still
  misclassifies); acceptance is defined on NeoMind's emitted events.
