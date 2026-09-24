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

    /// The process CPU figure has to be a measurement, not a constant.
    ///
    /// A process's CPU usage is a delta, and a fresh `System` has no previous
    /// sample to subtract: with two refreshes — all the machine-wide figure
    /// needs — this read 0.0% on every platform, every time, which is exactly
    /// what the About page showed. Three refreshes produce a real number; this
    /// is what would notice if someone trimmed the sampling back.
    #[tokio::test]
    async fn the_process_cpu_reading_is_not_a_constant_zero() {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        let burner = std::thread::spawn(move || {
            let mut x: u64 = 1;
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
            }
            std::hint::black_box(x);
        });

        let body = stats().await;
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = burner.join();

        let cpu = body["process"]["cpu_usage"]
            .as_f64()
            .expect("a cpu reading");
        assert!(
            cpu > 0.0,
            "a thread burning a core for the whole sample window still reads 0.0%, so the \
             figure is not a delta: {body:?}"
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
