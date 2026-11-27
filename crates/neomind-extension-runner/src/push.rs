//! `push` — split from the former main.rs monolith.

use super::*;

use std::collections::VecDeque;

use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use tracing::{debug, error, warn};

use neomind_extension_sdk::IpcFrame;

use ipc_routing::write_ipc_frame;

/// Maximum queued frames before dropping the oldest.
///
/// 64: a 30 fps H.264 relay with ~300 KB keyframes needs burst headroom
/// far beyond the old 16 — the silent oldest-drop here was one of the
/// measured loss points (49.6% end-to-end at one point).
pub(crate) const PUSH_BUFFER_CAPACITY: usize = 64;

/// Hard caps on FFI push frame field sizes to prevent OOM or invalid slice UB.
pub(crate) const MAX_PUSH_DATA_LEN: usize = 8 * 1024 * 1024;
pub(crate) const MAX_PUSH_SESSION_ID_LEN: usize = 256;
pub(crate) const MAX_PUSH_DATA_TYPE_LEN: usize = 256;
pub(crate) const MAX_PUSH_METADATA_LEN: usize = 64 * 1024;
pub(crate) const MAX_PUSH_JSON_LEN: usize = 12 * 1024 * 1024;

/// Shared push-output buffer: newest frames survive, oldest get dropped.
pub(crate) static PUSH_BUFFER: std::sync::OnceLock<(
    std::sync::Mutex<VecDeque<Vec<u8>>>,
    std::sync::Condvar,
)> = std::sync::OnceLock::new();

/// Lazily initialize and return the shared push buffer. Safe to call from
/// `extern "C"` callbacks even before `Runner::run()` starts the drain thread.
pub(crate) fn push_buffer() -> &'static (std::sync::Mutex<VecDeque<Vec<u8>>>, std::sync::Condvar) {
    PUSH_BUFFER.get_or_init(|| {
        (
            std::sync::Mutex::new(VecDeque::with_capacity(PUSH_BUFFER_CAPACITY)),
            std::sync::Condvar::new(),
        )
    })
}

/// Counter for dropped push frames (diagnostics).
pub(crate) static PUSH_DROPPED_COUNT: AtomicU64 = AtomicU64::new(0);

/// Background thread that drains the push buffer and writes to the private IPC channel.
pub(crate) fn push_stdout_writer_thread() {
    let (lock, cvar) = push_buffer();
    let mut seq: u64 = 0;
    loop {
        let mut guard = lock.lock().unwrap_or_else(|e| {
            error!("PUSH_BUFFER mutex poisoned in writer thread: {}", e);
            e.into_inner()
        });
        // Wait until there's at least one frame
        while guard.is_empty() {
            guard = cvar.wait(guard).unwrap_or_else(|e| {
                error!("PUSH_BUFFER condvar wait poisoned: {}", e);
                e.into_inner()
            });
        }
        // Drain all available frames, keep only the latest
        let batch: Vec<Vec<u8>> = guard.drain(..).collect();
        drop(guard);

        // Write each frame to the private IPC output channel
        for frame in batch {
            if let Err(e) = write_ipc_frame(&frame) {
                error!("Push stdout writer: write error: {e}");
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
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if session_id.is_null() || session_id_len == 0 || data.is_null() || data_len == 0 {
            warn!("PushOutputRawWriter: null/empty fields");
            return -1;
        }
        if session_id_len > MAX_PUSH_SESSION_ID_LEN
            || data_type_len > MAX_PUSH_DATA_TYPE_LEN
            || metadata_len > MAX_PUSH_METADATA_LEN
            || data_len > MAX_PUSH_DATA_LEN
        {
            warn!(
                session_id_len,
                data_type_len,
                metadata_len,
                data_len,
                "PushOutputRawWriter: frame field exceeds cap"
            );
            return -4;
        }

        let sid = std::str::from_utf8(std::slice::from_raw_parts(session_id, session_id_len))
            .unwrap_or_default();
        let dtype = if data_type.is_null() || data_type_len == 0 {
            "application/octet-stream"
        } else {
            std::str::from_utf8(std::slice::from_raw_parts(data_type, data_type_len))
                .unwrap_or("application/octet-stream")
        };
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

        let (lock, cvar) = push_buffer();
        let mut guard = lock.lock().unwrap_or_else(|e| {
            error!("PUSH_BUFFER mutex poisoned in raw writer: {}", e);
            e.into_inner()
        });
        if guard.len() >= PUSH_BUFFER_CAPACITY {
            guard.pop_front();
            PUSH_DROPPED_COUNT.fetch_add(1, AtomicOrdering::Relaxed);
        }
        guard.push_back(encoded);
        drop(guard);
        cvar.notify_one();
        0
    }));

    match result {
        Ok(code) => code,
        Err(_) => {
            error!("PushOutputRawWriter panicked across FFI boundary");
            -99
        }
    }
}

pub(crate) unsafe extern "C" fn push_output_writer(data: *const u8, len: usize) -> i32 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if data.is_null() || len == 0 {
            warn!("PushOutputWriter: null/empty data");
            return -1;
        }
        if len > MAX_PUSH_JSON_LEN {
            warn!(
                len,
                max = MAX_PUSH_JSON_LEN,
                "PushOutputWriter: payload exceeds cap"
            );
            return -4;
        }

        let bytes = std::slice::from_raw_parts(data, len);
        let msg: serde_json::Value = match serde_json::from_slice(bytes) {
            Ok(v) => v,
            Err(e) => {
                warn!("PushOutputWriter: invalid JSON: {e}");
                return -2;
            }
        };

        let session_id = msg
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let data_type = msg
            .get("data_type")
            .and_then(|v| v.as_str())
            .unwrap_or("application/octet-stream");
        if session_id.len() > MAX_PUSH_SESSION_ID_LEN || data_type.len() > MAX_PUSH_DATA_TYPE_LEN {
            warn!(
                session_id_len = session_id.len(),
                data_type_len = data_type.len(),
                "PushOutputWriter: header field exceeds cap"
            );
            return -4;
        }

        // SEGMENTED BINARY (perf-critical): the FFI payload carries `data` as
        // a base64 string. The historical path re-embedded it as base64 in
        // JSON (and older builds even decoded+re-encoded it) — multiple full
        // codec passes per 40-300 KB frame capped the relay at ~20 fps with
        // 49.6% measured loss. Now: ONE base64 decode here, then the payload
        // is `[header_len][small header JSON with data_len][raw bytes]` — the
        // core side parses the small header and takes the bytes verbatim
        // (see neomind_extension_sdk::parse_response_payload).
        let decoded: Vec<u8> = {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(msg.get("data").and_then(|v| v.as_str()).unwrap_or_default())
                .unwrap_or_default()
        };
        if decoded.len() > MAX_PUSH_DATA_LEN {
            warn!(
                data_len = decoded.len(),
                max = MAX_PUSH_DATA_LEN,
                "PushOutputWriter: decoded payload exceeds cap"
            );
            return -4;
        }

        let header = serde_json::to_vec(&serde_json::json!({
            "PushOutput": {
                "session_id": session_id,
                "sequence": msg.get("sequence")
                    .and_then(|v| v.as_u64()).unwrap_or(0),
                "data_len": decoded.len(),
                "data_type": data_type,
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
        let payload = neomind_extension_sdk::encode_segmented_payload(&header, &decoded);

        let frame = IpcFrame::new(payload);
        let encoded = frame.encode();

        // Enqueue into bounded buffer (non-blocking).
        // If full, drop the oldest frame so the newest always gets through.
        let (lock, cvar) = push_buffer();
        let mut guard = lock.lock().unwrap_or_else(|e| {
            error!("PUSH_BUFFER mutex poisoned in writer: {}", e);
            e.into_inner()
        });
        if guard.len() >= PUSH_BUFFER_CAPACITY {
            guard.pop_front();
            PUSH_DROPPED_COUNT.fetch_add(1, AtomicOrdering::Relaxed);
        }
        guard.push_back(encoded);
        drop(guard);
        cvar.notify_one();

        0
    }));

    match result {
        Ok(code) => code,
        Err(_) => {
            error!("PushOutputWriter panicked across FFI boundary");
            -99
        }
    }
}

pub(crate) type RegisterPushWriterFn =
    unsafe extern "C" fn(neomind_extension_sdk::PushOutputWriterFn) -> i32;
pub(crate) type RegisterPushWriterRawFn =
    unsafe extern "C" fn(neomind_extension_sdk::PushOutputRawWriterFn) -> i32;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_writer_handles_null_data_type_without_ub() {
        let sid = b"sess-1";
        let payload = b"hello";
        let rc = unsafe {
            push_output_raw_writer(
                sid.as_ptr(),
                sid.len(),
                1,
                std::ptr::null(),
                0,
                12345,
                std::ptr::null(),
                0,
                payload.as_ptr(),
                payload.len(),
            )
        };
        assert_eq!(rc, 0);

        // Even with non-zero data_type_len and null data_type pointer, must not UB
        let rc2 = unsafe {
            push_output_raw_writer(
                sid.as_ptr(),
                sid.len(),
                2,
                std::ptr::null(),
                16,
                12346,
                std::ptr::null(),
                0,
                payload.as_ptr(),
                payload.len(),
            )
        };
        assert_eq!(rc2, 0);
    }

    #[test]
    fn raw_writer_enforces_frame_size_caps() {
        let sid = b"sess-cap";
        let payload = b"x";
        // Oversized data_len
        let rc = unsafe {
            push_output_raw_writer(
                sid.as_ptr(),
                sid.len(),
                1,
                std::ptr::null(),
                0,
                0,
                std::ptr::null(),
                0,
                payload.as_ptr(),
                MAX_PUSH_DATA_LEN + 1,
            )
        };
        assert_eq!(rc, -4);

        // Oversized session_id_len
        let rc_sid = unsafe {
            push_output_raw_writer(
                sid.as_ptr(),
                MAX_PUSH_SESSION_ID_LEN + 1,
                1,
                std::ptr::null(),
                0,
                0,
                std::ptr::null(),
                0,
                payload.as_ptr(),
                payload.len(),
            )
        };
        assert_eq!(rc_sid, -4);

        // Oversized data_type_len
        let rc_dt = unsafe {
            push_output_raw_writer(
                sid.as_ptr(),
                sid.len(),
                1,
                b"text/plain".as_ptr(),
                MAX_PUSH_DATA_TYPE_LEN + 1,
                0,
                std::ptr::null(),
                0,
                payload.as_ptr(),
                payload.len(),
            )
        };
        assert_eq!(rc_dt, -4);

        // Oversized metadata_len
        let rc_meta = unsafe {
            push_output_raw_writer(
                sid.as_ptr(),
                sid.len(),
                1,
                std::ptr::null(),
                0,
                0,
                b"{}".as_ptr(),
                MAX_PUSH_METADATA_LEN + 1,
                payload.as_ptr(),
                payload.len(),
            )
        };
        assert_eq!(rc_meta, -4);
    }
}
