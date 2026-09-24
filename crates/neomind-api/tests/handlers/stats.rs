//! Tests for the system stats handler — what the About page reads.
//!
//! Everything it used to report describes the *box*: total memory, machine-wide
//! CPU, every disk and every interface. The page could therefore say "this
//! machine is at 42%" and never "NeoMind is holding 312 MB of it", which on a
//! small edge device is the number an operator is actually asking about.

use axum::extract::State;
use neomind_api::handlers::stats::get_system_stats_handler;
use neomind_api::handlers::ServerState;

async fn create_test_server_state() -> ServerState {
    crate::common::create_test_server_state().await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn stats() -> serde_json::Value {
        get_system_stats_handler(State(create_test_server_state().await))
            .await
            .expect("the handler must not error")
            .0
            .data
            .expect("the envelope carries data")
    }

    #[tokio::test]
    async fn the_stats_carry_this_process_own_footprint() {
        let body = stats().await;

        let process = body["process"]
            .as_object()
            .expect("a process block — the About page renders it whenever it is present");

        // The pid has to be OURS. Anything else means sysinfo was asked about a
        // different process and every number below describes that one.
        assert_eq!(
            process["pid"].as_u64(),
            Some(std::process::id() as u64),
            "the block must describe this process, not some other: {process:?}"
        );
        assert!(
            process["memory_bytes"].as_u64().unwrap_or(0) > 0,
            "a running process is holding memory: {process:?}"
        );
        // `threads` is Linux-only: `sysinfo::Process::tasks` returns `None` on
        // every other platform, so the field is optional and this asserts the
        // shape of the answer rather than a count. The first version defaulted
        // it to 1, which made this assertion pass on macOS against a number
        // nothing had measured.
        #[cfg(target_os = "linux")]
        assert!(
            process["threads"].as_u64().unwrap_or(0) >= 1,
            "on Linux a process has at least its main thread: {process:?}"
        );
        #[cfg(not(target_os = "linux"))]
        assert!(
            process["threads"].is_null(),
            "the OS does not report thread counts here, so it must be absent \
             rather than invented: {process:?}"
        );
        assert!(
            process["uptime_secs"].as_u64().is_some(),
            "the process's own run time must be present: {process:?}"
        );
    }

    /// The process CPU figure is optional, and absent for a reason.
    ///
    /// It is a delta, and there is nothing to difference against until one
    /// sample has been taken — so the first call after boot says nothing rather
    /// than reporting a zero that reads like a measurement. That the *second*
    /// call carries a real figure is asserted against the sampler itself in
    /// `stats.rs`'s unit tests, where a sleep can be taken without going through
    /// the handler's five-second cache.
    #[tokio::test]
    async fn the_process_cpu_reading_is_present_or_honestly_absent() {
        let body = stats().await;
        let cpu = &body["process"]["cpu_usage"];
        assert!(
            cpu.is_null() || cpu.as_f64().is_some_and(|v| v.is_finite() && v >= 0.0),
            "a cpu reading has to be a finite number or absent, not {cpu:?}"
        );
    }

    #[tokio::test]
    async fn the_machine_wide_fields_are_untouched() {
        // This change adds a block; it does not replace one. A payload that lost
        // a field would render an empty gauge rather than fail.
        let body = stats().await;
        for key in [
            "cpu_usage",
            "total_memory",
            "used_memory",
            "free_memory",
            "available_memory",
            "cpu_count",
            "disks",
            "networks",
            "data_dir",
        ] {
            assert!(
                body.get(key).is_some(),
                "`{key}` went missing from the payload"
            );
        }
    }
}
