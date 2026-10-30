//! Streaming response processing — re-exports from sub-modules.
//!
//! This module was decomposed from a single 2495-line file into focused sub-modules:
//! - `stream_core`: text-only streaming with multi-round ReAct loop
//! - `stream_multimodal`: multimodal (text + images) streaming
//! - `cache`: tool result caching
//! - `thinking`: thinking content cleanup
//! - `sanitize`: base64 stripping, truncation
//! - `dedup`: tool result deduplication
//! - `result_format`: tool result formatting
//! - `context`: context window management
//! - `resolve`: cached argument resolution
//! - `tool_exec`: tool execution with retry

// Sub-modules
mod cache;
mod context;
mod dedup;
mod resolve;
mod result_format;
mod sanitize;
mod stream_core;
mod stream_multimodal;
mod thinking;
mod tool_exec;

// Re-exports from neomind_core
pub use neomind_core::llm::compaction::{CompactionConfig, MessagePriority};

// Public API re-exports (preserving all original import paths)
pub use cache::ToolResultCache;
pub use result_format::format_tool_results;
pub use stream_core::{process_stream_events_with_safeguards, StreamSafeguards};
pub use stream_multimodal::process_multimodal_stream_events_with_safeguards;
pub use thinking::cleanup_thinking_content;

/// `StreamExt::next().await` wrapped in `tokio::time::timeout`.
///
/// Every LLM stream loop previously did `while let Some(x) = stream.next().await`
/// with only an *in-loop* elapsed-timeout check. When the upstream stream
/// stalls (zero chunks — e.g. the LLM backend never yields), `next()` blocks
/// forever and the in-loop check is never reached, hanging the whole chat/eval
/// turn. This helper bounds each `next()` so a stall yields `None` and the
/// loop force-completes instead of hanging.
///
/// `Ok(Some(item))` = a chunk arrived; `Ok(None)` = stream ended normally;
/// `Err(_)` = stalled for `dur` with zero chunks → treat as end of stream.
pub async fn next_chunk_or_timeout<S>(stream: &mut S, dur: std::time::Duration) -> Option<S::Item>
where
    S: futures::Stream + Unpin,
{
    match tokio::time::timeout(dur, futures::StreamExt::next(stream)).await {
        Ok(Some(item)) => Some(item),
        Ok(None) => None,
        Err(_elapsed) => {
            tracing::warn!(
                "Stream stalled: no chunks for {:?}, forcing completion",
                dur
            );
            None
        }
    }
}

/// Outcome of an interruptible chunk wait.
pub(crate) enum StreamWait<T> {
    /// A chunk arrived (`Some`) or the stream ended/stalled (`None`).
    Chunk(Option<T>),
    /// The cancel watch channel fired while waiting.
    Interrupted,
}

/// `next_chunk_or_timeout` + cancel-channel awareness.
///
/// The plain helper blocks on `next(stream)` for up to `dur` — on a local
/// llama.cpp backend the wait for the FIRST chunk includes request queueing
/// behind other slots plus prompt prefill, which can run tens of seconds
/// (live 2026-10-19: ~62s). During that wait the stream loop body (where the
/// interrupt signal was checked) never executes, so a user cancel sat
/// unprocessed until the first token arrived — 55s after the __CANCEL__
/// frame — while the zombie request kept occupying the backend's only slot
/// and every retry queued behind it. This variant wakes on the cancel
/// channel as well, so the generator (and with it the underlying HTTP
/// request) terminates immediately.
pub(crate) async fn next_chunk_or_timeout_or_interrupt<S>(
    stream: &mut S,
    dur: std::time::Duration,
    interrupt: Option<&tokio::sync::watch::Receiver<bool>>,
) -> StreamWait<S::Item>
where
    S: futures::Stream + Unpin,
{
    // Fast path: already interrupted before waiting (cancel arrived while
    // the previous chunk was being processed).
    if interrupt.map(|rx| *rx.borrow()).unwrap_or(false) {
        return StreamWait::Interrupted;
    }
    let mut rx = interrupt.cloned();
    loop {
        let Some(current) = rx.as_ref() else {
            return StreamWait::Chunk(next_chunk_or_timeout(stream, dur).await);
        };
        let mut probe = current.clone();
        tokio::select! {
            item = next_chunk_or_timeout(stream, dur) => return StreamWait::Chunk(item),
            changed = probe.changed() => match changed {
                // Sender dropped: no future cancel can arrive — stop
                // selecting on it (a resolved-Err changed() would otherwise
                // busy-loop).
                Err(_) => rx = None,
                Ok(()) => {
                    if *probe.borrow_and_update() {
                        return StreamWait::Interrupted;
                    }
                    // Flipped back to false — fall through and keep waiting.
                }
            },
        }
    }
}

pub(crate) async fn wait_for_interrupt(interrupt: Option<&tokio::sync::watch::Receiver<bool>>) {
    if let Some(mut rx) = interrupt.cloned() {
        if *rx.borrow() {
            return;
        }
        while rx.changed().await.is_ok() {
            if *rx.borrow_and_update() {
                return;
            }
        }
    }
    std::future::pending::<()>().await;
}

/// Bound request setup as well as token consumption. A backend may stall
/// before returning a stream; that time belongs to the same turn budget.
pub(crate) async fn request_before_deadline<T>(
    request: impl std::future::Future<Output = crate::error::Result<T>>,
    deadline: std::time::Instant,
    interrupt: Option<&tokio::sync::watch::Receiver<bool>>,
) -> crate::error::Result<T> {
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    if remaining.is_zero() || interrupt.is_some_and(|rx| *rx.borrow()) {
        return Err(crate::error::NeoMindError::Llm(
            "Turn ended before request setup".into(),
        ));
    }
    tokio::select! {
        result = tokio::time::timeout(remaining, request) => result
            .map_err(|_| crate::error::NeoMindError::Llm("Turn time budget exhausted".into()))?,
        () = wait_for_interrupt(interrupt) => Err(crate::error::NeoMindError::Llm("Turn interrupted".into())),
    }
}

/// Summaries and empty-response recovery share the original deadline and
/// cancellation channel, including when the model keeps producing thinking.
pub(crate) async fn next_before_deadline<S>(
    stream: &mut S,
    idle: std::time::Duration,
    deadline: std::time::Instant,
    interrupt: Option<&tokio::sync::watch::Receiver<bool>>,
) -> Option<S::Item>
where
    S: futures::Stream + Unpin,
{
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    if remaining.is_zero() {
        return None;
    }
    match next_chunk_or_timeout_or_interrupt(stream, idle.min(remaining), interrupt).await {
        StreamWait::Chunk(item) => item,
        StreamWait::Interrupted => None,
    }
}

// Re-exports for internal crate use and test access
pub(crate) use resolve::resolve_cached_arguments;
pub(crate) use sanitize::{sanitize_tool_result_for_prompt, truncate_result_utf8};

#[cfg(test)]
mod stall_tests {
    use super::{next_chunk_or_timeout, next_chunk_or_timeout_or_interrupt, StreamWait};
    use std::time::Duration;

    #[tokio::test]
    async fn request_setup_and_ready_streams_obey_the_same_deadline() {
        let deadline = std::time::Instant::now() + Duration::from_millis(25);
        let pending = std::future::pending::<crate::error::Result<()>>();
        assert!(super::request_before_deadline(pending, deadline, None)
            .await
            .is_err());
        let mut ready = futures::stream::repeat("token");
        assert!(
            super::next_before_deadline(&mut ready, Duration::from_secs(60), deadline, None)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn cancel_interrupts_silent_request_setup() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let request = super::request_before_deadline(
            std::future::pending::<crate::error::Result<()>>(),
            std::time::Instant::now() + Duration::from_secs(60),
            Some(&rx),
        );
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            tx.send(true).unwrap();
        });
        assert!(tokio::time::timeout(Duration::from_secs(1), request)
            .await
            .expect("cancel wakes setup")
            .is_err());
    }

    /// A cancel arriving while the stream is stalled/pending must wake the
    /// wait IMMEDIATELY — not when the chunk/stall deadline finally hits.
    /// Regression for the live 55s cancel latency (2026-10-19): the wait
    /// sat on `next()` while llama.cpp spent ~60s on slot queueing + prompt
    /// prefill before the first token, so the __CANCEL__ frame wasn't seen
    /// until then and the zombie request kept the backend's only slot.
    #[tokio::test]
    async fn cancel_interrupts_pending_chunk_wait() {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let mut stalled: futures::stream::Pending<Result<(String, bool), &'static str>> =
            futures::stream::pending();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = cancel_tx.send(true);
        });
        let started = std::time::Instant::now();
        let got = next_chunk_or_timeout_or_interrupt(
            &mut stalled,
            // Far beyond the test's patience: only the cancel may end this.
            Duration::from_secs(60),
            Some(&cancel_rx),
        )
        .await;
        assert!(
            matches!(got, StreamWait::Interrupted),
            "cancel must interrupt the pending wait"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "cancel must not wait for the chunk/stall deadline"
        );
    }

    /// A zero-chunk stall (upstream LLM never yields) must force-break via
    /// the timeout instead of hanging forever. Regression for the 8h hang on
    /// the `dashboard/components` eval case (stream_core PHASE 1 loop).
    #[tokio::test]
    async fn stalled_stream_times_out() {
        let mut stalled: futures::stream::Pending<Result<(String, bool), &'static str>> =
            futures::stream::pending();
        let started = std::time::Instant::now();
        let got = next_chunk_or_timeout(&mut stalled, Duration::from_millis(50)).await;
        assert!(got.is_none(), "stalled stream must yield None on timeout");
        assert!(
            started.elapsed() >= Duration::from_millis(40),
            "must actually wait for the timeout, not return instantly"
        );
    }

    /// A finite stream still yields normally through the wrapper.
    #[tokio::test]
    async fn finite_stream_yields() {
        let chunks: Vec<Result<(String, bool), &'static str>> = vec![
            Ok(("hi".to_string(), false)),
            Ok(("there".to_string(), false)),
        ];
        let mut s = futures::stream::iter(chunks);
        let mut n = 0;
        while let Some(item) = next_chunk_or_timeout(&mut s, Duration::from_secs(1)).await {
            assert!(item.is_ok());
            n += 1;
        }
        assert_eq!(n, 2);
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod nudge_tests;
