# Development handoff — 2026-11-02

Primary checkout: this Mac repository; base commit cead1801. Changes remain uncommitted. Do not restart the historical attribution queue or overlap edits without coordinating.

Artifacts and original source backup: `/tmp/neomind-takeover-20261102-141551`.

Implemented: preserve old memory in folded/fallback summaries; token-budget and offline/manual compact handling; stateless structured skill search; turn-local guide exploration budget with mixed-progress and batch-order protection; grounded natural LLM replies on stalled exploration; separate thinking from visible summary; fixed text only as last resort.

Latest validation: `cargo test --workspace --locked --features neomind-agent/test-utils` passed, 3683 tests, 0 failed, 69 ignored. An earlier run failed one unchanged companion bootstrap test; its component rerun and the subsequent complete run passed. Logs retained; do not erase the failed attempt.

Spark-1.7B single-run checks: protocol question 5 tool rounds, natural Chinese request for exact model/docs; normal device list 1 round. Onboarding question 2 rounds but ordinary final answer still mislabeled ne101_camera as Xiaomi and replied in English. This is NOT accepted as correct. No full same-model before/after score comparison has been run.

Next: entity/manufacturer evidence and language retention in normal answers; concurrent summary protection; context-meter/UI work. Do not call the whole incident fixed or publish based on this batch.

No commit, push, deployment or release is authorized. Never store credentials in this repository.

## Follow-up: scoped guide recovery (2026-11-02)

Do not resume the old attribution queue. CLI-hint screening in `/tmp/neomind-eval-triage-20261102-153704` was stopped after a shared-baseline problem appeared: a webhook request repeatedly loaded connector guides, then the overly device-specific summary brief requested irrelevant device information. Completed trials remain evidence, not an eval improvement claim.

Current code adds one request with skill excluded after the per-turn guide budget is exhausted. Other tools remain available, so the model can perform and verify the original task. Reject unadvertised skill attempts on that request; do not mutate global tool definitions. Remove repeated guide payloads from that request's historical previews; preserve user messages, other tool facts, and stored history. Generic final generation is now task-neutral and still uses the LLM.

Agent lib: 893 passed. Workspace gate currently running; release build follows only on success. Experiment directory: `/tmp/neomind-guide-recovery-20261102-175359`; queued helper: `/tmp/neomind-recovery-after-gate.py` PID 73314. A/B uses two zh cases, two repetitions, frozen Spark runtime/model/config, with CLI fix present in BOTH binaries. No broad eval gain or production acceptance has been established. No commits, pushes, deployment, or release.

Follow-up completed: workspace 95 groups / 3687 passed / 0 failed / 69 ignored; release build and diff check passed. Targeted Spark A/B: baseline 3/4, candidate 4/4 (webhook 1/2 vs 2/2, device rename 2/2 each). NO candidate run fired skill_recovery, so this is smoke evidence, NOT causal eval gain. Candidate webhook still tried connector commands before push; inspect skill semantic coverage next. Artifacts, source patches, binaries and hashes: `/tmp/neomind-guide-recovery-20261102-175359`, including review.json. Dedicated inference and helper queue exited. No full regression acceptance or publishing.

## Connector feedback follow-up (2026-11-02)

Boundary-only prompt pilot `/tmp/neomind-routing-20261102-183410` stopped after 3 completed records to prioritize a deterministic bug found in its traces; no statistical gain claim, no default boundary injection shipped. Baseline created an MQTT connector for a push request and falsely reported push verification.

`cli-ops/connector.rs` previously ignored connector_type when posting to /brokers but echoed the requested type in success text. Now unsupported types fail BEFORE API access, with a structured purpose-based recovery hint. Default/explicit mqtt keep the existing broker operation. Updated CLI help + connector guide, and corrected push guide defaults (webhook type default; enabled=true; distinguish list/get from live delivery test). Exactly four new files changed relative to the previous candidate, no schema redesign or automatic command rerouting.

Two targeted tests passed: unsupported types do not contact an API; default/explicit MQTT still post the original broker request to a local mock. Full workspace gate + release build in `/tmp/neomind-connector-feedback-gate.sh` PID 85581, queued eval helper PID 87140. New experiment `/tmp/neomind-connector-feedback-20261102-184142`: fixed Spark settings, webhook case zh/en twice per version (8 records). No boundary override; prior candidate binary is the baseline. Do not start overlapping builds/evaluations. No commits, pushes, or publishing.

Connector follow-up completed: full CI gate 95 groups / 3689 passed / 0 failed / 69 ignored, release build and final diff check passed. Real CLI rejection smoke returns success=false, UNSUPPORTED_CONNECTOR_TYPE, exit 3. Fixed Spark webhook zh/en two repetitions: baseline 3/4, candidate 4/4; first shell domain push 0/4 vs 4/4; total shell calls 25 vs 8; rejected commands 8 vs 0; mean time including failures 89.56s vs 27.29s. Candidate always ran only push create + push list and state checks passed. First zh candidate read the corrected connector guide, then loaded data-push guide and executed correctly. Real model trials did NOT hit the type-rejection branch, so do not attribute these gains to that branch; its correctness is separately tested. Single-case targeted evidence, no full-regression gain claim. review-summary.json, trace-review.json, raw cases/scores, hashes, final-source.patch retained in the experiment directory. Dedicated inference and queue exited; no old queue resumed. No commit/push/publish.

## Result-source follow-up (2026-11-02)

Current scope: three files relative to previous connector-feedback candidate: agent/types.rs (model-facing shell result source label, bounded prefix, payload unchanged), streaming/dedup.rs (command + optional working_dir identity; preserve unkeyed/late-differing evidence), toolkit/shell.rs (include explicit working_dir metadata; correct stale domain index claiming webhook/HTTP connector support). Six new tests; agent lib 899 passed. Do not infer that this fixes all false completion claims or all compaction behavior. Malformed/legacy result payloads retain their old headers.

Artifacts `/tmp/neomind-result-source-20261102-194828`; baseline binary and sources preserved. Gate PID 12914; queued eval helper PID 14648. Full CI then release before inference. Eval selects device-offline-no-data + diagnose-device-status, zh/en each twice and both versions (16 runs), frozen Spark config, prior fixes in both arms. No scoring changes or fixture changes. Do not start overlapping builds or inference. No commit/push/publish.

Result-source pilot completed 16 records: both arms 4/8, all no-data cases missed history. No demonstrated gain from extra source headers. types.rs has been restored from its own pre-change backup (including removal of the three feature-specific tests); retain the deterministic dedup fix and shell cwd/index correction. Raw results and tested-source.patch/decision.json are preserved. Do NOT reapply the withdrawn header experiment.

Next isolated item: device get now uses the existing suggestion field to explain current status vs history. Code confirms last_seen initialized at registration and transport_changed_at starts at 0. A false snapshot/null time does not establish a past connection or root cause; for missing-data diagnosis check history and state query window. No device/API schema change. Artifacts `/tmp/neomind-device-evidence-20261102-200547`. Baseline is connector-feedback candidate (without withdrawn headers), three changed files dedup.rs/shell.rs/device.rs. Gate PID20567, queued helper PID22589. Fixed Spark no-data + rename control, zh/en two repeats both arms =16. No overlapping build/inference; no commit/push/publish.

Device long-hint trial completed 16: baseline4/8, candidate5/8; no-data0/4 vs1/4, rename4/4 each and no extra history. The one successful probe sequence still claimed never-connected and answered Chinese in English. Long prose hint NOT accepted as diagnostic resolution. Many failures stop at device list before get, so did not see its hint. Raw records/decision.json preserved.

Current candidate replaces long hint with short current/history scope notes, and corrects contradictory device list help (old help both said no get needed for deep inspection and recommended get). Missing-data workflow is get + history; an empty queried window does not establish lifetime connectivity/root cause. Artifact `/tmp/neomind-device-probes-20261102-202057`; same frozen baseline from connector-feedback batch. Four changed files: dedup.rs, shell.rs, cli-ops/device.rs and dispatch/commands.rs. Gate PID27534, queued model helper PID29115. Same16-case protocol, no score/case change. Do not reapply withdrawn types.rs header or long hint. No commit/push/publish.

Final device-probes candidate complete: CI95 groups/3692 passed/0 failed/69 ignored; release and diff check passed. Frozen Spark16 trials: no-data get+history coverage0/4→4/4; rename state4/4→4/4, no extra history in controls. Diagnostic mean19.12s→18.43s, rename15.46s→15.03s; tiny samples, no general speed claim. Raw hard total4/8→8/8 is NOT business diagnostic success. Manual review of all four candidate diagnoses still found unsupported causal/lifetime claims (recent disconnect from registration-initialized last_seen, plugin failure without a causal probe, never-connected/ever-no-data from null timestamps and a bounded empty query). Manual semantic acceptance0/4, saved separately as semantic-review.json; original grading unchanged.

Retain dedup/shell evidence fixes + corrected list/get/history documentation. Extra result headers and long get prose were withdrawn; do not reapply them. Current four-file scope and final-source.patch, binaries, hashes, raw traces and review-summary.json live in `/tmp/neomind-device-probes-20261102-202057`. Dedicated18080 inference and gate/queue exited; no old queue resumed. No commit/push/publish. Next priority: structured observation provenance, time scope and unknown root cause, with semantic grounding acceptance; more prose alone has not solved the reasoning error. Read agent-eval-method.md for the reusable workflow.

## Structured device evidence experiment (2026-11-02)

Baseline is previous device-probes candidate. New scope ONE file cli-ops/device.rs: replace current/history prose suggestions with a leading _diagnostic_evidence object. Current evidence copies actual state flags/timestamps, marks last_seen as registration-or-status-activity and not proof of recent disconnect; root cause is not diagnosed by this endpoint and full connection history is unqueried. History evidence copies the actual window/pagination, marks activity outside the returned window and transport transitions unqueried, cause not diagnosed. Original data fields/paths stay intact. Unknown, failed, over-nested or already-annotated payloads pass through unchanged. No device state or API operation changes, no fixed final answer.

Four new unit tests passed, full CI95 groups/3696 passed/0 failed/69 ignored. Gate PID46647; queued eval PID49832. Artifacts `/tmp/neomind-structured-evidence-20261102-210159`, semantic-protocol.json frozen before model runs. Same no-data diagnosis + rename control, zh/en two repeats, both arms=16. Fixed Spark configs and original grading; separately review causal/lifetime/temporal claims and language. No overlapping builds/inference and no commit/push/publish. Results pending; don't credit command pass as semantic success.

Structured experiment finished and WITHDRAWN: hard8/8 each arm, candidate diagnostic semantic0/4 despite2 evidence cards per diagnostic trace. Candidate CI3696 passed; no evidence of semantic benefit. Archived prototype/tests/patch/records in `/tmp/neomind-structured-evidence-20261102-210159`. Restored device.rs from its baseline backup; full source diff byte-equals frozen baseline and target/release/neomind SHA equals validated baseline. Do not reapply the structured wrapper.

Important restoration correction: copy2 preserved source mtime, initial restored gate reused candidate test binary3696. That run is INVALID for restored source. Refreshed device.rs mtime and recompiled; FINAL valid gate95 groups/3692 passed/0 failed/69 ignored, exit0, `/tmp/neomind-structured-restored-workspace-r2.log`. Restored manifest/hash checks and final diff check passed.

Independent final-answer replay8 calls (cards vs compact, same Spark/runtime,1536token cap) in `/tmp/neomind-final-context-20261102-214111`. Some cautious replies but remaining scope/history ambiguities and one truncated output. No replacement samples; no production integration or regression gain claim. Do not conflate this with agent hard grading. Further design must handle actual fact coverage, streaming buffers and persisted answers before integration; no global raw-result deletion. Current retained production source equals previous device-probes baseline. Dedicated18080 model and all this-turn queues/gates exited. No commit/push/publish.

## 独立语义验收已完成

新增显式启用的Python设备证据/语义评审与真实end事件采集，23项新unittest、hard_signal24/24、preflight8/8通过，diff check通过。最终20个预设参考标签全部匹配（非独立人工盲审）。固定Spark新8条硬8/8、结束8/8；初次语义2FAIL/6ABSTAIN，4个写入控制超范围、2个服务不可用。仅对不可用2条增加时限重试一次，均FAIL，原始结果保留。四条诊断仍有过度推断，不宣称模型或全量分数提升。本批无Rust/生产回复修改，旧撤回试验不要恢复。产物 /tmp/neomind-semantic-gate-20261102-231542；细节见 [语义验收](semantic-review-2026-11-02.md) 和 [工具用法](../../eval/semantic/README.md)。专用18080与本轮队列已退出，未动用户原有模型服务。无提交推送发布。

## 自动改良：连接器配置保留已验收

保留llamacpp显式thinking参数转发（未指定保留默认，effort优先）和connector更新时保留配置的修复。所有自改写候选被交付门槛拒绝；关闭全局thinking的试验16/32提前停止，6/8→4/8，不采用该配置，默认未改。GLM-flash完整代码审查建议已核实并补强流测试。

connector原错误：GET响应层级读错，随后PUT空名称/地址，认证/映射也可能丢失。新解析验证实体与必要字段、保留可写字段和掩码凭据，仅覆盖明确参数；异常响应不写入。4项新回归（含启用/停用HTTP）通过。最终95组3699通过/0失败/69忽略，Python40项通过，构建/diff check通过。

固定Spark原参数、主thinking=true，connector-enable+device-update-name控制，中英各两轮两版16次：4/8→8/8，连接器0/4→4/4，控制4/4→4/4，4增0退。原wrapper收尾误检查辅助摘要thinking，错误保留；audit.py按明确的主true/摘要false分别验证全部请求并复核全部原评分，无重生成。产物 /tmp/neomind-connector-preservation-20261103-004028/ab/verified-summary.json；否定试验 /tmp/neomind-refinement-20261102-235215。16次已完成，本轮所有队列及18080/18081均退出，不恢复旧队列。未重跑完整138例，诊断语义仍未解决；无提交推送发布。

阅读 [本轮验收](automation-review-2026-11-02.md) 与 [自动化用法](../../eval/semantic/REFINEMENT.md)。源码完整回传曾被自动审批拒绝，未绕过，已在Mac原地局部修复，不需要该传输。

### Active comprehensive study, 2026-11-03

Do not resume old queues. Root /tmp/neomind-comprehensive-20261103-085034, official main/; three frozen Release arms baseline, cli_only, full; 69 case IDs x two languages x two rounds x three =828. Controller /tmp/neomind-comprehensive-single-run.py PID38647 (verify). Spark32K single-slot, thinking true, .3/.75/20,600s case cap. Root four-slot capacity pilot is excluded. No overlapping builds/inference. Native score watcher /tmp/neomind-local-audit.py PID43164 (verify); five boundary/tampering tests pass. It writes main/local-audit.json and final native-audit-records.json; preserves incomplete status until all828, checks native grader, case hashes, alignment, final binary/evaluator hashes. No network or keys. External GLM evaluation-answer/evidence export was rejected by automatic review; sender not run. Use local task/state/trace/delivery metrics, no universal factuality claim. Source remains frozen for evaluation; existing live uncommitted production work retained. No commit/push/publish.

Comprehensive local audit final helper: PID45773 (verify), analysis/ archived with manifest, 7 audit tests and 6 pairing tests pass. Native hard signals are independently recomputed from frozen case+record, not just copied. Final comprehensive-report.md is emitted only after all828 and hash checks pass. Pilot orphan API processes were cleaned; official controller PID38647 remains active.

Comprehensive current processes and analysis versions: read main/processes.json and main/analysis-manifest-current.json instead of relying on older PIDs above. Current audit PID58544, controller38647, passive monitor51697, idle-sleep inhibitor49907; verify. Native timeout record can drop turn traces: v2 audit (8 tests) marks missing/partial coverage and lower-bound counts, triage separately counts console execution events. Pairing tests6 pass. Archives analysis/revisions/trace-coverage-v2/. Main grader/snapshots/binaries unchanged. No retry of product failures. Deleted only owned rebuildable build-target cache (records main/build-cache-cleanup.json), freeing ~5.3GiB; independent binaries/source/logs preserved. Cleanup occurred during baseline HACCP case; retain score and assess timing sensitivity if material. Current primary not complete; no comprehensive improvement claim.
