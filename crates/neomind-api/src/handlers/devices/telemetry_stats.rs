//! Telemetry performance monitoring handler.

use axum::extract::State;
use serde_json::json;

use crate::handlers::{
    common::{ok, HandlerResult},
    ServerState,
};

/// Get telemetry performance statistics.
///
/// GET /api/telemetry/stats
///
/// Returns detailed performance metrics for telemetry queries including:
/// - Average read/write latency
/// - Cache hit rates
/// - Query performance by metric type
#[utoipa::path(
    get,
    path = "/api/telemetry/stats",
    tag = "telemetry",
    responses(
        (status = 200, description = "Storage/series counters for telemetry"),
    )
)]
pub async fn get_telemetry_stats_handler(
    State(state): State<ServerState>,
) -> HandlerResult<serde_json::Value> {
    // Get telemetry storage stats
    let telemetry = state.devices.telemetry.clone();
    let telemetry_stats = telemetry.get_stats().await;

    let avg_read_ms = telemetry_stats.avg_read_us() / 1000.0;
    let avg_write_ms = telemetry_stats.avg_write_us() / 1000.0;
    let cache_hit_rate = telemetry_stats.cache_hit_rate();

    // Calculate performance tier
    let performance_tier = if avg_read_ms < 10.0 {
        "excellent"
    } else if avg_read_ms < 50.0 {
        "good"
    } else if avg_read_ms < 200.0 {
        "fair"
    } else {
        "poor"
    };

    ok(json!({
        "performance": {
            "avg_read_ms": avg_read_ms,
            "avg_write_ms": avg_write_ms,
            "performance_tier": performance_tier,
            "cache_hit_rate": cache_hit_rate,
            "read_count": telemetry_stats.read_count,
            "write_count": telemetry_stats.write_count,
        },
        "health": {
            "status": if avg_read_ms < 200.0 { "healthy" } else { "degraded" },
            "recommendations": get_performance_recommendations(avg_read_ms, cache_hit_rate, telemetry_stats.read_count)
        }
    }))
}

/// Get performance recommendations based on stats
fn get_performance_recommendations(
    avg_read_ms: f64,
    cache_hit_rate: f64,
    read_count: u64,
) -> Vec<String> {
    let mut recommendations = Vec::new();

    if avg_read_ms > 200.0 {
        recommendations.push(
            "High read latency detected (>200ms). Consider optimizing database queries."
                .to_string(),
        );
        recommendations.push("Check if redb database file is fragmented.".to_string());
    } else if avg_read_ms > 50.0 {
        recommendations.push("Moderate read latency (>50ms). Monitor for degradation.".to_string());
    }

    if cache_hit_rate < 0.5 && read_count > 100 {
        recommendations.push(format!(
            "Low cache hit rate ({:.1}%). Consider increasing cache size.",
            cache_hit_rate * 100.0
        ));
    }

    if recommendations.is_empty() {
        recommendations.push("Performance is within acceptable ranges.".to_string());
    }

    recommendations
}
