//! Minimal GGUF header parser — validate the magic, extract the model's
//! name / context length / quant / architecture from the metadata K/V block.
//!
//! Used by the local-import path ("bring your own GGUF"): the UI picks a
//! file, we verify it's really a GGUF, and auto-fill the picker fields from
//! the header instead of asking the user. Only the header is read (the
//! metadata block lives at the start of the file); tensors are never
//! touched.

use std::path::Path;

/// Metadata extracted from a GGUF header.
#[derive(Debug, Clone)]
pub struct GgufMeta {
    pub name: Option<String>,
    pub context_length: Option<u64>,
    /// Human quant label derived from `general.file_type` (may be unknown).
    pub quant: Option<String>,
    pub architecture: Option<String>,
}

/// GGUF value types (spec).
const T_UINT32: u32 = 4;
const T_INT32: u32 = 5;
const T_FLOAT32: u32 = 6;
const T_BOOL: u32 = 7;
const T_STRING: u32 = 8;
const T_ARRAY: u32 = 9;
const T_UINT64: u32 = 10;
const T_INT64: u32 = 11;

/// Read a GGUF string (u64 length + bytes) at `cur`, advancing the cursor.
fn read_gguf_string(buf: &[u8], cur: &mut usize) -> Option<String> {
    let len = read_u64(buf, cur)? as usize;
    if *cur + len > buf.len() {
        return None;
    }
    let s = String::from_utf8_lossy(&buf[*cur..*cur + len]).to_string();
    *cur += len;
    Some(s)
}

fn read_u32(buf: &[u8], cur: &mut usize) -> Option<u32> {
    if *cur + 4 > buf.len() {
        return None;
    }
    let v = u32::from_le_bytes(buf[*cur..*cur + 4].try_into().ok()?);
    *cur += 4;
    Some(v)
}

fn read_u64(buf: &[u8], cur: &mut usize) -> Option<u64> {
    if *cur + 8 > buf.len() {
        return None;
    }
    let v = u64::from_le_bytes(buf[*cur..*cur + 8].try_into().ok()?);
    *cur += 8;
    Some(v)
}

/// Skip a value of the given GGUF type, advancing the cursor. Returns the
/// value's data only for the scalar/string types callers care about.
fn read_value(buf: &[u8], cur: &mut usize, ty: u32) -> Option<Option<Vec<u8>>> {
    match ty {
        T_UINT32 | T_INT32 | T_FLOAT32 | T_BOOL => {
            if *cur + 4 > buf.len() {
                return None;
            }
            let v = buf[*cur..*cur + 4].to_vec();
            *cur += 4;
            Some(Some(v))
        }
        T_UINT64 | T_INT64 => {
            if *cur + 8 > buf.len() {
                return None;
            }
            let v = buf[*cur..*cur + 8].to_vec();
            *cur += 8;
            Some(Some(v))
        }
        T_STRING => Some(Some(read_gguf_string(buf, cur)?.into_bytes())),
        T_ARRAY => {
            let elem_ty = read_u32(buf, cur)?;
            let count = read_u64(buf, cur)? as usize;
            for _ in 0..count {
                read_value(buf, cur, elem_ty)?;
            }
            Some(None)
        }
        _ => None,
    }
}

fn u32_of(raw: &[u8]) -> Option<u32> {
    if raw.len() == 4 {
        Some(u32::from_le_bytes(raw.try_into().ok()?))
    } else {
        None
    }
}

fn u64_of(raw: &[u8]) -> Option<u64> {
    if raw.len() == 8 {
        Some(u64::from_le_bytes(raw.try_into().ok()?))
    } else {
        None
    }
}

/// GGUF `general.file_type` → human quant label for the common ones.
fn file_type_quant(code: u32) -> Option<&'static str> {
    Some(match code {
        0 => "f32",
        1 => "f16",
        2 => "q4_0",
        3 => "q4_1",
        6 => "q5_0",
        7 => "q5_1",
        8 => "q8_0",
        10 => "q2_k",
        11 => "q3_k",
        12 => "q4_k",
        13 => "q5_k",
        14 => "q6_k",
        15 => "q8_k",
        16 => "iq1_s",
        17 => "iq2_xxs",
        18 => "iq2_xs",
        19 => "iq3_xxs",
        20 => "iq3_xs",
        21 => "iq4_xs",
        22 => "iq1_m",
        23 => "iq2_s",
        24 => "q4_0_4_4",
        25 => "q4_0_4_8",
        26 => "q4_0_8_8",
        27 => "tq1_0",
        28 => "tq2_0",
        29 => "iq4_nl",
        _ => return None,
    })
}

/// Validate a file is a GGUF and extract header metadata.
///
/// Reads only the first 1 MiB (the metadata block is at the head of the
/// file; tensors follow far beyond). Returns an error for non-GGUF files.
pub fn parse_gguf(path: &Path) -> Result<GgufMeta, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut buf = vec![0u8; 1 << 20];
    let n = f
        .read(&mut buf)
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    let buf = &buf[..n];

    if buf.len() < 24 || &buf[0..4] != b"GGUF" {
        return Err("not a GGUF file (bad magic)".to_string());
    }

    let mut cur = 4usize;
    let _version = read_u32(buf, &mut cur).unwrap_or(0);
    let _tensor_count = read_u64(buf, &mut cur).unwrap_or(0);
    let kv_count = read_u64(buf, &mut cur).unwrap_or(0);

    let mut meta = GgufMeta {
        name: None,
        context_length: None,
        quant: None,
        architecture: None,
    };

    for _ in 0..kv_count {
        let key = match read_gguf_string(buf, &mut cur) {
            Some(k) => k,
            None => break, // truncated header — keep what we have
        };
        let ty = match read_u32(buf, &mut cur) {
            Some(t) => t,
            None => break,
        };
        let value = match read_value(buf, &mut cur, ty) {
            Some(v) => v,
            None => break,
        };
        let value = value.unwrap_or_default();
        match key.as_str() {
            "general.name" => meta.name = Some(String::from_utf8_lossy(&value).to_string()),
            "general.architecture" => {
                meta.architecture = Some(String::from_utf8_lossy(&value).to_string())
            }
            // Context length is arch-specific: "<arch>.context_length".
            "llama.context_length"
            | "qwen2.context_length"
            | "qwen3.context_length"
            | "gemma2.context_length"
            | "gemma3.context_length"
            | "mpt.context_length"
            | "gpt2.context_length"
            | "gptj.context_length"
            | "mamba.context_length"
            | "phi2.context_length"
            | "phi3.context_length"
            | "stablelm.context_length" => {
                meta.context_length = u64_of(&value).or_else(|| u32_of(&value).map(u64::from))
            }
            "general.file_type" => {
                if let Some(code) = u32_of(&value) {
                    meta.quant = file_type_quant(code).map(str::to_string);
                }
            }
            _ => {}
        }
    }

    // If no arch-specific ctx key matched, fall back to the raw arch key.
    if meta.context_length.is_none() {
        if let Some(arch) = &meta.architecture {
            // Already covered by the key list above for known arches.
            let _ = arch;
        }
    }

    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_kv(buf: &mut Vec<u8>, key: &str, ty: u32, value: &[u8]) {
        let klen = (key.len() as u64).to_le_bytes();
        buf.extend_from_slice(&klen);
        buf.extend_from_slice(key.as_bytes());
        buf.extend_from_slice(&ty.to_le_bytes());
        buf.extend_from_slice(value);
    }

    fn string_value(s: &str) -> Vec<u8> {
        let mut v = (s.len() as u64).to_le_bytes().to_vec();
        v.extend_from_slice(s.as_bytes());
        v
    }

    fn make_gguf() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"GGUF");
        buf.extend_from_slice(&3u32.to_le_bytes()); // version
        buf.extend_from_slice(&0u64.to_le_bytes()); // tensor_count
        buf.extend_from_slice(&4u64.to_le_bytes()); // kv_count
        write_kv(
            &mut buf,
            "general.name",
            T_STRING,
            &string_value("My Test Model"),
        );
        write_kv(
            &mut buf,
            "general.architecture",
            T_STRING,
            &string_value("qwen2"),
        );
        write_kv(
            &mut buf,
            "qwen2.context_length",
            T_UINT32,
            &32768u32.to_le_bytes(),
        );
        write_kv(
            &mut buf,
            "general.file_type",
            T_UINT32,
            &12u32.to_le_bytes(),
        ); // q4_k
        buf
    }

    #[test]
    fn rejects_non_gguf() {
        let dir = std::env::temp_dir();
        let p = dir.join("neomind-notgguf.bin");
        std::fs::write(&p, b"not a gguf file at all, just some bytes").unwrap();
        let r = parse_gguf(&p);
        assert!(r.is_err(), "non-GGUF must be rejected");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn parses_metadata() {
        let dir = std::env::temp_dir();
        let p = dir.join("neomind-test.gguf");
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(&make_gguf()).unwrap();
        let m = parse_gguf(&p).expect("parse");
        assert_eq!(m.name.as_deref(), Some("My Test Model"));
        assert_eq!(m.architecture.as_deref(), Some("qwen2"));
        assert_eq!(m.context_length, Some(32768));
        assert_eq!(m.quant.as_deref(), Some("q4_k"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn empty_metadata_tolerated() {
        let dir = std::env::temp_dir();
        let p = dir.join("neomind-empty.gguf");
        let mut buf = Vec::new();
        buf.extend_from_slice(b"GGUF");
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());
        std::fs::write(&p, &buf).unwrap();
        let m = parse_gguf(&p).expect("parse");
        assert!(m.name.is_none());
        assert!(m.quant.is_none());
        let _ = std::fs::remove_file(&p);
    }
}
