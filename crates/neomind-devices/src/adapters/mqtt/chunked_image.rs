//! MQTT Chunked Image Upload Reassembler (`>= 1 MiB` protocol).
//!
//! Devices (such as CamThink NE301 firmware) split large JPEG captures (`>= 1 MiB`)
//! into one JSON header message (`type = "image_chunked_header"`) followed by `N`
//! Base64 JSON chunk messages (`type = "image_chunk"`) published on the same uplink
//! topic, correlated by `image_id`.
//!
//! This module buffers and validates incoming headers/chunks with strict memory and
//! TTL bounds, handles out-of-order delivery, pre-header chunks, whole-image
//! retransmits (duplicate header resets stale partial state), and post-completion
//! duplicate suppression (firmware flush-pipeline QoS>=1 ack mismatch).

use base64::Engine as _;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Protocol message type for the chunked image metadata header.
pub(crate) const TYPE_IMAGE_CHUNKED_HEADER: &str = "image_chunked_header";
/// Protocol message type for an individual base64 image chunk.
pub(crate) const TYPE_IMAGE_CHUNK: &str = "image_chunk";

/// Maximum allowed total decoded image size (16 MiB).
pub(crate) const MAX_TOTAL_SIZE: usize = 16 * 1024 * 1024;
/// Maximum allowed number of chunks per image.
pub(crate) const MAX_TOTAL_CHUNKS: usize = 256;
/// Maximum concurrent in-flight chunked image sessions across all devices.
pub(crate) const MAX_ACTIVE_SESSIONS: usize = 16;
/// Maximum total decoded bytes buffered across all in-flight sessions (64 MiB).
pub(crate) const MAX_TOTAL_BUFFERED_BYTES: usize = 64 * 1024 * 1024;
/// Maximum length of an `image_id` string.
const MAX_IMAGE_ID_LEN: usize = 256;
/// Idle timeout for incomplete sessions before eviction.
pub(crate) const SESSION_TTL: Duration = Duration::from_secs(60);
/// Suppression window for retransmissions of an already-completed `(broker, topic, image_id)`.
pub(crate) const COMPLETED_DEDUP_TTL: Duration = Duration::from_secs(60);
/// Cap on remembered completed image IDs to bound memory under heavy traffic.
const MAX_COMPLETED_ENTRIES: usize = 256;

/// Fast byte-level pre-check so normal telemetry and single-frame images skip
/// an extra JSON parse when they cannot possibly be chunked-image packets.
///
/// Both `"image_chunked_header"` and `"image_chunk"` contain the literal ASCII
/// byte sequence `\"image_chunk` (which cannot appear inside base64 image data).
#[inline]
pub(crate) fn payload_may_be_chunked_image(payload: &[u8]) -> bool {
    const NEEDLE: &[u8] = b"\"image_chunk";
    if payload.len() < NEEDLE.len() + 2 {
        return false;
    }
    let trimmed = payload
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .map(|i| &payload[i..])
        .unwrap_or(&[]);
    if !trimmed.starts_with(b"{") {
        return false;
    }
    trimmed.windows(NEEDLE.len()).any(|w| w == NEEDLE)
}

/// Completed, reassembled image payload ready to enter the standard MQTT report pipeline.
#[derive(Debug, Clone)]
pub(crate) struct ReassembledImage {
    pub(crate) image_id: String,
    pub(crate) metadata: Option<Value>,
    pub(crate) ai_result: Option<Value>,
    pub(crate) extra_fields: Map<String, Value>,
    pub(crate) image_bytes: Vec<u8>,
    pub(crate) encoding: String,
}

impl ReassembledImage {
    /// Synthesize a standard single-frame JSON report (`metadata`, `ai_result`,
    /// `image_data`, `encoding`, plus any extra top-level header fields such as
    /// `device_info` or `timestamp`).
    pub(crate) fn to_report_json(&self, image_data_value: String) -> Value {
        let mut obj = self.extra_fields.clone();
        if let Some(ref metadata) = self.metadata {
            obj.insert("metadata".to_string(), metadata.clone());
        }
        if let Some(ref ai_result) = self.ai_result {
            obj.insert("ai_result".to_string(), ai_result.clone());
        }
        obj.insert("image_data".to_string(), Value::String(image_data_value));
        obj.insert("encoding".to_string(), Value::String(self.encoding.clone()));
        Value::Object(obj)
    }

    /// Synthesize a lightweight JSON representation (without multi-MB base64 data)
    /// for device identity resolution and Auto-Onboarding discovery samples.
    pub(crate) fn to_discovery_sample_json(&self) -> Value {
        self.to_report_json(format!("<chunked_image:{} bytes>", self.image_bytes.len()))
    }
}

/// Result of feeding one MQTT JSON message into [`ChunkedImageReassembler`].
#[derive(Debug)]
pub(crate) enum ChunkIngestOutcome {
    /// Not a chunked-image protocol message — process as normal telemetry.
    NotChunked,
    /// Header or chunk buffered; awaiting remaining parts.
    Pending {
        image_id: String,
        received_chunks: usize,
        total_chunks: usize,
    },
    /// Duplicate chunk or retransmission of a recently completed image — drop silently.
    Duplicate { image_id: String },
    /// Malformed packet or resource limit exceeded — log and drop.
    Rejected { reason: String },
    /// All chunks and header received and length-verified.
    Completed(Box<ReassembledImage>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionKey {
    broker_id: String,
    topic: String,
    image_id: String,
}

#[derive(Debug, Clone)]
struct HeaderInfo {
    total_size: usize,
    total_chunks: usize,
    #[allow(dead_code)]
    nominal_chunk_size: usize,
    metadata: Option<Value>,
    ai_result: Option<Value>,
    extra_fields: Map<String, Value>,
}

#[derive(Debug)]
struct ChunkSession {
    header: Option<HeaderInfo>,
    expected_total_chunks: usize,
    chunks: HashMap<usize, Vec<u8>>,
    buffered_bytes: usize,
    encoding: String,
    last_updated: Instant,
}

/// Stateful, bounded reassembler for MQTT chunked image uploads.
#[derive(Debug)]
pub(crate) struct ChunkedImageReassembler {
    sessions: HashMap<SessionKey, ChunkSession>,
    completed: HashMap<SessionKey, Instant>,
    total_buffered_bytes: usize,
    max_total_size: usize,
    max_total_chunks: usize,
    max_active_sessions: usize,
    max_total_buffered_bytes: usize,
    session_ttl: Duration,
    completed_dedup_ttl: Duration,
}

impl Default for ChunkedImageReassembler {
    fn default() -> Self {
        Self::new()
    }
}

impl ChunkedImageReassembler {
    pub(crate) fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            completed: HashMap::new(),
            total_buffered_bytes: 0,
            max_total_size: MAX_TOTAL_SIZE,
            max_total_chunks: MAX_TOTAL_CHUNKS,
            max_active_sessions: MAX_ACTIVE_SESSIONS,
            max_total_buffered_bytes: MAX_TOTAL_BUFFERED_BYTES,
            session_ttl: SESSION_TTL,
            completed_dedup_ttl: COMPLETED_DEDUP_TTL,
        }
    }

    #[cfg(test)]
    fn with_limits(
        max_total_size: usize,
        max_total_chunks: usize,
        max_active_sessions: usize,
        max_total_buffered_bytes: usize,
        session_ttl: Duration,
        completed_dedup_ttl: Duration,
    ) -> Self {
        Self {
            sessions: HashMap::new(),
            completed: HashMap::new(),
            total_buffered_bytes: 0,
            max_total_size,
            max_total_chunks,
            max_active_sessions,
            max_total_buffered_bytes,
            session_ttl,
            completed_dedup_ttl,
        }
    }

    /// Evict expired sessions and stale completed-image dedup entries using `Instant::now()`.
    pub(crate) fn evict_expired_now(&mut self) -> usize {
        self.evict_expired(Instant::now())
    }

    fn evict_expired(&mut self, now: Instant) -> usize {
        let session_ttl = self.session_ttl;
        let before = self.sessions.len();
        let mut freed_bytes = 0usize;
        self.sessions.retain(|_, session| {
            let keep = now.saturating_duration_since(session.last_updated) < session_ttl;
            if !keep {
                freed_bytes += session.buffered_bytes;
            }
            keep
        });
        self.total_buffered_bytes = self.total_buffered_bytes.saturating_sub(freed_bytes);

        let completed_ttl = self.completed_dedup_ttl;
        self.completed
            .retain(|_, ts| now.saturating_duration_since(*ts) < completed_ttl);

        before.saturating_sub(self.sessions.len())
    }

    fn remove_session(&mut self, key: &SessionKey) -> Option<ChunkSession> {
        let removed = self.sessions.remove(key)?;
        self.total_buffered_bytes = self
            .total_buffered_bytes
            .saturating_sub(removed.buffered_bytes);
        Some(removed)
    }

    fn evict_oldest_session(&mut self, exclude: Option<&SessionKey>) -> bool {
        let oldest_key = self
            .sessions
            .iter()
            .filter(|(k, _)| exclude != Some(*k))
            .min_by_key(|(_, s)| s.last_updated)
            .map(|(k, _)| k.clone());
        if let Some(k) = oldest_key {
            self.remove_session(&k);
            true
        } else {
            false
        }
    }

    fn record_completed(&mut self, key: SessionKey, now: Instant) {
        if self.completed.len() >= MAX_COMPLETED_ENTRIES {
            if let Some(oldest) = self
                .completed
                .iter()
                .min_by_key(|(_, ts)| **ts)
                .map(|(k, _)| k.clone())
            {
                self.completed.remove(&oldest);
            }
        }
        self.completed.insert(key, now);
    }

    /// Process a parsed MQTT JSON payload at `Instant::now()`.
    pub(crate) fn ingest_json(
        &mut self,
        broker_id: &str,
        topic: &str,
        json: &Value,
    ) -> ChunkIngestOutcome {
        self.ingest_json_at(broker_id, topic, json, Instant::now())
    }

    fn ingest_json_at(
        &mut self,
        broker_id: &str,
        topic: &str,
        json: &Value,
        now: Instant,
    ) -> ChunkIngestOutcome {
        self.evict_expired(now);

        let Some(obj) = json.as_object() else {
            return ChunkIngestOutcome::NotChunked;
        };
        let Some(msg_type) = obj.get("type").and_then(Value::as_str) else {
            return ChunkIngestOutcome::NotChunked;
        };
        if msg_type != TYPE_IMAGE_CHUNKED_HEADER && msg_type != TYPE_IMAGE_CHUNK {
            return ChunkIngestOutcome::NotChunked;
        }

        let Some(image_id) = obj
            .get("image_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return ChunkIngestOutcome::Rejected {
                reason: "missing or empty image_id".to_string(),
            };
        };
        if image_id.len() > MAX_IMAGE_ID_LEN {
            return ChunkIngestOutcome::Rejected {
                reason: format!(
                    "image_id length {} exceeds limit {}",
                    image_id.len(),
                    MAX_IMAGE_ID_LEN
                ),
            };
        }

        let key = SessionKey {
            broker_id: broker_id.to_string(),
            topic: topic.to_string(),
            image_id: image_id.to_string(),
        };

        if self.completed.contains_key(&key) {
            return ChunkIngestOutcome::Duplicate {
                image_id: image_id.to_string(),
            };
        }

        match msg_type {
            TYPE_IMAGE_CHUNKED_HEADER => self.ingest_header(key, obj, now),
            TYPE_IMAGE_CHUNK => self.ingest_chunk(key, obj, now),
            _ => ChunkIngestOutcome::NotChunked,
        }
    }

    fn ingest_header(
        &mut self,
        key: SessionKey,
        obj: &Map<String, Value>,
        now: Instant,
    ) -> ChunkIngestOutcome {
        let Some(total_size) = obj
            .get("total_size")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
        else {
            return ChunkIngestOutcome::Rejected {
                reason: "header missing valid total_size".to_string(),
            };
        };
        let Some(total_chunks) = obj
            .get("total_chunks")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
        else {
            return ChunkIngestOutcome::Rejected {
                reason: "header missing valid total_chunks".to_string(),
            };
        };
        let Some(chunk_size) = obj
            .get("chunk_size")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
        else {
            return ChunkIngestOutcome::Rejected {
                reason: "header missing valid chunk_size".to_string(),
            };
        };

        if total_size == 0 || total_size > self.max_total_size {
            return ChunkIngestOutcome::Rejected {
                reason: format!(
                    "header total_size {} out of bounds (1..={})",
                    total_size, self.max_total_size
                ),
            };
        }
        if total_chunks == 0 || total_chunks > self.max_total_chunks {
            return ChunkIngestOutcome::Rejected {
                reason: format!(
                    "header total_chunks {} out of bounds (1..={})",
                    total_chunks, self.max_total_chunks
                ),
            };
        }
        if chunk_size == 0 || chunk_size > self.max_total_size {
            return ChunkIngestOutcome::Rejected {
                reason: format!(
                    "header chunk_size {} out of bounds (1..={})",
                    chunk_size, self.max_total_size
                ),
            };
        }

        let expected_chunks = total_size.div_ceil(chunk_size);
        if total_chunks != expected_chunks {
            return ChunkIngestOutcome::Rejected {
                reason: format!(
                    "header total_chunks {} inconsistent with ceil({}/{})={}",
                    total_chunks, total_size, chunk_size, expected_chunks
                ),
            };
        }

        let metadata = obj.get("metadata").cloned();
        let ai_result = obj.get("ai_result").cloned();
        let mut extra_fields = Map::new();
        for (k, v) in obj {
            if !matches!(
                k.as_str(),
                "type"
                    | "image_id"
                    | "total_size"
                    | "total_chunks"
                    | "chunk_size"
                    | "metadata"
                    | "ai_result"
            ) {
                extra_fields.insert(k.clone(), v.clone());
            }
        }

        let header = HeaderInfo {
            total_size,
            total_chunks,
            nominal_chunk_size: chunk_size,
            metadata,
            ai_result,
            extra_fields,
        };

        // Spec §6.4: A duplicate header for the same in-flight image_id means
        // the device restarted a whole-image retransmit -> discard stale state.
        // If chunks arrived BEFORE the first header and match total_chunks,
        // attach the header to them instead of discarding.
        let reset_existing = self.sessions.get(&key).map(|s| {
            s.header.is_some()
                || s.expected_total_chunks != total_chunks
                || s.buffered_bytes > total_size
        });
        if reset_existing == Some(true) {
            self.remove_session(&key);
        }

        if let Some(session) = self.sessions.get_mut(&key) {
            session.header = Some(header);
            session.last_updated = now;
        } else {
            if self.sessions.len() >= self.max_active_sessions {
                self.evict_oldest_session(None);
            }
            self.sessions.insert(
                key.clone(),
                ChunkSession {
                    header: Some(header),
                    expected_total_chunks: total_chunks,
                    chunks: HashMap::new(),
                    buffered_bytes: 0,
                    encoding: "base64".to_string(),
                    last_updated: now,
                },
            );
        }

        self.try_complete(&key, now)
    }

    fn ingest_chunk(
        &mut self,
        key: SessionKey,
        obj: &Map<String, Value>,
        now: Instant,
    ) -> ChunkIngestOutcome {
        let Some(chunk_index) = obj
            .get("chunk_index")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
        else {
            return ChunkIngestOutcome::Rejected {
                reason: "chunk missing valid chunk_index".to_string(),
            };
        };
        let Some(total_chunks) = obj
            .get("total_chunks")
            .and_then(Value::as_u64)
            .map(|v| v as usize)
        else {
            return ChunkIngestOutcome::Rejected {
                reason: "chunk missing valid total_chunks".to_string(),
            };
        };
        if total_chunks == 0 || total_chunks > self.max_total_chunks {
            return ChunkIngestOutcome::Rejected {
                reason: format!(
                    "chunk total_chunks {} out of bounds (1..={})",
                    total_chunks, self.max_total_chunks
                ),
            };
        }
        if chunk_index >= total_chunks {
            return ChunkIngestOutcome::Rejected {
                reason: format!(
                    "chunk_index {} out of bounds for total_chunks {}",
                    chunk_index, total_chunks
                ),
            };
        }

        let encoding = obj
            .get("encoding")
            .and_then(Value::as_str)
            .unwrap_or("base64");
        if !encoding.eq_ignore_ascii_case("base64") {
            return ChunkIngestOutcome::Rejected {
                reason: format!("unsupported chunk encoding '{}'", encoding),
            };
        }

        let Some(data_str) = obj.get("data").and_then(Value::as_str) else {
            return ChunkIngestOutcome::Rejected {
                reason: "chunk missing base64 data string".to_string(),
            };
        };

        // Pre-decode upper bound to avoid allocating huge buffers on adversarial payloads.
        let max_b64_len = (self.max_total_size / 3 + 2) * 4 + 1024;
        if data_str.len() > max_b64_len {
            return ChunkIngestOutcome::Rejected {
                reason: format!("chunk base64 length {} exceeds limit", data_str.len()),
            };
        }

        let mut decoded = match decode_chunk_base64(data_str) {
            Some(bytes) if !bytes.is_empty() => bytes,
            _ => {
                return ChunkIngestOutcome::Rejected {
                    reason: "chunk base64 decode failed or empty".to_string(),
                };
            }
        };

        if let Some(declared_size) = obj.get("chunk_size").and_then(Value::as_u64) {
            let declared_size = declared_size as usize;
            if declared_size == 0 || declared_size > self.max_total_size {
                return ChunkIngestOutcome::Rejected {
                    reason: format!(
                        "chunk declared chunk_size {} out of bounds (1..={})",
                        declared_size, self.max_total_size
                    ),
                };
            }
            if decoded.len() != declared_size {
                // NE301 firmware base64-encodes chunks in 3-byte blocks without
                // converting trailing zero-pad slots to '=' when `chunk_size % 3 != 0`
                // (e.g. 102400 -> 102402 decoded bytes, 31901 -> 31902 decoded bytes).
                // Trim the 1..=2 trailing block-padding bytes so they are never
                // spliced into the middle of the reassembled JPEG bitstream.
                let rounded_up = declared_size.div_ceil(3) * 3;
                if decoded.len() == rounded_up {
                    if decoded[declared_size..].iter().any(|&b| b != 0) {
                        tracing::debug!(
                            declared_size,
                            decoded_len = decoded.len(),
                            tail = ?&decoded[declared_size..],
                            "Trimming non-zero 3-byte base64 block tail from MQTT image chunk"
                        );
                    }
                    decoded.truncate(declared_size);
                } else {
                    return ChunkIngestOutcome::Rejected {
                        reason: format!(
                            "chunk decoded size {} != declared chunk_size {}",
                            decoded.len(),
                            declared_size
                        ),
                    };
                }
            }
        }

        if decoded.len() > self.max_total_size {
            return ChunkIngestOutcome::Rejected {
                reason: format!(
                    "decoded chunk length {} exceeds max_total_size {}",
                    decoded.len(),
                    self.max_total_size
                ),
            };
        }

        // Validate against existing session if present.
        if let Some(session) = self.sessions.get(&key) {
            if session.expected_total_chunks != total_chunks {
                return ChunkIngestOutcome::Rejected {
                    reason: format!(
                        "chunk total_chunks {} != session total_chunks {}",
                        total_chunks, session.expected_total_chunks
                    ),
                };
            }
            if session.chunks.contains_key(&chunk_index) {
                return ChunkIngestOutcome::Duplicate {
                    image_id: key.image_id,
                };
            }
            let max_for_session = session
                .header
                .as_ref()
                .map(|h| h.total_size)
                .unwrap_or(self.max_total_size);
            if session.buffered_bytes.saturating_add(decoded.len()) > max_for_session {
                return ChunkIngestOutcome::Rejected {
                    reason: format!(
                        "session buffered bytes {} + {} exceeds limit {}",
                        session.buffered_bytes,
                        decoded.len(),
                        max_for_session
                    ),
                };
            }
        } else if self.sessions.len() >= self.max_active_sessions {
            self.evict_oldest_session(None);
        }

        // Enforce global memory cap across all active sessions.
        while self.total_buffered_bytes.saturating_add(decoded.len())
            > self.max_total_buffered_bytes
        {
            if !self.evict_oldest_session(Some(&key)) {
                return ChunkIngestOutcome::Rejected {
                    reason: "chunked image global buffer budget exceeded".to_string(),
                };
            }
        }

        let chunk_len = decoded.len();
        let session = self
            .sessions
            .entry(key.clone())
            .or_insert_with(|| ChunkSession {
                header: None,
                expected_total_chunks: total_chunks,
                chunks: HashMap::new(),
                buffered_bytes: 0,
                encoding: encoding.to_string(),
                last_updated: now,
            });
        session.chunks.insert(chunk_index, decoded);
        session.buffered_bytes += chunk_len;
        session.last_updated = now;
        self.total_buffered_bytes += chunk_len;

        self.try_complete(&key, now)
    }

    fn try_complete(&mut self, key: &SessionKey, now: Instant) -> ChunkIngestOutcome {
        let ready = match self.sessions.get(key) {
            Some(session) => {
                if let Some(ref header) = session.header {
                    session.chunks.len() == header.total_chunks
                } else {
                    false
                }
            }
            None => {
                return ChunkIngestOutcome::Rejected {
                    reason: "session vanished unexpectedly".to_string(),
                }
            }
        };

        if !ready {
            let session = &self.sessions[key];
            return ChunkIngestOutcome::Pending {
                image_id: key.image_id.clone(),
                received_chunks: session.chunks.len(),
                total_chunks: session.expected_total_chunks,
            };
        }

        let mut session = match self.remove_session(key) {
            Some(s) => s,
            None => {
                return ChunkIngestOutcome::Rejected {
                    reason: "session vanished unexpectedly".to_string(),
                };
            }
        };
        let header = session
            .header
            .take()
            .expect("ready checked header.is_some()");

        let mut image_bytes = Vec::with_capacity(header.total_size);
        for idx in 0..header.total_chunks {
            let Some(chunk) = session.chunks.remove(&idx) else {
                return ChunkIngestOutcome::Rejected {
                    reason: format!("missing chunk_index {} during assembly", idx),
                };
            };
            image_bytes.extend_from_slice(&chunk);
        }

        if image_bytes.len() != header.total_size {
            return ChunkIngestOutcome::Rejected {
                reason: format!(
                    "assembled image size {} != header total_size {}",
                    image_bytes.len(),
                    header.total_size
                ),
            };
        }

        self.record_completed(key.clone(), now);

        ChunkIngestOutcome::Completed(Box::new(ReassembledImage {
            image_id: key.image_id.clone(),
            metadata: header.metadata,
            ai_result: header.ai_result,
            extra_fields: header.extra_fields,
            image_bytes,
            encoding: session.encoding,
        }))
    }

    #[cfg(test)]
    fn active_session_count(&self) -> usize {
        self.sessions.len()
    }

    #[cfg(test)]
    fn buffered_bytes(&self) -> usize {
        self.total_buffered_bytes
    }
}

/// Tolerant base64 decode (handles standard padding, missing padding, folded
/// whitespace, and URL-safe alphabet — matching `image_storage.rs`).
fn decode_chunk_base64(raw: &str) -> Option<Vec<u8>> {
    let cleaned: Vec<u8> = raw
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(&cleaned)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&cleaned))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use serde_json::json;

    fn make_header(image_id: &str, total_size: usize, chunk_size: usize) -> Value {
        let total_chunks = total_size.div_ceil(chunk_size);
        json!({
            "type": "image_chunked_header",
            "image_id": image_id,
            "total_size": total_size,
            "total_chunks": total_chunks,
            "chunk_size": chunk_size,
            "metadata": {
                "image_id": image_id,
                "timestamp": 1732680000,
                "format": "jpeg",
                "width": 1920,
                "height": 1080,
                "size": total_size,
                "quality": 85,
                "trigger_type": "pir"
            },
            "ai_result": {
                "model_name": "yolo_v8",
                "inference_time_ms": 45
            }
        })
    }

    fn make_chunk(image_id: &str, idx: usize, total_chunks: usize, bytes: &[u8]) -> Value {
        json!({
            "type": "image_chunk",
            "image_id": image_id,
            "chunk_index": idx,
            "total_chunks": total_chunks,
            "chunk_size": bytes.len(),
            "data": BASE64.encode(bytes),
            "encoding": "base64"
        })
    }

    #[test]
    fn fast_precheck_filters_normal_payloads() {
        assert!(!payload_may_be_chunked_image(b"{\"temperature\":23.5}"));
        assert!(!payload_may_be_chunked_image(b"plain text"));
        assert!(!payload_may_be_chunked_image(
            b"{\"image_data\":\"/9j/4AAQSkZJRg==\"}"
        ));
        assert!(payload_may_be_chunked_image(
            br#"{"type":"image_chunked_header","image_id":"img_1"}"#
        ));
        assert!(payload_may_be_chunked_image(
            br#"  {"type": "image_chunk", "image_id": "img_1"}"#
        ));
    }

    #[test]
    fn happy_path_reassembles_exact_bytes_with_short_last_chunk() {
        let mut r = ChunkedImageReassembler::new();
        let raw: Vec<u8> = (0..250u8).cycle().take(256).collect();
        let chunk_size = 100;
        let now = Instant::now();

        let header = make_header("img_1", raw.len(), chunk_size);
        match r.ingest_json_at("b1", "ne301/AABBCC/upload/report", &header, now) {
            ChunkIngestOutcome::Pending {
                received_chunks: 0,
                total_chunks: 3,
                ..
            } => {}
            other => panic!("expected Pending, got {:?}", other),
        }

        for idx in 0..2 {
            let slice = &raw[idx * chunk_size..(idx + 1) * chunk_size];
            let c = make_chunk("img_1", idx, 3, slice);
            match r.ingest_json_at("b1", "ne301/AABBCC/upload/report", &c, now) {
                ChunkIngestOutcome::Pending {
                    received_chunks,
                    total_chunks: 3,
                    ..
                } => assert_eq!(received_chunks, idx + 1),
                other => panic!("expected Pending at chunk {}, got {:?}", idx, other),
            }
        }

        // Last chunk is 56 bytes (< nominal 100)
        let c2 = make_chunk("img_1", 2, 3, &raw[200..256]);
        match r.ingest_json_at("b1", "ne301/AABBCC/upload/report", &c2, now) {
            ChunkIngestOutcome::Completed(img) => {
                assert_eq!(img.image_id, "img_1");
                assert_eq!(img.image_bytes, raw);
                assert_eq!(img.encoding, "base64");
                assert_eq!(img.metadata.as_ref().unwrap()["trigger_type"], json!("pir"));
                assert_eq!(
                    img.ai_result.as_ref().unwrap()["model_name"],
                    json!("yolo_v8")
                );
            }
            other => panic!("expected Completed, got {:?}", other),
        }

        assert_eq!(r.active_session_count(), 0);
        assert_eq!(r.buffered_bytes(), 0);
    }

    #[test]
    fn handles_out_of_order_and_pre_header_chunks() {
        let mut r = ChunkedImageReassembler::new();
        let raw = b"AAAABBBBCC".to_vec();
        let now = Instant::now();

        // Chunk 2 arrives first (before header)
        let c2 = make_chunk("img_ooo", 2, 3, &raw[8..10]);
        assert!(matches!(
            r.ingest_json_at("b1", "t", &c2, now),
            ChunkIngestOutcome::Pending {
                received_chunks: 1,
                total_chunks: 3,
                ..
            }
        ));

        // Chunk 0 arrives next (still no header)
        let c0 = make_chunk("img_ooo", 0, 3, &raw[0..4]);
        assert!(matches!(
            r.ingest_json_at("b1", "t", &c0, now),
            ChunkIngestOutcome::Pending {
                received_chunks: 2,
                total_chunks: 3,
                ..
            }
        ));

        // Chunk 1 arrives (all chunks present, still waiting for header)
        let c1 = make_chunk("img_ooo", 1, 3, &raw[4..8]);
        assert!(matches!(
            r.ingest_json_at("b1", "t", &c1, now),
            ChunkIngestOutcome::Pending {
                received_chunks: 3,
                total_chunks: 3,
                ..
            }
        ));

        // Header arrives last -> immediately completes!
        let header = make_header("img_ooo", raw.len(), 4);
        match r.ingest_json_at("b1", "t", &header, now) {
            ChunkIngestOutcome::Completed(img) => assert_eq!(img.image_bytes, raw),
            other => panic!("expected Completed on late header, got {:?}", other),
        }
    }

    #[test]
    fn duplicate_header_resets_partial_session_for_retransmit() {
        let mut r = ChunkedImageReassembler::new();
        let now = Instant::now();

        let h = make_header("img_retry", 8, 4);
        r.ingest_json_at("b1", "t", &h, now);
        // Old chunk 0 with stale bytes
        let old_c0 = make_chunk("img_retry", 0, 2, b"OLD0");
        r.ingest_json_at("b1", "t", &old_c0, now);
        assert_eq!(r.buffered_bytes(), 4);

        // Duplicate header -> drops old_c0 and restarts cleanly
        assert!(matches!(
            r.ingest_json_at("b1", "t", &h, now),
            ChunkIngestOutcome::Pending {
                received_chunks: 0,
                total_chunks: 2,
                ..
            }
        ));
        assert_eq!(r.buffered_bytes(), 0);

        // New chunks complete cleanly with new bytes
        r.ingest_json_at("b1", "t", &make_chunk("img_retry", 0, 2, b"NEW0"), now);
        match r.ingest_json_at("b1", "t", &make_chunk("img_retry", 1, 2, b"NEW1"), now) {
            ChunkIngestOutcome::Completed(img) => assert_eq!(img.image_bytes, b"NEW0NEW1"),
            other => panic!("expected Completed, got {:?}", other),
        }
    }

    #[test]
    fn completed_image_retransmit_within_ttl_is_deduplicated() {
        let mut r = ChunkedImageReassembler::new();
        let t0 = Instant::now();

        let h = make_header("img_dup", 4, 4);
        let c0 = make_chunk("img_dup", 0, 1, b"JPEG");
        r.ingest_json_at("b1", "t", &h, t0);
        assert!(matches!(
            r.ingest_json_at("b1", "t", &c0, t0),
            ChunkIngestOutcome::Completed(_)
        ));

        // Flush-pipeline QoS>=1 retransmit 5 seconds later -> Duplicate!
        let t1 = t0 + Duration::from_secs(5);
        assert!(matches!(
            r.ingest_json_at("b1", "t", &h, t1),
            ChunkIngestOutcome::Duplicate { .. }
        ));
        assert!(matches!(
            r.ingest_json_at("b1", "t", &c0, t1),
            ChunkIngestOutcome::Duplicate { .. }
        ));

        // After dedup TTL expires (61s), same image_id is accepted again
        let t2 = t0 + Duration::from_secs(61);
        assert!(matches!(
            r.ingest_json_at("b1", "t", &h, t2),
            ChunkIngestOutcome::Pending { .. }
        ));
    }

    #[test]
    fn rejects_invalid_bounds_and_size_mismatch() {
        let mut r = ChunkedImageReassembler::new();
        let now = Instant::now();

        // total_size > MAX_TOTAL_SIZE
        let huge = json!({
            "type": "image_chunked_header",
            "image_id": "huge",
            "total_size": MAX_TOTAL_SIZE + 1,
            "total_chunks": 200,
            "chunk_size": 102400
        });
        assert!(matches!(
            r.ingest_json_at("b1", "t", &huge, now),
            ChunkIngestOutcome::Rejected { .. }
        ));

        // total_chunks > MAX_TOTAL_CHUNKS
        let too_many = json!({
            "type": "image_chunked_header",
            "image_id": "many",
            "total_size": 3000,
            "total_chunks": MAX_TOTAL_CHUNKS + 1,
            "chunk_size": 10
        });
        assert!(matches!(
            r.ingest_json_at("b1", "t", &too_many, now),
            ChunkIngestOutcome::Rejected { .. }
        ));

        // chunk_index >= total_chunks
        let bad_idx = make_chunk("img_bad", 3, 3, b"abc");
        assert!(matches!(
            r.ingest_json_at("b1", "t", &bad_idx, now),
            ChunkIngestOutcome::Rejected { .. }
        ));

        // declared chunk_size != decoded bytes
        let wrong_len = json!({
            "type": "image_chunk",
            "image_id": "img_badlen",
            "chunk_index": 0,
            "total_chunks": 1,
            "chunk_size": 99,
            "data": BASE64.encode(b"abc"),
            "encoding": "base64"
        });
        assert!(matches!(
            r.ingest_json_at("b1", "t", &wrong_len, now),
            ChunkIngestOutcome::Rejected { .. }
        ));

        // Assembled size != header total_size
        let h = make_header("img_short", 10, 5); // expects 2 chunks totaling 10 bytes
        r.ingest_json_at("b1", "t", &h, now);
        r.ingest_json_at("b1", "t", &make_chunk("img_short", 0, 2, b"12345"), now);
        // Second chunk only has 3 bytes (total 8 != 10)
        assert!(matches!(
            r.ingest_json_at("b1", "t", &make_chunk("img_short", 1, 2, b"678"), now),
            ChunkIngestOutcome::Rejected { .. }
        ));
        assert_eq!(r.active_session_count(), 0);
        assert_eq!(r.buffered_bytes(), 0);
    }

    #[test]
    fn evicts_expired_and_enforces_session_and_memory_caps() {
        let mut r = ChunkedImageReassembler::with_limits(
            1024,
            16,
            2,  // max 2 active sessions
            16, // max 16 bytes global buffer
            Duration::from_secs(10),
            Duration::from_secs(10),
        );
        let t0 = Instant::now();

        // Session 1: 8 bytes
        r.ingest_json_at("b1", "t", &make_header("s1", 16, 8), t0);
        r.ingest_json_at("b1", "t", &make_chunk("s1", 0, 2, b"12345678"), t0);

        // Session 2: 8 bytes (total 16 bytes, 2 sessions)
        let t1 = t0 + Duration::from_secs(1);
        r.ingest_json_at("b1", "t", &make_header("s2", 16, 8), t1);
        r.ingest_json_at("b1", "t", &make_chunk("s2", 0, 2, b"abcdefgh"), t1);
        assert_eq!(r.active_session_count(), 2);
        assert_eq!(r.buffered_bytes(), 16);

        // Session 3 arrives -> evicts oldest (s1) to satisfy session & memory caps
        let t2 = t0 + Duration::from_secs(2);
        r.ingest_json_at("b1", "t", &make_header("s3", 16, 8), t2);
        r.ingest_json_at("b1", "t", &make_chunk("s3", 0, 2, b"ABCDEFGH"), t2);
        assert_eq!(r.active_session_count(), 2);
        assert_eq!(r.buffered_bytes(), 16);

        // Advance past TTL -> all sessions evicted cleanly
        let t3 = t2 + Duration::from_secs(15);
        assert_eq!(r.evict_expired(t3), 2);
        assert_eq!(r.active_session_count(), 0);
        assert_eq!(r.buffered_bytes(), 0);
    }

    #[test]
    fn tolerates_ne301_zero_padded_base64_chunks_without_corrupting_bytes() {
        // Real NE301 firmware emits 4 * ceil(n/3) base64 chars with trailing 'A'
        // instead of '=' when chunk_size % 3 != 0:
        //   chunk 0: 102400 bytes (% 3 == 1) -> decodes to 102402 (+2 zero bytes)
        //   chunk 1:  31901 bytes (% 3 == 2) -> decodes to  31902 (+1 zero byte)
        let mut r = ChunkedImageReassembler::new();
        let chunk0_raw: Vec<u8> = (1u8..=251u8).cycle().take(102_400).collect();
        let chunk1_raw: Vec<u8> = (7u8..=253u8).cycle().take(31_901).collect();
        let mut expected = Vec::with_capacity(chunk0_raw.len() + chunk1_raw.len());
        expected.extend_from_slice(&chunk0_raw);
        expected.extend_from_slice(&chunk1_raw);

        // Simulate firmware encoder that pads to 3-byte multiple with 0x00 (no '=').
        let encode_ne301_buggy = |src: &[u8]| -> String {
            let mut padded = src.to_vec();
            while !padded.len().is_multiple_of(3) {
                padded.push(0);
            }
            BASE64.encode(&padded)
        };

        let now = Instant::now();
        let header = make_header("img_ne301_pad", expected.len(), 102_400);
        assert!(matches!(
            r.ingest_json_at("embedded", "ne301/281991/upload/report", &header, now),
            ChunkIngestOutcome::Pending {
                total_chunks: 2,
                ..
            }
        ));

        let c0 = json!({
            "type": "image_chunk",
            "image_id": "img_ne301_pad",
            "chunk_index": 0,
            "total_chunks": 2,
            "chunk_size": 102_400,
            "data": encode_ne301_buggy(&chunk0_raw),
            "encoding": "base64"
        });
        assert!(matches!(
            r.ingest_json_at("embedded", "ne301/281991/upload/report", &c0, now),
            ChunkIngestOutcome::Pending {
                received_chunks: 1,
                total_chunks: 2,
                ..
            }
        ));

        let c1 = json!({
            "type": "image_chunk",
            "image_id": "img_ne301_pad",
            "chunk_index": 1,
            "total_chunks": 2,
            "chunk_size": 31_901,
            "data": encode_ne301_buggy(&chunk1_raw),
            "encoding": "base64"
        });
        match r.ingest_json_at("embedded", "ne301/281991/upload/report", &c1, now) {
            ChunkIngestOutcome::Completed(img) => {
                assert_eq!(img.image_bytes.len(), expected.len());
                assert_eq!(img.image_bytes, expected);
            }
            other => panic!("expected Completed, got {:?}", other),
        }
    }
}
