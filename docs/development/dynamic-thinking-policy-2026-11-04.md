# Dynamic thinking policy audit (2026-11-04)

Base: ab21f800. This change retains dynamic thinking shutdown. It does not
restore the withdrawn always-inherit candidate, parse think markers, or add
reasoning_format. No model score improvement or leak elimination is claimed.

## Decisions

- Initial chat: session override, then bound backend settings; effort takes
  precedence over the legacy bool. Unset remains unset.
- Ordinary tool followup: retain the original local-setting trigger; the
  broader backend-derived trigger was removed after screening.
- Large text-stream prompt (>18000 estimated tokens): retain shutdown policy,
  resolving bool and effort together.
- Summary and empty-answer recovery: retain explicit shutdown.
- Context-overflow recovery: send explicit shutdown, not unset defaults.
- Integral-thinking builtin: ignore shutdown overrides. Use session-bound
  metadata first, then the interface manager, then legacy global fallback.
- Text and multimodal requests share effective bool/effort normalization.

## Validation and limits

Agent lib with test-utils,llamacpp: 922 passed, 0 failed before final log-only
edits. New request tests cover large prompts, setting-source equivalence,
summary/integral behavior, actual streamed context-overflow recovery, effort
precedence, session overrides and unset defaults.
Full workspace gate and logs: /tmp/neomind-dynamic-thinking-audit/.

Explicit shutdown still cannot guarantee model output obeys the template.
Isolated closing think tags in content remain an unresolved output-protocol
issue. Model latency and task success need separate bounded paired acceptance.

Final workspace gate: {"exit_code": 0, "suites": 95, "passed": 3727, "failed": 0, "ignored": 69}.

## Post-screen scope correction

The eight-trial screen did not establish a model-quality gain. The change
that broadened tool-followup shutdown from local to backend-derived settings
was removed. Original followup trigger remains. Bool/effort consistency,
explicit overflow retry shutdown and bound-backend integral metadata remain
as correctness changes; no latency or score benefit is claimed.

Separate recovery channel fix and verification are documented in
summary-recovery-channels-2026-11-04.md.

The capability-gate experiment was withdrawn after screening; see thinking-readiness-result-2026-11-04.md.
