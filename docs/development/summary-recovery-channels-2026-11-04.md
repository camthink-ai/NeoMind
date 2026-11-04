# Recovery stream channel handling

Base: ab21f800. Three recovery consumers ignored the is_thinking flag and
converted reasoning into visible content: failed-followup summary,
incomplete-tool-JSON summary, and empty-answer retry.

The fix excludes reasoning chunks from content events and response buffers,
matching the existing ordinary final-summary behavior. It does not interpret
text markers, buffer normal output, or change backend request parameters.

Validation: three deterministic tests each failed on the original consumers
with PRIVATE_RECOVERY_REASONING in Content; all passed after the fix. Tests
verify public text survives, literal </think> in normal text is preserved,
and stored message content contains no reasoning. Exact final-message checks
cover incomplete-JSON and empty-answer recovery; failed-followup summary has
an existing event-only path, whose persistence semantics are not changed here.
Final agent test-utils,llamacpp gate: 924 passed, 0 failed.
Logs, red/green proof and patches: /tmp/neomind-summary-channel-fix/.

This does not fix reasoning already mislabeled as content by the model/server.
The English BMS screen has marked content in ordinary tool-call responses,
so that incident cannot be attributed to these recovery consumers.
No model evaluations or score changes are claimed for this fix.

Final workspace gate: {"exit_code": 0, "suites": 95, "passed": 3729, "failed": 0, "ignored": 69}.
