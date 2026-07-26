//! Test helpers for agent tests
#![allow(dead_code)]

use std::fs;
use std::path::Path;

/// Create a minimal test image directory structure under a temp data_dir
pub fn setup_test_image_dir(data_dir: &Path) -> anyhow::Result<()> {
    let images_dir = data_dir.join("images");
    fs::create_dir_all(&images_dir)?;

    // Create a test device directory
    let device_dir = images_dir.join("test-device-001");
    fs::create_dir_all(&device_dir)?;

    // Create a test metric directory
    let metric_dir = device_dir.join("image");
    fs::create_dir_all(&metric_dir)?;

    // Create a minimal 1x1 PNG image (red pixel)
    // PNG header: 89 50 4E 47 0D 0A 1A 0A
    // IHDR: 00 00 00 01 00 00 00 01 08 02 00 00 00
    let png_data = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        0x00, 0x00, 0x00, 0x0D, // IHDR length
        0x49, 0x48, 0x44, 0x52, // IHDR
        0x00, 0x00, 0x00, 0x01, // width: 1
        0x00, 0x00, 0x00, 0x01, // height: 1
        0x08, 0x02, 0x00, 0x00, 0x00, // bit depth: 8, color type: 2 (RGB), etc.
        0x90, 0x77, 0x53, 0xDE, // CRC
        0x00, 0x00, 0x00, 0x0C, // IDAT length
        0x49, 0x44, 0x41, 0x54, // IDAT
        0x08, 0x99, 0x01, 0x01, 0x00, 0x00, 0xFF, 0x80, 0x00, 0x03, 0x00,
        0x01, // compressed data
        0x5C, 0x5A, 0x1B, 0x5A, // CRC
        0x00, 0x00, 0x00, 0x00, // IEND length
        0x49, 0x45, 0x4E, 0x44, // IEND
        0xAE, 0x42, 0x60, 0x82, // CRC
    ];

    let test_image_path = metric_dir.join("1234567890000.png");
    fs::write(&test_image_path, png_data)?;

    // Create a JPEG test image (minimal JPEG: 1x1 red pixel)
    // JPEG: FF D8 FF E0 00 10 4A 46 49 46 ...
    let jpeg_data = vec![
        0xFF, 0xD8, // SOI (Start of Image)
        0xFF, 0xE0, 0x00, 0x10, // APP0 marker
        0x4A, 0x46, 0x49, 0x46, 0x00, // "JFIF" identifier
        0x01, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00,
        0x00, // JFIF version and density
        0xFF, 0xDB, 0x00, 0x43, 0x00, // DQT (Define Quantization Table)
        0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08, 0x07, 0x07, 0x07, // quantization table
        0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0F, 0x0C, 0x0D, 0x0D, 0x0E, 0x11, 0x11, 0x10, 0x12,
        0x14, 0x17, 0x16, 0x14, 0x16, 0x18, 0x1D, 0x1D, 0x1F, 0x1F, 0x1F, 0x18, 0x17, 0x18, 0x16,
        0x16, 0x1B, 0x1C, 0x1F, 0x25, 0x26, 0x25, 0x24, 0x26, 0x28, 0x30, 0x2C, 0x2B, 0x2C, 0x2C,
        0x2C, 0xFF, 0xC0, 0x00, 0x0B, // SOF0 (Start of Frame)
        0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0x02, // 1x1 RGB
        0xFF, 0xC4, 0x00, 0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01,
        0x01, // DHT (Define Huffman Table)
        0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05,
        0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5, 0x10, 0x00, 0x02, 0x01, 0x03,
        0x03, 0x02, 0x04, 0x03, 0x05, 0x05, 0x04, 0x04, 0x00, 0x00, 0x01, 0x7D, 0x01, 0x02, 0x03,
        0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x06, 0x13, 0x51, 0x61, 0x07, 0x22, 0x71, 0x14,
        0x32, 0x81, 0x91, 0xA1, 0x08, 0x23, 0x42, 0xB1, 0xC1, 0x15, 0x52, 0xD1, 0xF0, 0x24, 0x33,
        0x62, 0x72, 0x82, 0x09, 0x0A, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x25, 0x26, 0x27, 0x28, 0x29,
        0x2A, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49,
        0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68,
        0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x83, 0x84, 0x85, 0x86, 0x87,
        0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4,
        0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA,
        0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7,
        0xD8, 0xD9, 0xDA, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF1, 0xF2,
        0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF, 0xC4, 0x00, 0x1F, 0x01, 0x00, 0x03,
        0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0xFF, 0xC4, 0x00, 0xB5,
        0x11, 0x00, 0x02, 0x01, 0x02, 0x04, 0x04, 0x03, 0x04, 0x07, 0x05, 0x04, 0x04, 0x00, 0x01,
        0x02, 0x77, 0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51,
        0x07, 0x61, 0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xA1, 0xB1, 0xC1, 0x09,
        0x23, 0x33, 0x52, 0xF0, 0x15, 0x62, 0x72, 0xD1, 0x0A, 0x16, 0x24, 0x34, 0xE1, 0x25, 0xF1,
        0x17, 0x18, 0x19, 0x1A, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A,
        0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59,
        0x5A, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78,
        0x79, 0x7A, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x92, 0x93, 0x94, 0x95,
        0x96, 0x97, 0x98, 0x99, 0x9A, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xB2,
        0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8,
        0xC9, 0xCA, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xE2, 0xE3, 0xE4, 0xE5,
        0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFF,
        0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x3F, 0xFF, 0xD9, // SOS + EOI
    ];

    let test_jpg_path = metric_dir.join("1234567890001.jpg");
    fs::write(&test_jpg_path, jpeg_data)?;

    Ok(())
}

/// Clean up test image directory
pub fn cleanup_test_image_dir(data_dir: &Path) -> anyhow::Result<()> {
    let images_dir = data_dir.join("images");
    if images_dir.exists() {
        fs::remove_dir_all(&images_dir)?;
    }
    Ok(())
}

// ============================================================================
// Mock LlmRuntime — deterministic test double for agent-loop tests.
// Gated behind the `test-utils` feature so it never ships in a release binary.
// ============================================================================
#[cfg(feature = "test-utils")]
pub mod mock_llm {
    use std::collections::VecDeque;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use futures::Stream;

    use neomind_core::llm::backend::{
        BackendId, FinishReason, LlmError, LlmInput, LlmOutput, LlmRuntime, StreamChunk, TokenUsage,
    };

    /// One scripted LLM response. Returned in order, one per `generate` /
    /// `generate_stream` / `generate_to_completion` call.
    #[derive(Debug, Clone)]
    pub struct MockResponse {
        /// Final text content (also what `generate_to_completion` aggregates).
        pub text: String,
        /// Optional reasoning/thinking text.
        pub thinking: Option<String>,
        /// Structured tool calls — populated into `LlmOutput::tool_calls` (the
        /// loop reads these when present). For the streaming path they are ALSO
        /// embedded as JSON in the content stream so the loop's text-fallback
        /// parser sees them when `generate_to_completion` is used.
        pub tool_calls: Vec<serde_json::Value>,
        /// If set, the call returns `LlmError::Network(this)` instead of any
        /// text/tools. Stored as a `String` (not `LlmError`) so `MockResponse`
        /// stays `Clone` — `LlmError` itself is not `Clone`.
        pub error_msg: Option<String>,
    }

    impl MockResponse {
        /// A plain text response with no tool calls (ends the loop naturally).
        pub fn text(s: impl Into<String>) -> Self {
            Self {
                text: s.into(),
                thinking: None,
                tool_calls: vec![],
                error_msg: None,
            }
        }

        /// A response that requests a single tool call.
        pub fn tool_call(name: &str, arguments: serde_json::Value) -> Self {
            Self {
                text: String::new(),
                thinking: None,
                tool_calls: vec![serde_json::json!({ "name": name, "arguments": arguments })],
                error_msg: None,
            }
        }

        /// Attach assistant text to a tool-call response.
        pub fn with_text(mut self, s: impl Into<String>) -> Self {
            self.text = s.into();
            self
        }

        /// Attach thinking/reasoning text.
        pub fn with_thinking(mut self, s: impl Into<String>) -> Self {
            self.thinking = Some(s.into());
            self
        }

        /// An error response — the call returns `LlmError::Network(msg)`.
        pub fn error(msg: impl Into<String>) -> Self {
            Self {
                text: String::new(),
                thinking: None,
                tool_calls: vec![],
                error_msg: Some(msg.into()),
            }
        }
    }

    /// Reusable mock [`LlmRuntime`] for deterministic agent-loop tests.
    ///
    /// Unlike `neomind-core`'s `#[cfg(test)]` `MockStreamRuntime` (crate-local,
    /// returns the same chunks every call), this:
    ///   - is exported under the `test-utils` feature, so it is usable from
    ///     integration tests (which are separate crates);
    ///   - advances the script per call (round 1 → response 1, round 2 →
    ///     response 2, …), so it can drive multi-round ReAct loops;
    ///   - records the number of LLM calls + per-call message counts for
    ///     post-hoc assertions (e.g. "the loop stopped after N rounds").
    ///
    /// State lives behind `Arc<Mutex<…>>` (trait methods take `&self`); the
    /// runtime is cheap to `Clone` (Arc) and safe to share across the executor
    /// and the test thread.
    #[derive(Clone)]
    pub struct MockLlmRuntime {
        state: Arc<Mutex<State>>,
    }

    struct State {
        script: VecDeque<MockResponse>,
        default: MockResponse,
        call_count: usize,
        msg_counts: Vec<usize>,
        max_context: usize,
    }

    impl MockLlmRuntime {
        /// Play `script` in order, then repeat a sentinel
        /// `"[mock: script exhausted]"` text response forever.
        pub fn new(script: Vec<MockResponse>) -> Self {
            Self::with_default(script, MockResponse::text("[mock: script exhausted]"))
        }

        /// Like [`Self::new`] but with a custom response once the script is
        /// exhausted (useful for driving a fixed loop topology).
        pub fn with_default(script: Vec<MockResponse>, default: MockResponse) -> Self {
            Self {
                state: Arc::new(Mutex::new(State {
                    script: script.into_iter().collect(),
                    default,
                    call_count: 0,
                    msg_counts: Vec::new(),
                    max_context: 4096,
                })),
            }
        }

        /// Number of times any generation method was invoked.
        pub fn call_count(&self) -> usize {
            self.state.lock().unwrap().call_count
        }

        /// `messages.len()` recorded for each call, in call order — lets tests
        /// assert that context grew / was compacted across rounds.
        pub fn message_counts(&self) -> Vec<usize> {
            self.state.lock().unwrap().msg_counts.clone()
        }

        /// Pop the next scripted response (or the default), recording the call.
        fn next_response(&self, msg_count: usize) -> MockResponse {
            let mut st = self.state.lock().unwrap();
            st.call_count += 1;
            st.msg_counts.push(msg_count);
            st.script.pop_front().unwrap_or_else(|| st.default.clone())
        }

        fn to_output(resp: MockResponse) -> LlmOutput {
            let has_tools = !resp.tool_calls.is_empty();
            LlmOutput {
                text: resp.text,
                finish_reason: if has_tools {
                    FinishReason::ToolCalls
                } else {
                    FinishReason::Stop
                },
                usage: Some(TokenUsage::new(0, 0)),
                thinking: resp.thinking,
                tool_calls: if has_tools {
                    Some(resp.tool_calls)
                } else {
                    None
                },
            }
        }
    }

    #[async_trait]
    impl LlmRuntime for MockLlmRuntime {
        fn backend_id(&self) -> BackendId {
            BackendId::new("mock")
        }

        fn model_name(&self) -> &str {
            "mock-model"
        }

        async fn generate(&self, input: LlmInput) -> Result<LlmOutput, LlmError> {
            let msg_count = input.messages.len();
            let resp = self.next_response(msg_count);
            if let Some(msg) = resp.error_msg {
                return Err(LlmError::Network(msg));
            }
            Ok(Self::to_output(resp))
        }

        async fn generate_stream(
            &self,
            input: LlmInput,
        ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>, LlmError> {
            let msg_count = input.messages.len();
            let resp = self.next_response(msg_count);
            if let Some(msg) = resp.error_msg {
                return Err(LlmError::Network(msg));
            }
            let mut chunks: Vec<StreamChunk> = Vec::new();
            if let Some(th) = &resp.thinking {
                chunks.push(Ok((th.clone(), true)));
            }
            if !resp.text.is_empty() {
                chunks.push(Ok((resp.text.clone(), false)));
            }
            if !resp.tool_calls.is_empty() {
                // Embed as a JSON-array content chunk so the default
                // `generate_to_completion` (which leaves `tool_calls = None`)
                // still surfaces tool calls via the loop's text-fallback parser.
                let arr = serde_json::Value::Array(resp.tool_calls.clone());
                let json = serde_json::to_string(&arr).unwrap_or_default();
                chunks.push(Ok((json, false)));
            }
            Ok(Box::pin(futures::stream::iter(chunks)))
        }

        fn max_context_length(&self) -> usize {
            self.state.lock().unwrap().max_context
        }
    }
}

#[cfg(all(test, feature = "test-utils"))]
mod mock_llm_tests {
    use super::mock_llm::{MockLlmRuntime, MockResponse};
    use neomind_core::llm::backend::{LlmInput, LlmRuntime};

    #[tokio::test]
    async fn text_responses_play_in_order() {
        let rt = MockLlmRuntime::new(vec![MockResponse::text("a"), MockResponse::text("b")]);
        let o1 = rt.generate(LlmInput::new("hi")).await.unwrap();
        let o2 = rt.generate(LlmInput::new("hi")).await.unwrap();
        assert_eq!(o1.text, "a");
        assert_eq!(o2.text, "b");
        assert_eq!(rt.call_count(), 2);
    }

    #[tokio::test]
    async fn script_exhausted_falls_back_to_default() {
        let rt = MockLlmRuntime::new(vec![MockResponse::text("only")]);
        let o1 = rt.generate(LlmInput::new("hi")).await.unwrap();
        let o2 = rt.generate(LlmInput::new("hi")).await.unwrap();
        assert_eq!(o1.text, "only");
        assert_eq!(o2.text, "[mock: script exhausted]");
    }

    #[tokio::test]
    async fn tool_calls_populated_structurally() {
        let rt = MockLlmRuntime::new(vec![MockResponse::tool_call(
            "shell",
            serde_json::json!({ "command": "neomind device list" }),
        )]);
        let o = rt.generate(LlmInput::new("hi")).await.unwrap();
        let calls = o.tool_calls.expect("tool_calls should be populated");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["name"], "shell");
    }

    #[tokio::test]
    async fn generate_to_completion_advances_per_call() {
        // The loop's thinking-model path uses generate_to_completion (default
        // drains generate_stream). Verify the script still advances there.
        let rt = MockLlmRuntime::new(vec![
            MockResponse::text("round1"),
            MockResponse::text("round2"),
        ]);
        let o1 = rt
            .generate_to_completion(LlmInput::new("hi"))
            .await
            .unwrap();
        let o2 = rt
            .generate_to_completion(LlmInput::new("hi"))
            .await
            .unwrap();
        assert_eq!(o1.text, "round1");
        assert_eq!(o2.text, "round2");
        assert_eq!(rt.call_count(), 2);
    }

    #[tokio::test]
    async fn error_response_propagates() {
        let rt = MockLlmRuntime::new(vec![MockResponse::error("boom")]);
        let r = rt.generate(LlmInput::new("hi")).await;
        assert!(r.is_err());
        assert_eq!(rt.call_count(), 1);
    }

    #[tokio::test]
    async fn message_counts_recorded() {
        let rt = MockLlmRuntime::new(vec![MockResponse::text("a"), MockResponse::text("b")]);
        let _ = rt.generate(LlmInput::new("hi")).await;
        let _ = rt.generate(LlmInput::new("hi")).await;
        assert_eq!(rt.message_counts().len(), rt.call_count());
    }
}
