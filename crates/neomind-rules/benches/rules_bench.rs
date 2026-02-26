//! Rule engine performance benchmarks using Criterion.rs
//!
//! Run with: cargo bench -p neomind-rules --bench rules_bench

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use neomind_rules::dsl::{Action, CompiledRule, Condition, Rule};
use neomind_rules::engine::RuleEngine;
use tokio::runtime::Runtime;

/// Benchmark rule parsing
fn bench_rule_parsing(c: &mut Criterion) {
    let simple_rule = r#"
RULE "Simple Temperature Rule"
WHEN device("sensor1").temperature > 30
DO
  notify("admin")
END
"#;

    let complex_rule = r#"
RULE "Complex Multi-Condition Rule"
WHEN device("sensor1").temperature > 30 AND device("sensor2").humidity < 40
FOR 5m
DO
  notify("admin")
  device("ac").set_temperature(26)
END
"#;

    c.bench_function("rule_parse_simple", |b| {
        b.iter(|| {
            let _ = Rule::parse(black_box(simple_rule));
        });
    });

    c.bench_function("rule_parse_complex", |b| {
        b.iter(|| {
            let _ = Rule::parse(black_box(complex_rule));
        });
    });
}

/// Benchmark rule compilation
fn bench_rule_compilation(c: &mut Criterion) {
    let rules = vec![
        (
            "simple",
            r#"
RULE "Simple Rule"
WHEN device("sensor1").temperature > 30
DO
  notify("admin")
END
"#,
        ),
        (
            "complex",
            r#"
RULE "Complex Rule"
WHEN device("sensor1").temperature > 30 AND device("sensor2").humidity < 40
FOR 5m
DO
  notify("admin")
  device("ac").set_temperature(26)
  log("Temperature alert", Warning)
END
"#,
        ),
    ];

    let mut group = c.benchmark_group("rule_compilation");
    for (name, rule_str) in &rules {
        group.bench_with_input(BenchmarkId::from_parameter(name), rule_str, |b, rule_str| {
            b.iter(|| {
                let rule = Rule::parse(black_box(rule_str)).unwrap();
                let _ = CompiledRule::new(black_box(&rule));
            });
        });
    }
    group.finish();
}

/// Benchmark rule matching
fn bench_rule_matching(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let rule_str = r#"
RULE "Temperature Alert"
WHEN device("sensor1").temperature > 30
DO
  notify("admin")
END
"#;

    let rule = Rule::parse(rule_str).unwrap();
    let compiled = CompiledRule::new(&rule);

    c.bench_function("rule_match_positive", |b| {
        b.iter(|| {
            let state = serde_json::json!({
                "sensor1": { "temperature": 35.0 }
            });
            let _ = compiled.matches(black_box(&state));
        });
    });

    c.bench_function("rule_match_negative", |b| {
        b.iter(|| {
            let state = serde_json::json!({
                "sensor1": { "temperature": 25.0 }
            });
            let _ = compiled.matches(black_box(&state));
        });
    });
}

/// Benchmark rule engine with multiple rules
fn bench_rule_engine(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let setup_engine = |rule_count: usize| -> RuleEngine {
        let engine = RuleEngine::new();
        for i in 0..rule_count {
            let rule_str = format!(
                r#"
RULE "Rule {}"
WHEN device("sensor{}").temperature > {}
DO
  notify("admin")
END
"#,
                i,
                i,
                20 + i
            );
            let rule = Rule::parse(&rule_str).unwrap();
            let compiled = CompiledRule::new(&rule);
            rt.block_on(async {
                engine.add_rule(compiled).await.unwrap();
            });
        }
        engine
    };

    let mut group = c.benchmark_group("rule_engine");
    for rule_count in [10, 50, 100].iter() {
        group.bench_with_input(
            BenchmarkId::new("evaluate", rule_count),
            rule_count,
            |b, &rule_count| {
                let engine = setup_engine(rule_count);
                let state = serde_json::json!({
                    "sensor0": { "temperature": 35.0 },
                    "sensor1": { "temperature": 25.0 },
                    "sensor2": { "temperature": 30.0 },
                });
                b.to_async(&rt).iter(|| async {
                    let _ = engine.evaluate(&state).await;
                });
            },
        );
    }
    group.finish();
}

/// Benchmark condition evaluation
fn bench_condition_evaluation(c: &mut Criterion) {
    c.bench_function("condition_simple_gt", |b| {
        let cond = Condition::Simple {
            device: "sensor1".to_string(),
            metric: "temperature".to_string(),
            operator: ">".to_string(),
            value: 30.0,
        };
        let state = serde_json::json!({
            "sensor1": { "temperature": 35.0 }
        });
        b.iter(|| {
            let _ = cond.evaluate(black_box(&state));
        });
    });

    c.bench_function("condition_and", |b| {
        let left = Box::new(Condition::Simple {
            device: "sensor1".to_string(),
            metric: "temperature".to_string(),
            operator: ">".to_string(),
            value: 30.0,
        });
        let right = Box::new(Condition::Simple {
            device: "sensor2".to_string(),
            metric: "humidity".to_string(),
            operator: "<".to_string(),
            value: 40.0,
        });
        let cond = Condition::And { left, right };
        let state = serde_json::json!({
            "sensor1": { "temperature": 35.0 },
            "sensor2": { "humidity": 35.0 },
        });
        b.iter(|| {
            let _ = cond.evaluate(black_box(&state));
        });
    });
}

criterion_group!(
    benches,
    bench_rule_parsing,
    bench_rule_compilation,
    bench_rule_matching,
    bench_rule_engine,
    bench_condition_evaluation,
);
criterion_main!(benches);
