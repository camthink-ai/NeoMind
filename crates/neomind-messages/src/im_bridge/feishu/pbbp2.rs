//! pbbp2 protobuf frame codec — Feishu WebSocket long-connection transport layer.
//!
//! Feishu's WS long connection wraps every message (ping/pong/event/ack) in a
//! custom protobuf frame protocol called **pbbp2**. This module implements
//! hand-rolled protobuf encode/decode for the two message types (`Frame`,
//! `Header`). It is byte-compatible with `larksuite/node-sdk`'s
//! `ws-client/proto-buf/pbbp2.js`.
//!
//! # pbbp2 proto schema (extracted from larksuite/node-sdk — field numbers are
//! authoritative, not guessed)
//!
//! Source of truth:
//! - `ws-client/proto-buf/pbbp2.js`  (protobufjs-generated encode/decode; tag
//!   constants map 1:1 to field numbers)
//! - `ws-client/proto-buf/pbbp2.d.ts`
//! - `ws-client/enum.ts`
//!
//! Equivalent `.proto`:
//!
//! ```proto
//! message Header {
//!   string key   = 1;   // tag 0x0a (field 1, wire 2 / length-delimited)
//!   string value = 2;   // tag 0x12 (field 2, wire 2)
//! }
//!
//! message Frame {
//!   uint64          SeqID          = 1;  // tag 0x08 (field 1, wire 0 / varint)
//!   uint64          LogID          = 2;  // tag 0x10 (field 2, wire 0)
//!   int32           service        = 3;  // tag 0x18 (field 3, wire 0) — serviceId from conn
//!   int32           method         = 4;  // tag 0x20 (field 4, wire 0) — FrameType enum
//!   repeated Header headers        = 5;  // tag 0x2a (field 5, wire 2) — NOT packed
//!   string          payloadEncoding = 6; // tag 0x32 (field 6, wire 2) — optional
//!   string          payloadType    = 7;  // tag 0x3a (field 7, wire 2) — optional
//!   bytes           payload        = 8;  // tag 0x42 (field 8, wire 2) — optional
//!   string          LogIDNew       = 9;  // tag 0x4a (field 9, wire 2) — optional
//! }
//! ```
//!
//! Tag identity check: tag = (field_number << 3) | wire_type.
//! - SeqID tag 8  = (1<<3)|0 ✓   LogID tag 16 = (2<<3)|0 ✓
//! - service 24   = (3<<3)|0 ✓   method 32   = (4<<3)|0 ✓
//! - headers 42   = (5<<3)|2 ✓   payloadEncoding 50 = (6<<3)|2 ✓
//! - payloadType 58= (7<<3)|2 ✓  payload 66  = (8<<3)|2 ✓
//! - LogIDNew 74  = (9<<3)|2 ✓
//!
//! # Enums (from `ws-client/enum.ts`)
//!
//! - `FrameType`: `control = 0`, `data = 1` (numeric — carried in `method`).
//! - `HeaderKey`: string-valued (`"type"`, `"message_id"`, `"sum"`, `"seq"`,
//!   `"trace_id"`, `"biz_rt"`, `"handshake-status"`, `"handshake-msg"`,
//!   `"handshake-autherrcode"`).
//! - `MessageType`: string-valued (`"event"`, `"card"`, `"ping"`, `"pong"`).
//!
//! `HeaderKey` / `MessageType` are **not** protobuf enums — they travel as the
//! string `key`/`value` of a `Header` (e.g. ping control frame has
//! `headers=[{key:"type", value:"ping"}]`).
//!
//! # Why hand-rolled, not `prost`
//!
//! `prost` would add a `.proto` + `build.rs` + `protoc` (or `protobuf-src`)
//! toolchain dependency to the **whole workspace** build (see project gotcha #2:
//! workspace-root `cargo build` must stay clean). The pbbp2 surface is tiny —
//! four varint scalars, one repeated nested message, four length-delimited
//! optionals — and needs only standard varint + length-delimited wire encoding.
//! Hand-rolling keeps it zero-dependency, self-contained, and trivially
//! auditable against the node-sdk tags above.

// ── Errors ──────────────────────────────────────────────────────────────────

/// Decode failure (truncated buffer, bad wire type, unknown FrameType).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DecodeError {
    /// Read past end of buffer.
    #[error("unexpected end of buffer")]
    Truncated,
    /// Varint longer than 10 bytes (u64 max width).
    #[error("varint overflow (>10 bytes)")]
    VarintOverflow,
    /// Encountered an unsupported wire type (only 0/varint and 2/length-delimited
    /// appear in pbbp2).
    #[error("unsupported wire type {0}")]
    UnsupportedWireType(u32),
    /// `method` field value is not a known `FrameType`.
    #[error("unknown FrameType method value {0}")]
    UnknownFrameType(i32),
}

// ── Enums & known header constants ──────────────────────────────────────────

/// Frame method enum (`FrameType` in node-sdk). Carried by the `method` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FrameType {
    /// Control frame (ping/pong/handshake).
    Control = 0,
    /// Data frame (event/ack with payload).
    Data = 1,
}

impl FrameType {
    #[inline]
    pub fn as_i32(self) -> i32 {
        self as i32
    }
    /// `None` for values outside the known enum (protocol drift surfaced to caller).
    #[inline]
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Control),
            1 => Some(Self::Data),
            _ => None,
        }
    }
}

/// Known `Header.key` string values (`HeaderKey` in node-sdk).
///
/// These are strings on the wire, not protobuf enum ints — exposed as `&str`
/// constants for ergonomic `Header` construction.
pub mod header_key {
    pub const TYPE: &str = "type";
    pub const MESSAGE_ID: &str = "message_id";
    pub const SUM: &str = "sum";
    pub const SEQ: &str = "seq";
    pub const TRACE_ID: &str = "trace_id";
    pub const BIZ_RT: &str = "biz_rt";
    pub const HANDSHAKE_STATUS: &str = "handshake-status";
    pub const HANDSHAKE_MSG: &str = "handshake-msg";
    pub const HANDSHAKE_AUTHERRCODE: &str = "handshake-autherrcode";
}

/// Known `Header.value` strings carried under `key="type"` (`MessageType`).
pub mod message_type {
    pub const EVENT: &str = "event";
    pub const CARD: &str = "card";
    pub const PING: &str = "ping";
    pub const PONG: &str = "pong";
}

// ── Header ──────────────────────────────────────────────────────────────────

/// pbbp2 `Header` message (`{ key: string, value: string }`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Header {
    pub key: String,
    pub value: String,
}

impl Header {
    #[inline]
    pub fn new<K: Into<String>, V: Into<String>>(key: K, value: V) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    /// Encode this header into `out` (without the enclosing length prefix —
    /// the caller writes the field-5 tag + length around the returned bytes).
    fn encode_into(&self, out: &mut Vec<u8>) {
        // field 1 key (string, wire 2): tag 0x0a, len, bytes
        write_string_field(out, 0x0a, self.key.as_bytes());
        // field 2 value (string, wire 2): tag 0x12, len, bytes
        write_string_field(out, 0x12, self.value.as_bytes());
    }
}

// ── Frame ───────────────────────────────────────────────────────────────────

/// pbbp2 `Frame` message. Optional fields are `Option`; they are only emitted
/// on the wire when `Some` (matching node-sdk's conditional encode and keeping
/// round-trip lossless).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Field 1 (uint64). Always encoded.
    pub seq_id: u64,
    /// Field 2 (uint64). Always encoded.
    pub log_id: u64,
    /// Field 3 (int32) — `serviceId` handed back when the connection is established.
    pub service: i32,
    /// Field 4 (int32) — `FrameType` (control=0 / data=1).
    pub frame_type: FrameType,
    /// Field 5 (repeated Header).
    pub headers: Vec<Header>,
    /// Field 6 (string, optional) — e.g. `"json"`.
    pub payload_encoding: Option<String>,
    /// Field 7 (string, optional).
    pub payload_type: Option<String>,
    /// Field 8 (bytes, optional) — event/ack JSON bytes.
    pub payload: Option<Vec<u8>>,
    /// Field 9 (string, optional) — newer string-form log id.
    pub log_id_new: Option<String>,
}

impl Frame {
    /// Build a ping **control** frame: `frame_type=Control`, a single
    /// `{key:"type", value:"ping"}` header, no payload.
    pub fn ping(seq_id: u64, log_id: u64, service: i32) -> Self {
        Self {
            seq_id,
            log_id,
            service,
            frame_type: FrameType::Control,
            headers: vec![Header::new(header_key::TYPE, message_type::PING)],
            payload_encoding: None,
            payload_type: None,
            payload: None,
            log_id_new: None,
        }
    }

    /// Build an event **data** frame with the standard header set
    /// (`type=event`, `message_id`, `sum`, `seq`) and a JSON payload.
    pub fn event(
        seq_id: u64,
        log_id: u64,
        service: i32,
        message_id: &str,
        sum: u64,
        seq: u64,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            seq_id,
            log_id,
            service,
            frame_type: FrameType::Data,
            headers: vec![
                Header::new(header_key::TYPE, message_type::EVENT),
                Header::new(header_key::MESSAGE_ID, message_id),
                Header::new(header_key::SUM, sum.to_string()),
                Header::new(header_key::SEQ, seq.to_string()),
            ],
            payload_encoding: Some("json".to_string()),
            payload_type: None,
            payload: Some(payload),
            log_id_new: None,
        }
    }

    /// Encode this frame to a fresh `Vec<u8>`.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.encode_into(&mut out);
        out
    }

    /// Encode this frame into the provided buffer (append).
    pub fn encode_into(&self, out: &mut Vec<u8>) {
        // Field 1 SeqID (uint64, varint) — tag 0x08. Always emitted.
        write_varint_field(out, 0x08, self.seq_id);
        // Field 2 LogID (uint64, varint) — tag 0x10. Always emitted.
        write_varint_field(out, 0x10, self.log_id);
        // Field 3 service (int32, varint) — tag 0x18. Always emitted.
        // int32 negative → sign-extend to 64 bits (matches protobuf int32 wire).
        write_varint_field(out, 0x18, self.service as i64 as u64);
        // Field 4 method (int32, varint) — tag 0x20. Always emitted.
        write_varint_field(out, 0x20, self.frame_type.as_i32() as i64 as u64);
        // Field 5 headers (repeated Header, length-delimited) — tag 0x2a each.
        for h in &self.headers {
            let mut sub = Vec::new();
            h.encode_into(&mut sub);
            write_string_field(out, 0x2a, &sub);
        }
        // Optional length-delimited fields — emitted only when Some.
        if let Some(s) = &self.payload_encoding {
            write_string_field(out, 0x32, s.as_bytes());
        }
        if let Some(s) = &self.payload_type {
            write_string_field(out, 0x3a, s.as_bytes());
        }
        if let Some(b) = &self.payload {
            write_string_field(out, 0x42, b);
        }
        if let Some(s) = &self.log_id_new {
            write_string_field(out, 0x4a, s.as_bytes());
        }
    }

    /// Decode a frame from `bytes`. Unknown fields are skipped; unknown
    /// `method` values surface as `DecodeError::UnknownFrameType`.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut seq_id = 0u64;
        let mut log_id = 0u64;
        let mut service = 0i32;
        let mut method: Option<i32> = None;
        let mut headers: Vec<Header> = Vec::new();
        let mut payload_encoding: Option<String> = None;
        let mut payload_type: Option<String> = None;
        let mut payload: Option<Vec<u8>> = None;
        let mut log_id_new: Option<String> = None;

        let mut pos = 0usize;
        while pos < bytes.len() {
            let (tag, after_tag) = read_varint(bytes, pos)?;
            pos = after_tag;
            let field = (tag >> 3) as u32;
            let wire = (tag & 7) as u32;

            match (field, wire) {
                // Field 1 SeqID (uint64)
                (1, 0) => {
                    let (v, p) = read_varint(bytes, pos)?;
                    seq_id = v;
                    pos = p;
                }
                // Field 2 LogID (uint64)
                (2, 0) => {
                    let (v, p) = read_varint(bytes, pos)?;
                    log_id = v;
                    pos = p;
                }
                // Field 3 service (int32)
                (3, 0) => {
                    let (v, p) = read_varint(bytes, pos)?;
                    service = v as u32 as i32;
                    pos = p;
                }
                // Field 4 method (int32) → FrameType
                (4, 0) => {
                    let (v, p) = read_varint(bytes, pos)?;
                    method = Some(v as u32 as i32);
                    pos = p;
                }
                // Field 5 headers (repeated Header, length-delimited)
                (5, 2) => {
                    let (h, p) = read_length_delimited(bytes, pos)?;
                    headers.push(decode_header(h)?);
                    pos = p;
                }
                // Field 6 payloadEncoding (string)
                (6, 2) => {
                    let (s, p) = read_length_delimited(bytes, pos)?;
                    payload_encoding = Some(String::from_utf8_lossy(s).into_owned());
                    pos = p;
                }
                // Field 7 payloadType (string)
                (7, 2) => {
                    let (s, p) = read_length_delimited(bytes, pos)?;
                    payload_type = Some(String::from_utf8_lossy(s).into_owned());
                    pos = p;
                }
                // Field 8 payload (bytes)
                (8, 2) => {
                    let (s, p) = read_length_delimited(bytes, pos)?;
                    payload = Some(s.to_vec());
                    pos = p;
                }
                // Field 9 LogIDNew (string)
                (9, 2) => {
                    let (s, p) = read_length_delimited(bytes, pos)?;
                    log_id_new = Some(String::from_utf8_lossy(s).into_owned());
                    pos = p;
                }
                // Unknown field — skip by wire type.
                (_, 0) => {
                    let (_, p) = read_varint(bytes, pos)?;
                    pos = p;
                }
                (_, 2) => {
                    let (_, p) = read_length_delimited(bytes, pos)?;
                    pos = p;
                }
                (_, 1) => return Err(DecodeError::UnsupportedWireType(wire)), // 64-bit fixed
                (_, 5) => return Err(DecodeError::UnsupportedWireType(wire)), // 32-bit fixed
                (_, w) => return Err(DecodeError::UnsupportedWireType(w)),
            }
        }

        let frame_type = FrameType::from_i32(method.unwrap_or(0))
            .ok_or(DecodeError::UnknownFrameType(method.unwrap_or(-1)))?;

        Ok(Self {
            seq_id,
            log_id,
            service,
            frame_type,
            headers,
            payload_encoding,
            payload_type,
            payload,
            log_id_new,
        })
    }
}

/// Decode a `Header` (the body of a field-5 length-delimited entry).
fn decode_header(bytes: &[u8]) -> Result<Header, DecodeError> {
    let mut key = String::new();
    let mut value = String::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let (tag, after_tag) = read_varint(bytes, pos)?;
        pos = after_tag;
        let field = (tag >> 3) as u32;
        let wire = (tag & 7) as u32;
        match (field, wire) {
            (1, 2) => {
                let (s, p) = read_length_delimited(bytes, pos)?;
                key = String::from_utf8_lossy(s).into_owned();
                pos = p;
            }
            (2, 2) => {
                let (s, p) = read_length_delimited(bytes, pos)?;
                value = String::from_utf8_lossy(s).into_owned();
                pos = p;
            }
            (_, 0) => {
                let (_, p) = read_varint(bytes, pos)?;
                pos = p;
            }
            (_, 2) => {
                let (_, p) = read_length_delimited(bytes, pos)?;
                pos = p;
            }
            (_, w) => return Err(DecodeError::UnsupportedWireType(w)),
        }
    }
    Ok(Header { key, value })
}

// ── Low-level protobuf primitives ───────────────────────────────────────────

/// Append a varint to `out`.
#[inline]
fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(((value & 0x7f) as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Write `tag` then a varint `value` (field of wire type 0).
#[inline]
fn write_varint_field(out: &mut Vec<u8>, tag: u32, value: u64) {
    write_varint(out, tag as u64);
    write_varint(out, value);
}

/// Write `tag`, a length varint, then `data` (field of wire type 2).
#[inline]
fn write_string_field(out: &mut Vec<u8>, tag: u32, data: &[u8]) {
    write_varint(out, tag as u64);
    write_varint(out, data.len() as u64);
    out.extend_from_slice(data);
}

/// Read a base-128 varint. Returns `(value, new_pos)`.
fn read_varint(bytes: &[u8], mut pos: usize) -> Result<(u64, usize), DecodeError> {
    let mut result: u64 = 0;
    let mut shift = 0;
    for _ in 0..10 {
        if pos >= bytes.len() {
            return Err(DecodeError::Truncated);
        }
        let b = bytes[pos];
        pos += 1;
        result |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Ok((result, pos));
        }
        shift += 7;
    }
    // Still continuation bit set after 10 bytes → malformed.
    Err(DecodeError::VarintOverflow)
}

/// Read a length-delimited field (length varint + that many bytes).
/// Returns `(slice, new_pos)`.
fn read_length_delimited(bytes: &[u8], pos: usize) -> Result<(&[u8], usize), DecodeError> {
    let (len, after_len) = read_varint(bytes, pos)?;
    let len = len as usize;
    let end = after_len.checked_add(len).ok_or(DecodeError::Truncated)?;
    if end > bytes.len() {
        return Err(DecodeError::Truncated);
    }
    Ok((&bytes[after_len..end], end))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Required case 1: generic round-trip ─────────────────────────────────
    #[test]
    fn frame_round_trip_full() {
        let original = Frame {
            seq_id: 0x1234_5678_9abc,
            log_id: 42,
            service: 7,
            frame_type: FrameType::Data,
            headers: vec![
                Header::new(header_key::TYPE, message_type::EVENT),
                Header::new(header_key::MESSAGE_ID, "om_abcdef123456"),
                Header::new(header_key::TRACE_ID, "trace-00-ff"),
            ],
            payload_encoding: Some("json".to_string()),
            payload_type: Some("event/v1".to_string()),
            payload: Some(br#"{"event_type":"message.receive"}"#.to_vec()),
            log_id_new: Some("log-id-new-xyz".to_string()),
        };

        let bytes = original.encode();
        let decoded = Frame::decode(&bytes).expect("decode should succeed");
        assert_eq!(decoded, original, "round-trip must be lossless");
    }

    // ── Required case 2: ping control frame ─────────────────────────────────
    #[test]
    fn ping_control_frame_bytes_exact() {
        // Hand-computed from the node-sdk field tags (see module docs):
        //   SeqID=1 LogID=1 service=1 method=0(control)
        //   headers=[{key:"type"(=74 79 70 65), value:"ping"(=70 69 6e 67)}]
        let expected_hex = "08011001180120002a0c0a0474797065120470696e67";
        let expected: Vec<u8> = (0..expected_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&expected_hex[i..i + 2], 16).unwrap())
            .collect();

        let frame = Frame::ping(1, 1, 1);
        let encoded = frame.encode();
        assert_eq!(
            encoded, expected,
            "ping frame bytes must match node-sdk tag layout exactly"
        );

        // And it decodes back to the same logical frame.
        let decoded = Frame::decode(&encoded).unwrap();
        assert_eq!(decoded.frame_type, FrameType::Control);
        assert_eq!(decoded.headers.len(), 1);
        assert_eq!(decoded.headers[0], Header::new("type", "ping"));
        assert!(decoded.payload.is_none());
    }

    // ── Required case 3: event data frame ───────────────────────────────────
    #[test]
    fn event_data_frame_round_trip() {
        let payload = br#"{"schema":"2.0","event":{"message":{"chat_id":"oc_x"}}}"#.to_vec();
        let payload_clone = payload.clone();

        let frame = Frame::event(99, 100, 5, "om_d41d8cd98f00b204e980", 1, 1, payload);

        // Verify construction shape.
        assert_eq!(frame.frame_type, FrameType::Data);
        assert_eq!(frame.headers.len(), 4);
        assert_eq!(frame.headers[0], Header::new("type", "event"));
        assert_eq!(frame.headers[1].key, "message_id");
        assert_eq!(frame.headers[1].value, "om_d41d8cd98f00b204e980");
        assert_eq!(frame.headers[2], Header::new("sum", "1"));
        assert_eq!(frame.headers[3], Header::new("seq", "1"));
        assert_eq!(frame.payload.as_deref(), Some(&payload_clone[..]));
        assert_eq!(frame.payload_encoding.as_deref(), Some("json"));

        // Round-trip.
        let bytes = frame.encode();
        let decoded = Frame::decode(&bytes).unwrap();
        assert_eq!(decoded, frame);

        // Payload JSON is preserved byte-for-byte.
        assert_eq!(decoded.payload.unwrap(), payload_clone);
    }

    // ── Wire-format correctness spot checks ─────────────────────────────────

    #[test]
    fn decode_rejects_truncated_varint() {
        // varint with continuation bit but no following byte.
        let bytes = [0x80u8];
        assert_eq!(read_varint(&bytes, 0), Err(DecodeError::Truncated));
    }

    #[test]
    fn decode_rejects_varint_overflow() {
        // 11 bytes all with continuation bit set.
        let bytes = [0x80u8; 11];
        assert_eq!(read_varint(&bytes, 0), Err(DecodeError::VarintOverflow));
    }

    #[test]
    fn decode_skips_unknown_fields() {
        // Build a frame with a synthetic unknown field 20 (wire 2) appended,
        // then a real field. Decoder must skip the unknown and still recover
        // the known data.
        let bytes = Frame::ping(3, 3, 9).encode();
        // Inject an unknown length-delimited field (tag = (20<<3)|2 = 0xa2 0x01).
        let unknown: Vec<u8> = vec![0xa2, 0x01, 0x03, b'x', b'y', b'z'];
        let mut combined = unknown;
        combined.extend_from_slice(&bytes);
        let decoded = Frame::decode(&combined).unwrap();
        assert_eq!(decoded.seq_id, 3);
        assert_eq!(decoded.headers[0], Header::new("type", "ping"));
    }

    #[test]
    fn decode_pong_frame_matches_ping_shape() {
        let mut frame = Frame::ping(7, 8, 2);
        frame.headers[0].value = message_type::PONG.to_string();
        let bytes = frame.encode();
        let decoded = Frame::decode(&bytes).unwrap();
        assert_eq!(decoded, frame);
        assert_eq!(decoded.headers[0].value, "pong");
    }

    #[test]
    fn empty_headers_encode_and_decode() {
        let frame = Frame {
            seq_id: 0,
            log_id: 0,
            service: 1,
            frame_type: FrameType::Control,
            headers: Vec::new(),
            payload_encoding: None,
            payload_type: None,
            payload: None,
            log_id_new: None,
        };
        let bytes = frame.encode();
        let decoded = Frame::decode(&bytes).unwrap();
        assert_eq!(decoded, frame);
        // Fields 1-4 always present: minimum frame is 8 bytes (4 tags + 4 varints).
        assert_eq!(bytes.len(), 8);
    }

    #[test]
    fn large_varint_seq_id_round_trips() {
        let frame = Frame {
            seq_id: u64::MAX,
            log_id: u64::MAX / 2,
            service: 0,
            frame_type: FrameType::Data,
            headers: Vec::new(),
            payload_encoding: None,
            payload_type: None,
            payload: None,
            log_id_new: None,
        };
        let decoded = Frame::decode(&frame.encode()).unwrap();
        assert_eq!(decoded.seq_id, u64::MAX);
        assert_eq!(decoded.log_id, u64::MAX / 2);
    }
}
