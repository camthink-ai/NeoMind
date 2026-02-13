//! Agent performance benchmarks using Criterion.rs
//!
//! Run with: cargo bench -p edge-ai-agent

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use neomind_agent::agent::AgentMessage;
use tokio::runtime::Runtime;

/// Benchmark token estimation (synchronous, no async needed)
fn bench_token_estimation(c: &mut Criterion) {
    let messages = create_test_messages(10);

    c.bench_function("estimate_tokens_10_messages", |b| {
        b.iter(|| {
            let mut total = 0;
            for msg in &messages {
                total += black_box(neomind_agent::agent::tokenizer::estimate_message_tokens(
                    msg,
                ));
            }
            black_box(total)
        });
    });

    c.bench_function("estimate_tokens_50_messages", |b| {
        let messages = create_test_messages(50);
        b.iter(|| {
            let mut total = 0;
            for msg in &messages {
                total += black_box(neomind_agent::agent::tokenizer::estimate_message_tokens(
                    msg,
                ));
            }
            black_box(total)
        });
    });

    c.bench_function("estimate_tokens_100_messages", |b| {
        let messages = create_test_messages(100);
        b.iter(|| {
            let mut total = 0;
            for msg in &messages {
                total += black_box(neomind_agent::agent::tokenizer::estimate_message_tokens(
                    msg,
                ));
            }
            black_box(total)
        });
    });
}

/// Benchmark context compaction (synchronous)
fn bench_context_compaction(c: &mut Criterion) {
    c.bench_function("compact_10_messages", |b| {
        b.iter_batched(
            || create_test_messages(10),
            |messages| black_box(neomind_agent::agent::compact_tool_results(&messages, 2)),
            criterion::BatchSize::SmallInput,
        );
    });

    c.bench_function("compact_50_messages", |b| {
        b.iter_batched(
            || create_test_messages(50),
            |messages| black_box(neomind_agent::agent::compact_tool_results(&messages, 2)),
            criterion::BatchSize::LargeInput,
        );
    });

    c.bench_function("compact_100_messages", |b| {
        b.iter_batched(
            || create_test_messages(100),
            |messages| black_box(neomind_agent::agent::compact_tool_results(&messages, 2)),
            criterion::BatchSize::LargeInput,
        );
    });
}

/// Benchmark conversation context operations (synchronous)
fn bench_conversation_context(c: &mut Criterion) {
    c.bench_function("context_add_devices", |b| {
        b.iter_batched(
            || neomind_agent::agent::ConversationContext::new(),
            |mut ctx| {
                for i in 0..10 {
                    ctx.add_device(format!("device_{}", i));
                }
                black_box(ctx)
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Benchmark memory operations (synchronous)
fn bench_memory_operations(c: &mut Criterion) {
    c.bench_function("memory_clone_10_messages", |b| {
        b.iter_batched(
            || {
                let mut state = neomind_agent::agent::AgentInternalState::new("test".to_string());
                for i in 0..10 {
                    state.push_message(AgentMessage::user(format!("test message {}", i)));
                }
                state
            },
            |state| black_box(state.memory.clone()),
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Benchmark semaphore acquisition (async with custom executor)
fn bench_semaphore_acquire(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("semaphore_acquire_10_permit", |b| {
        b.to_async(&rt).iter(|| async {
            let semaphore = tokio::sync::Semaphore::new(10);
            let _permit = semaphore.acquire().await.unwrap();
            black_box(());
        });
    });
}

/// Benchmark message creation (common operation)
fn bench_message_creation(c: &mut Criterion) {
    c.bench_function("message_user", |b| {
        b.iter(|| black_box(AgentMessage::user("test message")));
    });

    c.bench_function("message_system", |b| {
        b.iter(|| black_box(AgentMessage::system("system message")));
    });

    c.bench_function("message_assistant", |b| {
        b.iter(|| black_box(AgentMessage::assistant("assistant response")));
    });
}

criterion_group!(
    agent_benches,
    bench_token_estimation,
    bench_context_compaction,
    bench_conversation_context,
    bench_memory_operations,
    bench_semaphore_acquire,
    bench_message_creation
);

criterion_main!(agent_benches);

// ============================================================================
// Helpers
// ============================================================================

fn create_test_messages(count: usize) -> Vec<AgentMessage> {
    (0..count)
        .map(|i| AgentMessage::user(format!("Test message {} with some content", i)))
        .collect()
}
