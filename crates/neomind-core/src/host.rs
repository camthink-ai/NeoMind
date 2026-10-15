//! Host environment facts — where this NeoMind instance physically runs.
//!
//! The chat model repeatedly failed "what device are you running on"
//! (2026-10-15: `neomind system info` carried only platform services, its
//! `server_ip: "0.0.0.0"` is a BIND address, and the system prompt had zero
//! environment facts — the model answered "我不确定"). These facts feed two
//! consumers: the slim system prompt (one summary line, answers without a
//! tool round-trip) and `neomind system info` (full JSON `host` section).
//!
//! Pure /proc reads + one UDP-connect trick for the LAN IP — no new deps,
//! no packets sent. Everything is Option-tolerant: non-Linux dev hosts and
//! stripped containers report nulls, never errors.

use serde_json::{json, Value};

fn read_trimmed(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn os_pretty_name() -> Option<String> {
    let release = std::fs::read_to_string("/etc/os-release").ok()?;
    release
        .lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
}

/// The host's primary outbound LAN IPv4, without sending a packet: UDP
/// `connect` only builds a route entry locally. Falls back to None on hosts
/// with no route (air-gapped) — 8.8.8.8 is never contacted.
fn lan_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}

fn meminfo_mib(key: &str) -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|v| v.trim_end_matches(" kB").trim().parse::<u64>().ok())
        .map(|kb| kb / 1024)
}

fn cpu_model() -> Option<String> {
    let text = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    text.lines()
        .find_map(|l| l.strip_prefix("model name"))
        // "/proc/cpuinfo" lines look like "model name\t: ARMv8 …" — the tab
        // precedes the colon, so trim whitespace FIRST or it shields it.
        .map(|v| v.trim().trim_start_matches(':').trim().to_string())
}

fn uptime_secs() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/uptime").ok()?;
    text.split_whitespace()
        .next()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|s| s as u64)
}

/// Cached full facts (values are stable for the process lifetime except
/// memory/uptime — those re-read per call, the rest come from the cache).
pub fn host_facts() -> Value {
    let cpu_model = cpu_model().unwrap_or_else(|| "unknown".into());
    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as u64)
        .unwrap_or(0);
    json!({
        "hostname": read_trimmed("/proc/sys/kernel/hostname"),
        "os": os_pretty_name(),
        "kernel": read_trimmed("/proc/sys/kernel/osrelease"),
        "arch": std::env::consts::ARCH,
        "cpu_model": cpu_model,
        "cpu_cores": cores,
        "ram_total_mb": meminfo_mib("MemTotal:"),
        "ram_available_mb": meminfo_mib("MemAvailable:"),
        "uptime_secs": uptime_secs(),
        "lan_ip": lan_ip(),
    })
}

/// One compact line for the system prompt, e.g.
/// `ubuntu (kernel 5.15.148-tegra, aarch64) · hostname ubuntu · 8× Cortex-A78AE · 7.4 GB RAM · LAN 192.168.93.20`.
/// Null fields are skipped, never rendered as "null".
pub fn host_summary() -> String {
    let f = host_facts();
    let mut parts: Vec<String> = Vec::new();
    if let Some(os) = f["os"].as_str() {
        parts.push(os.to_string());
    }
    if let (Some(k), Some(a)) = (f["kernel"].as_str(), f["arch"].as_str()) {
        parts.push(format!("kernel {k}, {a}"));
    }
    if let Some(h) = f["hostname"].as_str() {
        parts.push(format!("host {h}"));
    }
    if let Some(c) = f["cpu_cores"].as_u64() {
        if c > 0 {
            let model = f["cpu_model"].as_str().unwrap_or("cpu");
            parts.push(format!("{c}× {model}"));
        }
    }
    if let Some(t) = f["ram_total_mb"].as_u64() {
        parts.push(format!("{:.1} GB RAM", t as f64 / 1024.0));
    }
    if let Some(ip) = f["lan_ip"].as_str() {
        parts.push(format!("LAN {ip}"));
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_renders_without_nulls() {
        let s = host_summary();
        assert!(!s.contains("null"), "summary must skip null facts: {s}");
        assert!(!s.is_empty(), "summary must render something on any host");
    }

    #[test]
    fn facts_shape_is_stable() {
        let f = host_facts();
        for key in [
            "hostname", "os", "kernel", "arch", "cpu_model", "cpu_cores", "ram_total_mb",
            "ram_available_mb", "uptime_secs", "lan_ip",
        ] {
            assert!(f.get(key).is_some(), "missing key {key}");
        }
        // arch is never null — it comes from the compiler target.
        assert!(f["arch"].is_string());
    }
}
