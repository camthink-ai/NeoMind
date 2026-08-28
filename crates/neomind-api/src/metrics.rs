//! Process metrics for the public `/api/metrics` endpoint.
//!
//! Prometheus text format, counters only — no per-user or per-device data,
//! so the endpoint can stay unauthenticated like `/api/health` (an edge box
//! behind nginx scrapes it; there is nothing here worth an auth round-trip).
//!
//! What's exposed and why:
//! - `neomind_http_requests_total` / `_4xx` / `_5xx` — is the box serving,
//!   and is it erroring (a scraper alert on 5xx rate beats grepping logs).
//! - `neomind_eventbus_dropped_total` — silent event loss. The EventBus warn
//!   log already says "surface this, don't let the system fail quietly";
//!   this is the surfacing. Non-zero and growing = a rules/telemetry/
//!   automation subscriber is too slow and events are being missed.
//! - `neomind_uptime_seconds`, `neomind_build_info` — scrape correlation.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

pub static HTTP_REQUESTS_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static HTTP_RESPONSES_4XX: AtomicU64 = AtomicU64::new(0);
pub static HTTP_RESPONSES_5XX: AtomicU64 = AtomicU64::new(0);

fn started_at() -> &'static Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now)
}

/// Global HTTP counter middleware — mount on the merged router so it sees
/// every route (public, protected, webhooks, static assets).
pub async fn http_metrics_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let response = next.run(request).await;
    HTTP_REQUESTS_TOTAL.fetch_add(1, Ordering::Relaxed);
    let status = response.status().as_u16();
    if (400..500).contains(&status) {
        HTTP_RESPONSES_4XX.fetch_add(1, Ordering::Relaxed);
    } else if status >= 500 {
        HTTP_RESPONSES_5XX.fetch_add(1, Ordering::Relaxed);
    }
    response
}

/// Render all metrics in the Prometheus text exposition format.
pub fn render_prometheus(event_bus: Option<&neomind_core::eventbus::EventBus>) -> String {
    let mut out = String::with_capacity(1024);

    let counter = |out: &mut String, name: &str, help: &str, value: u64| {
        out.push_str(&format!(
            "# HELP {name} {help}\n# TYPE {name} counter\n{name} {value}\n"
        ));
    };

    counter(
        &mut out,
        "neomind_http_requests_total",
        "Total HTTP requests served since process start.",
        HTTP_REQUESTS_TOTAL.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "neomind_http_responses_4xx_total",
        "HTTP responses with a 4xx status since process start.",
        HTTP_RESPONSES_4XX.load(Ordering::Relaxed),
    );
    counter(
        &mut out,
        "neomind_http_responses_5xx_total",
        "HTTP responses with a 5xx status since process start.",
        HTTP_RESPONSES_5XX.load(Ordering::Relaxed),
    );

    let (dropped, subscribers) = match event_bus {
        Some(bus) => (bus.total_dropped_count(), bus.subscriber_count() as u64),
        None => (0, 0),
    };
    counter(
        &mut out,
        "neomind_eventbus_dropped_total",
        "Events silently dropped because a subscriber lagged behind the broadcast buffer. \
         Non-zero and growing means a downstream consumer (rules/telemetry/automation) \
         is missing events.",
        dropped,
    );
    counter(
        &mut out,
        "neomind_eventbus_subscribers",
        "Current EventBus broadcast subscribers.",
        subscribers,
    );

    let uptime = started_at().elapsed().as_secs();
    out.push_str(&format!(
        "# HELP neomind_uptime_seconds Seconds since process start.\n\
         # TYPE neomind_uptime_seconds gauge\n\
         neomind_uptime_seconds {uptime}\n"
    ));
    out.push_str(&format!(
        "# HELP neomind_build_info Build information.\n\
         # TYPE neomind_build_info gauge\n\
         neomind_build_info{{version=\"{}\"}} 1\n",
        env!("CARGO_PKG_VERSION")
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_includes_all_metric_families() {
        let bus = neomind_core::eventbus::EventBus::new();
        let out = render_prometheus(Some(&bus));
        for name in [
            "neomind_http_requests_total",
            "neomind_http_responses_4xx_total",
            "neomind_http_responses_5xx_total",
            "neomind_eventbus_dropped_total",
            "neomind_eventbus_subscribers",
            "neomind_uptime_seconds",
            "neomind_build_info",
        ] {
            assert!(out.contains(name), "missing metric {name} in:\n{out}");
        }
    }

    #[test]
    fn render_without_event_bus() {
        let out = render_prometheus(None);
        assert!(out.contains("neomind_eventbus_dropped_total 0\n"));
    }
}
