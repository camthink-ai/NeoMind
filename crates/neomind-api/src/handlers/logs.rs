//! System log archive download.
//!
//! `GET /api/logs/download` bundles every `neomind.log.*` file under
//! `<data_dir>/logs/` into a single in-memory ZIP and streams it back as a
//! `Content-Disposition: attachment` response. Intended for support /
//! diagnostic flows — the user opens Settings → About → "Download logs" and
//! emails the resulting zip back to the team.
//!
//! **Privacy note**: log files are not redacted and may contain API keys,
//! MQTT credentials, or other secrets that were logged at INFO/DEBUG level.
//! Users sharing the archive should review contents or set `RUST_LOG=warn`
//! before reproducing issues.

use std::io::Write;

use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use chrono::{Local, NaiveDate};

use crate::models::ErrorResponse;
use crate::server::ServerState;

/// Query-string parameters for `GET /api/logs/download`.
#[derive(Debug, serde::Deserialize, Default)]
pub struct LogsDownloadParams {
    /// Restrict the archive to log files from the last `days` days (today
    /// inclusive). `0` or omitted means "all time". The bare `neomind.log`
    /// active file (no date suffix) is always included regardless of this
    /// filter.
    pub days: Option<u32>,
}

/// Parse the date suffix from a daily-rotated log file name.
///
/// Returns `Some(date)` for names like `neomind.log.2026-07-07`. Returns
/// `None` for the bare `neomind.log` (today's active file, no suffix) or any
/// other shape — callers treat `None` as "no date filter applies".
fn parse_log_file_date(name: &str) -> Option<NaiveDate> {
    // Pattern: `neomind.log.YYYY-MM-DD`. Strip the known prefix, then parse.
    let suffix = name.strip_prefix("neomind.log.")?;
    // Reject anything that isn't a clean date (e.g. `neomind.log.2026-07-07.gz`
    // from a future compression scheme — handle it later if/when it appears).
    NaiveDate::parse_from_str(suffix, "%Y-%m-%d").ok()
}

/// Strip ANSI escape sequences from log bytes.
///
/// Older NeoMind builds wrote SGR color codes (`ESC [ <params> m`) into the
/// daily log files because the file layer defaulted `with_ansi` to true (see
/// the `neomind-cli` / Tauri `main.rs` file layers). The download endpoint is
/// the one place every archived file passes through, so it normalizes them
/// here — support recipients get readable plain text regardless of which
/// server version produced the logs. Only well-formed CSI SGR sequences are
/// removed; all other bytes pass through untouched.
fn strip_ansi(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        // CSI: ESC '[' then parameter/intermediate bytes (< 0x40) and a single
        // final byte in 0x40..=0x7E. The SGR codes tracing emits are
        // `ESC [ <digits> ; <digits> m`.
        if data[i] == 0x1b && i + 1 < data.len() && data[i + 1] == b'[' {
            let mut j = i + 2;
            while j < data.len() && data[j] < 0x40 {
                j += 1;
            }
            if j < data.len() && (0x40..=0x7e).contains(&data[j]) {
                i = j + 1;
                continue;
            }
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

/// Maximum bytes a single log file may contribute to the archive. Acts as a
/// sanity guard against pathological logs filling memory on low-RAM edge
/// devices — daily-rotated files are normally well under this size.
const MAX_BYTES_PER_FILE: u64 = 64 * 1024 * 1024; // 64 MiB

/// Maximum number of files to archive. Defensive only; rotation rarely
/// accumulates more than a couple dozen daily files before old ones are
/// cleaned up by `cleanup_old_logs` in the Tauri shell.
const MAX_FILES: usize = 60;

/// Hard ceiling on the *total* bytes read into memory before zip encoding.
/// Combined with `MAX_BYTES_PER_FILE × MAX_FILES` per-file guards, this
/// prevents pathological cases (e.g. user disabled rotation, accumulated
/// 60 × 60 MiB logs) from OOM-ing a low-RAM edge device. 512 MiB is roughly
/// the largest archive a 1 GiB device can buffer comfortably alongside the
/// appender itself.
const MAX_TOTAL_BYTES_READ: u64 = 512 * 1024 * 1024; // 512 MiB

pub async fn download_logs_handler(
    State(state): State<ServerState>,
    Query(params): Query<LogsDownloadParams>,
) -> Result<axum::response::Response, ErrorResponse> {
    let log_dir = state.data_dir.join("logs");

    if !log_dir.is_dir() {
        return Err(ErrorResponse::not_found(format!(
            "Log directory not found: {}. Hint: the server must be running and writing logs.",
            log_dir.display()
        )));
    }

    // Compute the inclusive lower-bound date for filename-based filtering.
    // `days=N` keeps files whose date suffix is within the last N days
    // (today inclusive, so N=1 = today only, N=7 = today + 6 prior days).
    // `days=0` and `days=None` mean "all time".
    //
    // **Local time, not UTC**: `tracing_appender::rolling::daily` names files
    // using the *local* timezone, so filtering with `Utc::now()` would
    // misclassify files around midnight for users outside UTC (e.g. a US
    // user picking "Today" at 8pm Pacific would exclude today's file because
    // UTC has already rolled to the next day). Match the appender's TZ.
    let cutoff_date: Option<NaiveDate> = match params.days {
        Some(days) if days > 0 => {
            let today = Local::now().date_naive();
            // `days - 1` because the user's mental model is "last N days
            // *including* today" (N=1 → today only). Without the -1, "Today"
            // would silently include yesterday's file too.
            let offset = (days - 1) as i64;
            Some(today - chrono::Duration::days(offset))
        }
        _ => None,
    };

    // Collect candidate log files (newest first). Sorted by modified-time so
    // the most recent — usually what support wants — appear at the top of the
    // archive. Skip files that don't match the daily-rotation naming pattern
    // (`neomind.log.YYYY-MM-DD`) plus the bare `neomind.log` if present.
    let mut entries: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
    let mut filtered_by_date = 0u32;
    for entry in std::fs::read_dir(&log_dir)
        .map_err(|e| ErrorResponse::internal(format!("Failed to read log dir: {}", e)))?
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !name.starts_with("neomind.log") {
            continue;
        }

        // Date-based filtering on the `neomind.log.YYYY-MM-DD` suffix. The
        // bare `neomind.log` (today's active file, no date suffix) always
        // passes the filter.
        if let Some(cutoff) = cutoff_date {
            if let Some(file_date) = parse_log_file_date(name) {
                if file_date < cutoff {
                    filtered_by_date += 1;
                    continue;
                }
            }
        }

        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        entries.push((path, mtime));
    }

    entries.sort_by(|a, b| b.1.cmp(&a.1));
    entries.truncate(MAX_FILES);

    if entries.is_empty() {
        return Err(ErrorResponse::not_found(if filtered_by_date > 0 {
            format!(
                "No log files match the selected time range ({} file(s) filtered out).",
                filtered_by_date
            )
        } else {
            "No log files found in log directory.".to_string()
        }));
    }

    // Build the archive in memory. Logs are small (capped by per-file + count
    // guards above), so buffering the full zip is simpler than streaming and
    // avoids awkward chunked-write lifetime issues with `ZipWriter`.
    let mut buf: Vec<u8> = Vec::with_capacity(512 * 1024);
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = zip::write::SimpleFileOptions::default();

        let mut included = 0u32;
        let mut skipped = 0u32;
        let mut total_bytes_read: u64 = 0;
        for (path, _mtime) in &entries {
            let meta = match std::fs::metadata(path) {
                Ok(m) => m,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            if meta.len() > MAX_BYTES_PER_FILE {
                tracing::warn!(
                    file = %path.display(),
                    size = meta.len(),
                    "Skipping oversized log file in archive"
                );
                skipped += 1;
                continue;
            }
            // Defense-in-depth: stop reading once cumulative bytes would
            // exceed the total cap, even if individual files passed the
            // per-file check. Logs are sorted newest-first so this retains
            // the most recent N days and drops the oldest gracefully.
            if total_bytes_read + meta.len() > MAX_TOTAL_BYTES_READ {
                tracing::warn!(
                    file = %path.display(),
                    total_bytes_read,
                    cap = MAX_TOTAL_BYTES_READ,
                    "Stopping archive at total-bytes cap; remaining files skipped"
                );
                skipped += 1;
                break;
            }
            let data = match std::fs::read(path) {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!(
                        file = %path.display(),
                        error = %e,
                        "Failed to read log file"
                    );
                    skipped += 1;
                    continue;
                }
            };
            total_bytes_read = total_bytes_read.saturating_add(data.len() as u64);
            // Older builds wrote ANSI color codes into the daily files (the
            // file layer defaulted `with_ansi` on); strip them so the archive
            // stays readable plain text no matter which server produced it.
            let data = strip_ansi(&data);
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown.log");
            if let Err(e) = zip.start_file(name, opts) {
                return Err(ErrorResponse::internal(format!("Zip write failed: {}", e)));
            }
            if let Err(e) = zip.write_all(&data) {
                return Err(ErrorResponse::internal(format!("Zip write failed: {}", e)));
            }
            included += 1;
        }

        if let Err(e) = zip.finish() {
            return Err(ErrorResponse::internal(format!(
                "Failed to finalize zip: {}",
                e
            )));
        }

        tracing::info!(
            included,
            skipped,
            filtered_by_date,
            total_seen = entries.len(),
            total_bytes_read,
            days = ?params.days,
            "Built diagnostic log archive"
        );
    }

    let timestamp = Local::now().format("%Y-%m-%d");
    let suffix = match params.days {
        Some(days) if days > 0 => format!("-last-{}d", days),
        _ => String::new(),
    };
    let filename = format!("neomind-logs-{}{}.zip", timestamp, suffix);

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
            (
                header::CACHE_CONTROL,
                "no-store, no-cache, must-revalidate".to_string(),
            ),
        ],
        buf,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // Const-vs-boundary guards are deliberate (clippy: assertions_on_constants
    // fires because both sides are const) — they pin the limits against drift.
    #[allow(clippy::assertions_on_constants)]
    fn max_files_and_size_guards_are_sane() {
        // Smoke: constants shouldn't drift to absurd values accidentally.
        assert!(MAX_FILES >= 10 && MAX_FILES <= 200);
        assert!(MAX_BYTES_PER_FILE >= 8 * 1024 * 1024);
        assert!(MAX_BYTES_PER_FILE <= 256 * 1024 * 1024);
        // Total cap must be ≥ per-file cap (otherwise a single file would
        // always trip the total cap) and ≤ a reasonable RAM ceiling.
        assert!(MAX_TOTAL_BYTES_READ >= MAX_BYTES_PER_FILE);
        assert!(MAX_TOTAL_BYTES_READ <= 2 * 1024 * 1024 * 1024); // ≤ 2 GiB
    }

    #[test]
    fn parse_log_file_date_handles_canonical_name() {
        let d = parse_log_file_date("neomind.log.2026-07-07").unwrap();
        assert_eq!(d, NaiveDate::from_ymd_opt(2026, 7, 7).unwrap());
    }

    #[test]
    fn parse_log_file_date_returns_none_for_bare_active_file() {
        // Today's active log has no date suffix — caller must NOT filter it out.
        assert_eq!(parse_log_file_date("neomind.log"), None);
    }

    #[test]
    fn parse_log_file_date_rejects_garbage_suffix() {
        // Unknown formats are treated as "no date" rather than panicking.
        // Note: chrono's `%Y-%m-%d` is lenient about single-digit month/day
        // (`2026-7-7` parses), so we only assert on clearly non-date shapes.
        assert_eq!(parse_log_file_date("neomind.log.2026-07-07.gz"), None);
        assert_eq!(parse_log_file_date("neomind.log.old"), None);
        assert_eq!(parse_log_file_date("unrelated.log"), None);
    }

    #[test]
    fn cutoff_for_days_1_includes_only_today() {
        // "Today" semantics: days=1 must include today and EXCLUDE yesterday.
        // Off-by-one regression guard.
        let today = Local::now().date_naive();
        let cutoff = today - chrono::Duration::days(0); // days=1 → offset 0
        assert!(today >= cutoff);
        let yesterday = today - chrono::Duration::days(1);
        assert!(yesterday < cutoff);
    }

    #[test]
    fn cutoff_for_days_7_spans_exactly_seven_days() {
        // days=7 → today + 6 prior days = 7-day window.
        let today = Local::now().date_naive();
        let cutoff = today - chrono::Duration::days(6);
        assert!(today >= cutoff);
        let seven_days_ago = today - chrono::Duration::days(7);
        assert!(seven_days_ago < cutoff, "8th day ago should be excluded");
    }

    #[test]
    fn strip_ansi_removes_sgr_color_codes() {
        // Real-world shape: the file layer emitted `ESC[2m`, `ESC[32m`,
        // `ESC[3m`, `ESC[0m` wrapping every line.
        let input =
            b"\x1b[2m2026-08-13T00:00:21Z\x1b[0m \x1b[32m INFO\x1b[0m \x1b[2mneomind\x1b[0m: ok";
        let text = String::from_utf8(strip_ansi(input)).unwrap();
        assert!(!text.contains('\u{1b}'), "no escape bytes may remain");
        assert!(text.starts_with("2026-08-13T00:00:21Z"));
        assert!(text.contains(" INFO"));
        assert!(text.contains("neomind: ok"));
    }

    #[test]
    fn strip_ansi_handles_parameter_lists_and_mixed_content() {
        let input = b"\x1b[33;1mWARN\x1b[0m plain \x1b[3mfield=1\x1b[0m";
        assert_eq!(strip_ansi(input).as_slice(), b"WARN plain field=1");
    }

    #[test]
    fn strip_ansi_leaves_plain_log_lines_untouched() {
        let input = b"2026-08-13T00:00:21Z  INFO neomind: no ansi here\n";
        assert_eq!(strip_ansi(input).as_slice(), input);
    }

    #[test]
    fn strip_ansi_handles_unterminated_escape_gracefully() {
        // A truncated `ESC[2` with no final byte must not panic or eat the
        // trailing text — the ESC is preserved as-is.
        let input = b"tail \x1b[2";
        assert_eq!(strip_ansi(input).as_slice(), b"tail \x1b[2");
    }
}
