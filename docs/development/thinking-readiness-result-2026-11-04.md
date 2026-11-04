# Thinking policy: review and bounded acceptance result

## Decision

Do not promote the capability-gate/default-policy candidate. Archive it outside
the production worktree. Preserve only independently reproduced parameter and
recovery-channel correctness fixes. No commit, push, deployment or publication.

## Code evidence

The retained four Rust files match byte-for-byte the source recorded by
/tmp/neomind-summary-channel-fix/final-source-hashes.json. That source passed
95 suites / 3729 tests / 0 failures / 69 ignored. After restoration, agent lib
with test-utils,llamacpp was rebuilt and passed 924/0. The withdrawn experiment
passed 3738/0, but that is not the retained version's test count.

Default-gate review found all production profiles Unknown, broader automatic
behavior changes, and inconsistent saved-setting validation. A compatibility
rollout mode and strict-mode consistency checks were tested, then archived
with the rest of the experiment because model acceptance did not pass.

## Model screen (not comprehensive scores)

Spark Q4_K_M, Mac b12024 Metal, 32K single slot, .3/.75/20, 600s cap.
Two selected tasks, zh/en, one round per arm: eight executions, 17m16s.
Baseline binary and key sources match HEAD ab21f800. Candidate was frozen;
all native scores were independently recomputed from the eight raw records.

| Pair | Baseline | Candidate |
|---|---|---|
| command-discovery en | fail | fail |
| command-discovery zh | pass | fail |
| building-bms en | fail | fail |
| building-bms zh | fail | fail |

Zero gains / one loss. Four pairs do not establish causal regression or
statistical non-regression. Command-discovery state checks passed in both arms;
the scored loss was command coverage (missing expected device-get step).
Both BMS arms failed real rule/channel/device-command requirements. Mean case
elapsed: baseline 135.31s, candidate 119.80s; lower time is not a benefit claim
when tasks were incomplete. No timeouts or agent-error status in these records.

The screened candidate used compatibility mode. Strict capability mode was
unit-tested only; no real runtime profile was certified. These data cannot
establish that strict mode is effective or ineffective.

## Leak evidence and observation limits

Final reply markers: baseline 0 turns, candidate 1 turn. Live WS capture was
available for only 1/4 baseline trials and 2/4 candidate trials. Observer v1
patched a duplicate imported server module; v2 corrected that for later workers.
Original/native evaluator and request bodies were unchanged, correction hashes
were recorded, and no cases were rerun. Missing observations are not zero leaks.
Observed live marked segments: baseline 3, candidate 5; unequal coverage prevents
comparing those counts as rates. Content events contained repair monologue plus
closing think markers even when persisted final replies were clean.

Artifacts: /tmp/neomind-thinking-readiness/ (screen/audit.json, freeze, raw trials,
observer correction, source hashes, final build/tests, withdrawn-policy/).
All 8 native scores regraded identically. Original 552 T4 results remain untouched.
The test controller exited; owned model port 36470 and recorded API PIDs were
verified stopped. Pre-existing Mac model servers and T4 services were not changed.

## Final retained build

Final retained release rebuilt successfully (exit 0), version 1.2.5 unchanged.
SHA256: 843370ff491e8c7884535b66749a5d5fa7960ccc126a303ee26779380ad78652.
The development target/release/neomind now matches retained source, not the
rejected candidate. Immutable copy and complete source hashes are recorded in
/tmp/neomind-thinking-readiness/final-retained-manifest.json.
This is a source/build correctness delivery, not a passed model acceptance.
