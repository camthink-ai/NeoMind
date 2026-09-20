//! `push` — split from the former main.rs monolith.

use super::*;

use std::collections::VecDeque;

use std::io::Write;

use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use tracing::{debug, error, warn};

use neomind_extension_sdk::IpcFrame;

use ipc_routing::STDOUT_WRITE_MUTEX;

/// Maximum queued frames before dropping the oldest.
///
/// 64: a 30 fps H.264 relay with ~300 KB keyframes needs burst headroom
/// far beyond the old 16 — the silent oldest-drop here was one of the
/// measured loss points (49.6% end-to-end at one point).
pub(crate) const PUSH_BUFFER_CAPACITY: usize = 64;

/// Shared push-output buffer: newest frames survive, oldest get dropped.
pub(crate) static PUSH_BUFFER: std::sync::OnceLock<(
    std::sync::Mutex<VecDeque<Vec<u8>>>,
    std::sync::Condvar,
)> = std::sync::OnceLock::new();

/// Counter for dropped push frames (diagnostics).
pub(crate) static PUSH_DROPPED_COUNT: AtomicU64 = AtomicU64::new(0);

/// Background thread that drains the push buffer and writes to stdout.
pub(crate) fn push_stdout_writer_thread() {
    let (lock, cvar) = PUSH_BUFFER.get().expect("PUSH_BUFFER not initialized");
    let mut seq: u64 = 0;
    loop {
        let mut guard = lock.lock().unwrap();
        // Wait until there's at least one frame
        while guard.is_empty() {
            guard = cvar.wait(guard).unwrap();
        }
        // Drain all available frames, keep only the latest
        let batch: Vec<Vec<u8>> = guard.drain(..).collect();
        drop(guard);

        // Write each frame to stdout (under STDOUT_WRITE_MUTEX for inter-task safety)
        for frame in batch {
            let _write_guard = STDOUT_WRITE_MUTEX.lock().unwrap_or_else(|e| {
                error!("STDOUT_WRITE_MUTEX poisoned: {}", e);
                e.into_inner()
            });
            let mut stdout = std::io::stdout();
            if let Err(e) = stdout.write_all(&frame) {
                error!("Push stdout writer: write error: {e}");
                return;
            }
            if let Err(e) = stdout.flush() {
                error!("Push stdout writer: flush error: {e}");
                return;
            }
        }
        seq += 1;

        if seq.is_multiple_of(100) {
            let dropped = PUSH_DROPPED_COUNT.swap(0, AtomicOrdering::Relaxed);
            if dropped > 0 {
                debug!(
                    "Push buffer: dropped {} frames in last 100 batches",
                    dropped
                );
            }
        }
    }
}

/// Push-output writer callback — called by the extension via FFI to push data.
///
/// Enqueues the encoded frame into a bounded buffer. If the buffer is full,
/// the **oldest** frame is dropped so the newest data always gets through.
/// A background thread drains the buffer to stdout, keeping this callback
/// non-blocking regardless of downstream backpressure.
/// Raw push writer: fields arrive as ptr+len slices — NO JSON parse and
/// NO base64 on the payload (the legacy path below pays both). Builds
/// the small segmented header directly and enqueues the frame; the data
/// bytes go from the extension's Vec<u8> into the IPC segment untouched.
pub(crate) unsafe extern "C" fn push_output_raw_writer(
    session_id: *const u8,
    session_id_len: usize,
    sequence: u64,
    data_type: *const u8,
    data_type_len: usize,
    timestamp: i64,
    metadata_json: *const u8,
    metadata_len: usize,
    data: *const u8,
    data_len: usize,
) -> i32 {
    if session_id.is_null() || data.is_null() || data_len == 0 {
        warn!("PushOutputRawWriter: null/empty fields");
        return -1;
    }
    let sid = std::str::from_utf8(std::slice::from_raw_parts(session_id, session_id_len))
        .unwrap_or_default();
    let dtype = std::str::from_utf8(std::slice::from_raw_parts(data_type, data_type_len))
        .unwrap_or("application/octet-stream");
    let metadata: Option<serde_json::Value> = if metadata_len > 0 && !metadata_json.is_null() {
        serde_json::from_slice(std::slice::from_raw_parts(metadata_json, metadata_len)).ok()
    } else {
        None
    };
    let bytes = std::slice::from_raw_parts(data, data_len);
    let header = serde_json::json!({
        "PushOutput": {
            "session_id": sid,
            "sequence": sequence,
            "data_len": data_len,
            "data_type": dtype,
            "timestamp": if timestamp != 0 { timestamp } else { chrono::Utc::now().timestamp_millis() },
            "metadata": metadata,
        }
    });
    let header_bytes = match serde_json::to_vec(&header) {
        Ok(h) => h,
        Err(e) => {
            error!("PushOutputRawWriter: serialise error: {e}");
            return -3;
        }
    };
    let payload = neomind_extension_sdk::encode_segmented_payload(&header_bytes, bytes);
    let frame = IpcFrame::new(payload);
    let encoded = frame.encode();

    let (lock, cvar) = PUSH_BUFFER.get().expect("PUSH_BUFFER not initialized");
    let mut guard = lock.lock().unwrap();
    if guard.len() >= PUSH_BUFFER_CAPACITY {
        guard.pop_front();
        PUSH_DROPPED_COUNT.fetch_add(1, AtomicOrdering::Relaxed);
    }
    guard.push_back(encoded);
    drop(guard);
    cvar.notify_one();
    0
}

pub(crate) unsafe extern "C" fn push_output_writer(data: *const u8, len: usize) -> i32 {
    if data.is_null() || len == 0 {
        warn!("PushOutputWriter: null/empty data");
        return -1;
    }

    let bytes = std::slice::from_raw_parts(data, len);
    let msg: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(e) => {
            warn!("PushOutputWriter: invalid JSON: {e}");
            return -2;
        }
    };

    // SEGMENTED BINARY (perf-critical): the FFI payload carries `data` as
    // a base64 string. The historical path re-embedded it as base64 in
    // JSON (and older builds even decoded+re-encoded it) — multiple full
    // codec passes per 40-300 KB frame capped the relay at ~20 fps with
    // 49.6% measured loss. Now: ONE base64 decode here, then the payload
    // is `[header_len][small header JSON with data_len][raw bytes]` — the
    // core side parses the small header and takes the bytes verbatim
    // (see neomind_extension_sdk::parse_response_payload).
    let data: Vec<u8> = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode(msg.get("data").and_then(|v| v.as_str()).unwrap_or_default())
            .unwrap_or_default()
    };
    let header = serde_json::to_vec(&serde_json::json!({
        "PushOutput": {
            "session_id": msg.get("session_id")
                .and_then(|v| v.as_str()).unwrap_or_default(),
            "sequence": msg.get("sequence")
                .and_then(|v| v.as_u64()).unwrap_or(0),
            "data_len": data.len(),
            "data_type": msg.get("data_type")
                .and_then(|v| v.as_str()).unwrap_or("application/octet-stream"),
            "timestamp": msg.get("timestamp")
                .and_then(|v| v.as_i64())
                .unwrap_or_else(|| chrono::Utc::now().timestamp_millis()),
            "metadata": msg.get("metadata").cloned(),
        }
    }))
    .map_err(|e| {
        error!("PushOutputWriter: serialise error: {e}");
        e
    });
    let header = match header {
        Ok(p) => p,
        Err(_) => return -3,
    };
    let payload = neomind_extension_sdk::encode_segmented_payload(&header, &data);

    let frame = IpcFrame::new(payload);
    let encoded = frame.encode();

    // Enqueue into bounded buffer (non-blocking).
    // If full, drop the oldest frame so the newest always gets through.
    let (lock, cvar) = PUSH_BUFFER.get().expect("PUSH_BUFFER not initialized");
    let mut guard = lock.lock().unwrap();
    if guard.len() >= PUSH_BUFFER_CAPACITY {
        guard.pop_front();
        PUSH_DROPPED_COUNT.fetch_add(1, AtomicOrdering::Relaxed);
    }
    guard.push_back(encoded);
    drop(guard);
    cvar.notify_one();

    0
}

pub(crate) type RegisterPushWriterFn =
    unsafe extern "C" fn(neomind_extension_sdk::PushOutputWriterFn) -> i32;
pub(crate) type RegisterPushWriterRawFn =
    unsafe extern "C" fn(neomind_extension_sdk::PushOutputRawWriterFn) -> i32;
