# CLI reasoning-format proposal review

Startup flag withdrawn; server.rs matches HEAD exactly. Original proposal and
16/0 builtin-server tests are in /tmp/neomind-cli-reasoning-review/.
The retained release hash remains unchanged. No new model inference performed.

b12024 explicitly supports request-body reasoning_format. Setting a CLI default
does not address malformed output missing an opening reasoning tag.
The completed zh/en r0 trials in /tmp/leak-fix-verify/ both leaked despite the
CLI flag; zh-r1 was interrupted, not established as a task timeout.
Its orphan API process was verified against the trial cwd and stopped.

Official Spark supports none/low/high. Served template matches its official
current tokenizer config exactly. A first-closer parser patch has the same
normal-text ambiguity as the rejected client splitter, including potential
streaming delay. No such patch is approved as a root fix.
Summary-channel filtering does not change request thinking modes and cannot
reclassify text the server already placed in content.

Next diagnostic candidates: actual low/high forwarding, bounded native thinking,
or a controlled official-runtime comparison. No performance or correctness
benefit is assumed; no fresh inference budget consumed by this review.
