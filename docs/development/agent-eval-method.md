# Agent improvement loop

Read the latest codex-handoff-2026-11-02.md before work. Check current source and processes; do not resume historical queues. Mac is the main working repository. Keep credentials out of files, reports and logs.

Start from one observable failure: original request, actual tool calls, raw results, expected state, and final reply. Scores alone do not explain a failure. Check implementation and documentation before changing device schemas or adding behavior rules.

Freeze the baseline and candidate scope, model/runtime/settings, cases, grading and timeout. Save source patches and binary/model/case hashes. Use unique run directories; preserve cases.jsonl before scoring. Build and run inference sequentially. Prefer a small affected case plus a normal-operation control, zh/en with two repetitions and interleaved arms.

Review task state, command errors, calls, latency and factual grounding separately. Correct commands do not prove the final diagnosis is correct. Do not credit an unexercised new branch for a gain, or describe a small targeted screen as full-regression improvement. Keep failed and interrupted trials with their reasons.

Withdraw unhelpful prompt expansions rather than accumulating them. Retain deterministic evidence-preservation fixes when backed by meaningful tests. Run checks for the final retained source; a green earlier experiment does not validate later code. Update the handoff with retained/withdrawn changes, exact artifacts and remaining problems.

Stop this experiment's dedicated inference/queue at completion and retain raw records. Do not commit, push, deploy or publish without an explicit user instruction.

When restoring Rust backups, refresh source mtime after copy2-style restores. Cargo can otherwise reuse candidate artifacts. Verify the rebuild and actual added/removed test counts, not only exit0; compare restored source diff and matching binary hashes. An earlier cached green is not restoration evidence.
