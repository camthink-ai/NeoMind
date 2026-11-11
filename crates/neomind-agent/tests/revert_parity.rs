//! Token-estimate & compaction regression guard ("revert parity").
//!
//! 2026-11-11 postmortem: a tokenizer "calibration" (CJK 1.8->1.0,
//! digits 0.3->1.5) looked better against the raw BPE tokenizer on
//! synthetic samples but regressed the 138-case gate from 99 to 88/138
//! (paired -11) — it inflated CLI/YAML/JSON estimates ~1.6x, over-compacted
//! history, and made the model redo work (rounds 14->36 on some cases).
//! The conservative over-estimate is load-bearing compaction headroom.
//!
//! This test pins the shipped rates and the compaction behavior. If you
//! intentionally change estimation, update the golden numbers here AND run
//! the 138 gate — do not ship a rate change on synthetic-sample evidence.
//!
//! Golden values were produced by a HEAD-only worktree (ddb3cebd) and must
//! match this crate's output exactly.

use neomind_agent::agent::tokenizer::estimate_tokens as agent_est;
use neomind_core::llm::compaction::{
    compact_messages, estimate_messages_tokens, estimate_tokens as core_est, CompactionConfig,
};
use neomind_core::message::{Content, Message, MessageRole};

/// Deterministic, session-shaped corpus covering the categories that
/// mattered in the 138 gate: CLI YAML output, zh/en record JSON, logs with
/// timestamps, code/commands, pure CJK, pure prose, digit runs.
fn corpus() -> Vec<(&'static str, String)> {
    vec![
        ("zh_record_json",
        r#"记录0123：{"温度":23,"状态":"正常","备注":"中文单行JSON压力载荷第0123条"}"#.repeat(20),),
        ("en_record_json",
        r#"record-0123:{"temp":23,"status":"ok","note":"english entry 0123"}"#.repeat(20),),
        ("cli_yaml",
        concat!(
            "device_id: cam-1\n",
            "metrics: [\"metadata.image_id\", \"metadata.timestamp\"]\n",
            "data:\n  start: 1794134548\n  end: 1794220948\n",
            "  pagination:\n    offset: 0\n    limit: 100\n    total: 400\n",
            "    next_cursor: 1794207533\n"
        )
        .repeat(20),),
        ("timestamped_log",
        "[2m2026-11-11T03:14:55.456547Z[0m Configured default LLM backend category=\"ai\" prior_error=LLM error: No active\n".repeat(20),),
        ("pure_cjk", "边界与窗口正确性的中文填充内容".repeat(50)),
        ("pure_prose",
        "extended padding to push one history page beyond the threshold for verification "
            .repeat(50),),
        ("digit_run", "1794183570".repeat(50)),
        ("code_cmd",
        "neomind device history demo-sensor-zh --metric note --limit 5000\n".repeat(20),),
        ("mixed_metrics",
        "{\"timestamp\":1794183570,\"value\":\"记录0123：温度传感器读数升高警报\",\"metric\":\"note\"},".repeat(20),),
    ]
}

fn msg(role: MessageRole, text: String) -> Message {
    Message {
        role,
        content: Content::Text(text),
        tool_name: None,
        timestamp: None,
    }
}

#[test]
fn token_rates_and_compaction_match_the_shipped_baseline() {
    // Golden estimates verified identical to HEAD (ddb3cebd) on the same
    // corpus: any drift here means the rates or the per-line accounting
    // changed.
    let golden: &[(&str, usize)] = &[
        ("zh_record_json", 1118),
        ("en_record_json", 485),
        ("cli_yaml", 1384),
        ("timestamped_log", 756),
        ("pure_cjk", 1486),
        ("pure_prose", 1265),
        ("digit_run", 165),
        ("code_cmd", 428),
        ("mixed_metrics", 988),
    ];
    let mut agent_total = 0usize;
    let mut core_total = 0usize;
    for ((name, text), (gname, want)) in corpus().into_iter().zip(golden.iter()) {
        let a = agent_est(&text);
        let c = core_est(&text);
        assert_eq!(name, *gname, "corpus/golden order drift");
        assert_eq!(a, *want, "agent estimate drifted for {name}");
        assert_eq!(c, *want, "core estimate drifted for {name}");
        agent_total += a;
        core_total += c;
    }
    assert_eq!(agent_total, 8075, "agent corpus total drifted");
    assert_eq!(core_total, 8075, "core corpus total drifted");

    // Compaction parity on histories that ACTUALLY exceed budget (the
    // previous 440-token fixture never compacted, so it proved nothing).
    // Tool results are ~825 tokens each (under the quarter cap), so the
    // retained protection-cap fix cannot interfere here.
    // Compaction over budget (normal message sizes): the 8K tier must
    // compress; the golden totals are the HEAD reference values.
    for (name, ctx) in [("small", 8_192usize), ("mid", 16_384), ("large", 32_768)] {
        let mut hist = vec![msg(MessageRole::System, "You are NeoMind.".to_string())];
        for i in 0..14 {
            hist.push(msg(MessageRole::User, format!("step {i}: run the command")));
            hist.push(msg(
                MessageRole::Tool,
                format!("result {i}: {}", "abcdefghij ".repeat(300)),
            ));
        }
        let cfg = CompactionConfig::for_context_size(ctx);
        let before = estimate_messages_tokens(&hist);
        let res = compact_messages(&hist, &cfg, ctx);
        let joined: String = res
            .messages
            .iter()
            .map(|m| format!("{}|{}", m.role, m.content.as_text()))
            .collect::<Vec<_>>()
            .join("\n");
        let digest = {
            let mut h: u64 = 1469598103934665603;
            for b in joined.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(1099511628211);
            }
            h
        };
        println!(
            "CMP {name} ctx={ctx} before={before} after={} removed={} truncated={} digest={digest:016x}",
            res.compacted_tokens, res.messages_removed, res.messages_truncated
        );
        assert_eq!(before, 14038, "fixture drift ({name})");
        match name {
            "small" => assert_eq!(
                res.compacted_tokens, 5688,
                "8K compaction drift (golden 5688; HEAD reference)"
            ),
            _ => assert_eq!(
                res.compacted_tokens, 14038,
                "16K/32K history must fit without compaction (golden 14038)"
            ),
        }
    }

    // Known retained difference: an oversized PROTECTED message is capped
    // in the reverted workspace but not in HEAD. Print it so the expected
    // divergence is explicit rather than accidental.
    // Retained fix (differs from HEAD by design): a protected message larger
    // than a quarter of the budget is truncated, not passed through.
    let big = vec![
        msg(MessageRole::System, "You are NeoMind.".to_string()),
        msg(MessageRole::Tool, "知".repeat(8_000)),
    ];
    let cfg = CompactionConfig::for_context_size(8_192);
    let res = compact_messages(&big, &cfg, 8_192);
    println!(
        "BIG after={} removed={} truncated={}",
        res.compacted_tokens, res.messages_removed, res.messages_truncated
    );
    assert_eq!(
        res.messages_truncated, 1,
        "protection-cap fix missing: an 8K-char protected message must be capped"
    );
    assert!(
        res.compacted_tokens < 3_000,
        "capped protected message must not blow an 8K window: {}",
        res.compacted_tokens
    );
}
