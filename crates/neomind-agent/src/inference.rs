//! Single-shot constrained inference — the L0 kernel entry.
//!
//! One call in, schema-validated fields out: system contract (the output
//! schema) + instruction + rendered context → JSON object parsed against
//! [`OperatorField`]s, with exactly one repair retry that feeds the
//! validation errors back to the model. No tools, no loop, no memory.
//!
//! Serves Structured-mode agents (L0 operators) and will serve chat tools
//! (`query_conclusion` refresh, dry-run). Runtime construction stays in the
//! instance manager (M0-2) — this module only resolves and calls.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use neomind_core::llm::backend::{GenerationParams, LlmError, LlmInput, LlmRuntime};
use neomind_core::message::{Content, ContentPart, Message, MessageRole};
use neomind_storage::{OperatorField, OperatorFieldType};

/// What the inference produced.
#[derive(Debug, Clone, PartialEq)]
pub struct InferenceOutcome {
    /// Schema-validated values keyed by field name. Numbers stay numbers,
    /// booleans stay booleans, enums stay strings.
    pub fields: BTreeMap<String, serde_json::Value>,
    /// The raw model text of the successful (final) attempt — evidence for
    /// the execution record.
    pub raw_text: String,
    /// 1 = first attempt validated; 2 = needed the repair retry.
    pub attempts: u8,
    /// The model's own answer to "how sure are you", 0–1. `None` when it did
    /// not give one — an absent claim rather than an invented default, because
    /// 002 §4.2 gates real behaviour on this number.
    pub confidence: Option<f32>,
}

/// Key the model is asked to answer with, alongside the schema fields.
///
/// Deliberately NOT a schema field: this is the inference's self-report, and
/// making it part of the contract the user authored would publish it as one of
/// their outputs.
pub const CONFIDENCE_KEY: &str = "confidence";

/// Everything a single inference call needs.
#[derive(Debug, Clone)]
pub struct InferenceRequest {
    /// The user's natural-language instruction (what to extract/judge).
    pub instruction: String,
    /// Rendered input data (the payload block appended to the instruction).
    pub context: String,
    /// Output contract. Empty = caller bug; rejected with [`InferenceError::NoSchema`].
    pub schema: Vec<OperatorField>,
    /// Specific backend id; None = the active backend.
    pub backend_id: Option<String>,
    /// Per-call wall-clock timeout in seconds (default via [`InferenceRequest::default`]).
    pub timeout_secs: u32,
    /// Output token cap. Numbers and enums fit in a few hundred tokens, but a
    /// text field (an image description) does not: capped at 512 the JSON
    /// never closes and every run fails validation. 2048 leaves room for a
    /// verbose description per field without inviting essays.
    pub max_output_tokens: usize,
    /// Images the model must actually SEE (S1: camera → fields). Rendered as
    /// multimodal parts, never as text — base64 in the prompt is truncated
    /// garbage the model rightly refuses to interpret. `(mime, base64)`.
    pub images: Vec<(String, String)>,
}

impl Default for InferenceRequest {
    fn default() -> Self {
        Self {
            instruction: String::new(),
            context: String::new(),
            schema: Vec::new(),
            backend_id: None,
            timeout_secs: 60,
            max_output_tokens: 2048,
            images: Vec::new(),
        }
    }
}

/// Why an inference call failed.
#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("no LLM backend available: {0}")]
    NoBackend(String),
    #[error("LLM call failed: {0}")]
    Transport(String),
    #[error("inference timed out after {0}s")]
    Timeout(u32),
    #[error("model output was not a JSON object: {0}")]
    NotJson(String),
    #[error("schema validation failed after retry: {0:?}")]
    Validation(Vec<String>),
    #[error("no output schema given")]
    NoSchema,
}

/// Thin client — stateless; the runtime comes from the instance manager.
#[derive(Debug, Default, Clone, Copy)]
pub struct InferenceClient;

impl InferenceClient {
    pub fn new() -> Self {
        Self
    }

    /// Resolve the runtime via the instance manager and run the request.
    pub async fn run(&self, req: &InferenceRequest) -> Result<InferenceOutcome, InferenceError> {
        if req.schema.is_empty() {
            return Err(InferenceError::NoSchema);
        }
        let manager = crate::llm_backends::get_instance_manager()
            .map_err(|e| InferenceError::NoBackend(e.to_string()))?;
        let runtime = match &req.backend_id {
            Some(id) => manager
                .get_runtime(id)
                .await
                .map_err(|e| InferenceError::NoBackend(e.to_string()))?,
            None => manager
                .get_active_runtime()
                .await
                .map_err(|e| InferenceError::NoBackend(e.to_string()))?,
        };
        self.run_with_runtime(&runtime, req).await
    }

    /// Core path with an injected runtime — the seam tests use.
    pub async fn run_with_runtime(
        &self,
        runtime: &Arc<dyn LlmRuntime>,
        req: &InferenceRequest,
    ) -> Result<InferenceOutcome, InferenceError> {
        if req.schema.is_empty() {
            return Err(InferenceError::NoSchema);
        }

        // With images the user message is multimodal: the text payload plus
        // one image part each. The payload text references them ("[image
        // attached]"); the pixels ride in the parts, where backends that
        // support vision will actually decode them.
        let user_message = if req.images.is_empty() {
            Message::user(Self::payload_message(req, None))
        } else {
            let mut parts = vec![ContentPart::text(Self::payload_message(req, None))];
            for (mime, data) in &req.images {
                parts.push(ContentPart::image_base64(data.clone(), mime.clone()));
            }
            Message::new(MessageRole::User, Content::Parts(parts))
        };
        let mut messages = vec![
            Message::system(Self::contract_message(&req.schema)),
            user_message,
        ];

        for attempt in 1u8..=2 {
            let input = LlmInput {
                messages: messages.clone(),
                params: Self::params(req),
                model: None,
                stream: false,
                tools: None,
            };

            let output = tokio::time::timeout(
                Duration::from_secs(req.timeout_secs.max(1) as u64),
                runtime.generate(input),
            )
            .await
            .map_err(|_| InferenceError::Timeout(req.timeout_secs))?
            .map_err(|e: LlmError| InferenceError::Transport(e.to_string()))?;

            let raw = output.text.trim().to_string();
            let parsed = extract_json_object(&raw)
                .ok_or_else(|| InferenceError::NotJson(truncate(&raw, 120)))?;

            let object = match parsed {
                serde_json::Value::Object(map) => map,
                _ => return Err(InferenceError::NotJson(truncate(&raw, 120))),
            };

            let mut fields = BTreeMap::new();
            let mut errors = Vec::new();
            for field in &req.schema {
                match object.get(&field.name) {
                    None => errors.push(format!("missing field \"{}\"", field.name)),
                    Some(v) => match coerce(v, &field.field_type) {
                        Ok(coerced) => {
                            fields.insert(field.name.clone(), coerced);
                        }
                        Err(why) => errors.push(format!("field \"{}\": {}", field.name, why)),
                    },
                }
            }

            if errors.is_empty() {
                return Ok(InferenceOutcome {
                    fields,
                    confidence: object.get(CONFIDENCE_KEY).and_then(read_confidence),
                    raw_text: raw,
                    attempts: attempt,
                });
            }

            if attempt == 1 {
                // Repair retry: show the model exactly what was wrong.
                messages = vec![
                    Message::system(Self::contract_message(&req.schema)),
                    Message::user(Self::payload_message(req, None)),
                    Message::assistant(raw.clone()),
                    Message::user(format!(
                        "Your output had schema errors:\n{}\nRespond again with ONE corrected \
                         JSON object only.",
                        errors
                            .iter()
                            .map(|e| format!("- {}", e))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )),
                ];
            } else {
                return Err(InferenceError::Validation(errors));
            }
        }
        unreachable!("loop runs at most twice")
    }

    fn params(req: &InferenceRequest) -> GenerationParams {
        GenerationParams {
            // Deterministic extraction, not creative writing.
            temperature: Some(0.1),
            // Thinking models must not burn tokens on chain-of-thought for a
            // single structured call (CLAUDE.md gotcha #7).
            thinking_enabled: Some(false),
            max_tokens: Some(req.max_output_tokens),
            ..GenerationParams::default()
        }
    }

    fn contract_message(schema: &[OperatorField]) -> String {
        let mut lines = String::from(
            "You are a data-extraction function. Respond with exactly ONE JSON object and no \
             other text (no markdown fence, no explanation). The object has these keys:\n",
        );
        for f in schema {
            let mut line = format!("- \"{}\": {}", f.name, type_words(&f.field_type));
            if let Some(d) = &f.description {
                line.push_str(&format!(" — {}", d));
            }
            if let Some(u) = &f.unit {
                line.push_str(&format!(" (unit: {})", u));
            }
            lines.push_str(&line);
            lines.push('\n');
        }
        lines.push_str(&format!(
            "- \"{CONFIDENCE_KEY}\": number from 0 to 1 — how sure you are that the values \
             above are right, given what you were shown. Say a LOW number when the input is \
             missing, unclear or ambiguous, or when you had to guess; do not default to a \
             high number.\n"
        ));
        lines
    }

    fn payload_message(req: &InferenceRequest, _retry: Option<()>) -> String {
        let mut m = format!("Instruction: {}\n", req.instruction);
        if !req.context.is_empty() {
            m.push_str("\nInput data:\n");
            m.push_str(&req.context);
        }
        m.push_str("\nJSON object:");
        m
    }
}

/// Read the model's confidence, best-effort.
///
/// Not a validation error when missing: a run should never fail over a
/// secondary self-report, and the existing happy-path tests assert exactly that
/// a response without one still validates. Out-of-range values are clamped
/// rather than discarded — a model that said 1.7 meant "very sure".
fn read_confidence(v: &serde_json::Value) -> Option<f32> {
    let n = match v {
        serde_json::Value::Number(n) => n.as_f64(),
        // Models wrap numbers in strings often enough that the field coercer
        // already tolerates it; do the same here.
        serde_json::Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }?;
    if !n.is_finite() {
        return None;
    }
    Some(n.clamp(0.0, 1.0) as f32)
}

/// Human words for a field type, used in the contract message.
fn type_words(t: &OperatorFieldType) -> String {
    match t {
        OperatorFieldType::Number => "number".to_string(),
        OperatorFieldType::Text => "string".to_string(),
        OperatorFieldType::Boolean => "boolean (true/false)".to_string(),
        // The allowed strings must be IN the contract. Pointing at "the
        // description" made the model guess — a Chinese-enum field with no
        // description got answered "red" in English, failing validation.
        OperatorFieldType::Enum(values) => {
            if values.is_empty() {
                "string".to_string()
            } else {
                let quoted: Vec<String> = values.iter().map(|v| format!("\"{}\"", v)).collect();
                format!("one of exactly {}", quoted.join(", "))
            }
        }
    }
}

/// Coerce a JSON value into the field's declared type. Lenient where models
/// are known to be sloppy (numbers quoted as strings), strict where the value
/// feeds automation (enum membership).
fn coerce(v: &serde_json::Value, t: &OperatorFieldType) -> Result<serde_json::Value, String> {
    match t {
        OperatorFieldType::Number => match v {
            serde_json::Value::Number(n) => Ok(serde_json::Value::Number(n.clone())),
            serde_json::Value::String(s) => s
                .trim()
                .parse::<f64>()
                .map(|f| {
                    serde_json::Number::from_f64(f)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null)
                })
                .map_err(|_| format!("expected number, got string {:?}", truncate(s, 40))),
            other => Err(format!("expected number, got {}", json_kind(other))),
        },
        OperatorFieldType::Text => match v {
            serde_json::Value::String(s) => Ok(serde_json::Value::String(s.clone())),
            serde_json::Value::Number(n) => Ok(serde_json::Value::String(n.to_string())),
            other => Err(format!("expected string, got {}", json_kind(other))),
        },
        OperatorFieldType::Boolean => match v {
            serde_json::Value::Bool(b) => Ok(serde_json::Value::Bool(*b)),
            serde_json::Value::String(s) if s.eq_ignore_ascii_case("true") => {
                Ok(serde_json::Value::Bool(true))
            }
            serde_json::Value::String(s) if s.eq_ignore_ascii_case("false") => {
                Ok(serde_json::Value::Bool(false))
            }
            other => Err(format!("expected boolean, got {}", json_kind(other))),
        },
        OperatorFieldType::Enum(allowed) => match v {
            serde_json::Value::String(s) => {
                let hit = allowed
                    .iter()
                    .find(|a| a.trim().eq_ignore_ascii_case(s.trim()));
                match hit {
                    Some(a) => Ok(serde_json::Value::String(a.trim().to_string())),
                    None => Err(format!(
                        "value {:?} not in allowed set {:?}",
                        truncate(s, 40),
                        allowed
                    )),
                }
            }
            other => Err(format!(
                "expected one of {:?}, got {}",
                allowed,
                json_kind(other)
            )),
        },
    }
}

fn json_kind(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Extract the first balanced JSON object from raw model text, tolerating
/// markdown fences and leading prose ("Here is the JSON: {...}").
fn extract_json_object(raw: &str) -> Option<serde_json::Value> {
    let bytes = raw.as_bytes();
    let Some(start) = bytes.iter().position(|&b| b == b'{') else {
        // Models sometimes emit the object WITHOUT its braces — a bare
        // `"key": value` pair (observed live: `"dominant_color": "red"`).
        // If it parses once wrapped, that is what was meant.
        let trimmed = raw.trim();
        if trimmed.contains("\":") {
            return serde_json::from_str(&format!("{{ {} }}", trimmed)).ok();
        }
        return None;
    };
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let candidate = &raw[start..=i];
                    return serde_json::from_str(candidate).ok();
                }
            }
            _ => {}
        }
    }
    None
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{}…", t)
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod tests {
    use super::*;
    use crate::testing_helpers::mock_llm::{MockLlmRuntime, MockResponse};
    use std::sync::Arc;

    fn schema() -> Vec<OperatorField> {
        vec![
            OperatorField {
                name: "missing_count".into(),
                field_type: OperatorFieldType::Number,
                unit: Some("件".into()),
                description: Some("number of missing items".into()),
            },
            OperatorField {
                name: "batch_status".into(),
                field_type: OperatorFieldType::Enum(vec![
                    "正常".into(),
                    "待检".into(),
                    "停线".into(),
                ]),
                unit: None,
                description: None,
            },
        ]
    }

    fn req() -> InferenceRequest {
        InferenceRequest {
            instruction: "检查包装台漏装并给出批次状态".into(),
            context: "cam-01 最新画面描述： conveyor 上有 1 件空位".into(),
            schema: schema(),
            ..InferenceRequest::default()
        }
    }

    fn rt(script: Vec<MockResponse>) -> Arc<dyn LlmRuntime> {
        Arc::new(MockLlmRuntime::new(script))
    }

    #[tokio::test]
    async fn happy_path_first_attempt() {
        let runtime = rt(vec![MockResponse::text(
            r#"{"missing_count": 1, "batch_status": "待检"}"#,
        )]);
        let out = InferenceClient::new()
            .run_with_runtime(&runtime, &req())
            .await
            .unwrap();
        assert_eq!(out.attempts, 1);
        assert_eq!(out.fields["missing_count"], serde_json::json!(1));
        assert_eq!(out.fields["batch_status"], serde_json::json!("待检"));
    }

    /// Design 002 §4.2 gates behaviour on confidence ("low confidence is not
    /// taken at face value"), which makes a fabricated number worse than none:
    /// a constant makes every threshold compare against the same value forever.
    /// So the model is asked for its own — as an inference-level key, NOT a
    /// schema field, which would put it in the user's output contract and
    /// publish it as data they never asked for.
    #[tokio::test]
    async fn the_model_is_asked_for_its_confidence_and_that_number_is_kept() {
        let runtime = rt(vec![MockResponse::text(
            r#"{"missing_count": 1, "batch_status": "待检", "confidence": 0.35}"#,
        )]);
        let out = InferenceClient::new()
            .run_with_runtime(&runtime, &req())
            .await
            .unwrap();

        assert_eq!(out.confidence, Some(0.35));
        assert_eq!(out.fields["missing_count"], serde_json::json!(1));
        assert!(
            !out.fields.contains_key("confidence"),
            "confidence is the inference's, not the user's contract: {:?}",
            out.fields.keys().collect::<Vec<_>>()
        );
    }

    /// A response that omits it yields no claim rather than a default. The
    /// happy-path test above already covers absence surviving validation; this
    /// pins what the value is.
    #[tokio::test]
    async fn an_absent_confidence_is_absent_rather_than_invented() {
        let runtime = rt(vec![MockResponse::text(
            r#"{"missing_count": 1, "batch_status": "待检"}"#,
        )]);
        let out = InferenceClient::new()
            .run_with_runtime(&runtime, &req())
            .await
            .unwrap();

        assert_eq!(out.confidence, None);
    }

    /// Models put the decimal point in the wrong place. Clamping keeps the
    /// intent — 1.7 means "very sure" — where rejecting it would discard an
    /// answer the model did give.
    #[tokio::test]
    async fn an_out_of_range_confidence_is_clamped_not_discarded() {
        for (raw, expected) in [("1.7", 1.0f32), ("-0.2", 0.0)] {
            let runtime = rt(vec![MockResponse::text(format!(
                r#"{{"missing_count": 1, "batch_status": "待检", "confidence": {raw}}}"#
            ))]);
            let out = InferenceClient::new()
                .run_with_runtime(&runtime, &req())
                .await
                .unwrap();

            assert_eq!(out.confidence, Some(expected), "raw confidence {raw}");
        }
    }

    /// The number is only real if it was actually asked for.
    #[test]
    fn the_contract_asks_the_model_for_a_confidence() {
        let message = InferenceClient::contract_message(&schema());

        assert!(message.contains("confidence"), "contract: {message}");
        assert!(
            message.to_lowercase().contains("sure"),
            "the model has to be told what the number means: {message}"
        );
    }

    #[tokio::test]
    async fn fenced_json_and_numeric_string_are_coerced() {
        let runtime = rt(vec![MockResponse::text(
            "```json\n{\"missing_count\": \"3\", \"batch_status\": \"正常\"}\n```",
        )]);
        let out = InferenceClient::new()
            .run_with_runtime(&runtime, &req())
            .await
            .unwrap();
        assert_eq!(out.fields["missing_count"], serde_json::json!(3.0));
    }

    #[tokio::test]
    async fn invalid_enum_repaired_on_retry() {
        let runtime = rt(vec![
            MockResponse::text(r#"{"missing_count": 2, "batch_status": "bad"}"#),
            MockResponse::text(r#"{"missing_count": 2, "batch_status": "停线"}"#),
        ]);
        let out = InferenceClient::new()
            .run_with_runtime(&runtime, &req())
            .await
            .unwrap();
        assert_eq!(out.attempts, 2);
        assert_eq!(out.fields["batch_status"], serde_json::json!("停线"));
    }

    #[tokio::test]
    async fn persistent_errors_fail_with_validation_list() {
        let runtime = rt(vec![
            MockResponse::text(r#"{"missing_count": "x", "batch_status": "nope"}"#),
            MockResponse::text(r#"{"missing_count": true}"#),
        ]);
        let err = InferenceClient::new()
            .run_with_runtime(&runtime, &req())
            .await
            .unwrap_err();
        match err {
            InferenceError::Validation(errs) => {
                assert!(errs.iter().any(|e| e.contains("missing_count")));
                assert!(errs.iter().any(|e| e.contains("batch_status")));
            }
            other => panic!("expected Validation, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn transport_error_surfaces() {
        let runtime = rt(vec![MockResponse::error("backend down")]);
        let err = InferenceClient::new()
            .run_with_runtime(&runtime, &req())
            .await
            .unwrap_err();
        assert!(matches!(err, InferenceError::Transport(_)));
    }

    #[tokio::test]
    async fn prose_wrapped_json_is_extracted() {
        let runtime = rt(vec![MockResponse::text(
            "好的，结果如下：{\"missing_count\": 0, \"batch_status\": \"正常\"} 以上。",
        )]);
        let out = InferenceClient::new()
            .run_with_runtime(&runtime, &req())
            .await
            .unwrap();
        assert_eq!(out.fields["missing_count"], serde_json::json!(0));
    }

    #[test]
    fn extract_json_handles_nested_and_strings() {
        let v = extract_json_object(r#"prefix {"a": {"b": "c}"}, "n": 1} suffix"#).unwrap();
        assert_eq!(v["n"], serde_json::json!(1));
        assert_eq!(v["a"]["b"], serde_json::json!("c}"));
        assert!(extract_json_object("no json here").is_none());
    }
}
