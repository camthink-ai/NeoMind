// Tests — split from the former openai.rs monolith.
use super::*;
use neomind_core::llm::backend::{LlmInput, LlmRuntime, ThinkingEffort};
use neomind_core::message::{Content, ContentPart, Message, MessageRole};

use neomind_core::llm::backend::GenerationParams;

#[test]
fn test_cloud_config_openai() {
    let config = CloudConfig::openai("sk-test");
    assert_eq!(config.provider, CloudProvider::OpenAI);
    assert_eq!(config.api_key, "sk-test");
}

#[test]
fn test_cloud_config_with_model() {
    let config = CloudConfig::openai("sk-test").with_model("gpt-4o");
    assert_eq!(config.model, Some("gpt-4o".to_string()));
}

#[test]
fn test_cloud_provider_urls() {
    assert_eq!(
        CloudProvider::OpenAI.base_url(),
        "https://api.openai.com/v1"
    );
    assert_eq!(
        CloudProvider::Anthropic.base_url(),
        "https://api.anthropic.com/v1"
    );
    assert_eq!(
        CloudProvider::Google.base_url(),
        "https://generativelanguage.googleapis.com/v1beta"
    );
    assert_eq!(CloudProvider::Grok.base_url(), "https://api.x.ai/v1");
}

#[test]
fn test_anthropic_base_url_normalization() {
    let cfg = |base: &str| CloudConfig {
        api_key: "k".into(),
        provider: CloudProvider::Anthropic,
        model: None,
        base_url: Some(base.into()),
        timeout_secs: 60,
        max_context: None,
    };
    // Ecosystem convention: base without /v1 → client expands it.
    assert_eq!(
        cfg("https://open.bigmodel.cn/api/anthropic").get_base_url(),
        "https://open.bigmodel.cn/api/anthropic/v1"
    );
    assert_eq!(
        cfg("https://api.anthropic.com").get_base_url(),
        "https://api.anthropic.com/v1"
    );
    // Already-/v1 forms pass through untouched (trailing slash trimmed).
    assert_eq!(
        cfg("https://api.anthropic.com/v1").get_base_url(),
        "https://api.anthropic.com/v1"
    );
    assert_eq!(
        cfg("https://open.bigmodel.cn/api/anthropic/v1/").get_base_url(),
        "https://open.bigmodel.cn/api/anthropic/v1"
    );
    // Other providers are not normalized.
    let mut openai_cfg = cfg("https://api.deepseek.com");
    openai_cfg.provider = CloudProvider::Custom;
    assert_eq!(openai_cfg.get_base_url(), "https://api.deepseek.com");
}

#[test]
fn test_anthropic_response_tolerates_thinking_blocks() {
    // GLM's Anthropic-compatible endpoint emits thinking blocks (as does
    // the official API with extended thinking). The response must parse
    // and extraction must keep only text + tool_use.
    let body = r#"{"content":[
            {"type":"thinking","thinking":"reasoning…","signature":"sig"},
            {"type":"redacted_thinking","data":"opaque"},
            {"type":"text","text":"Hello!"},
            {"type":"some_future_block","foo":1}
        ],"stop_reason":"end_turn","usage":{"input_tokens":13,"output_tokens":4}}"#;
    let resp: AnthropicResponse = serde_json::from_str(body).expect("parses");
    let text: String = resp
        .content
        .iter()
        .filter_map(|b| match b {
            AnthropicContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "Hello!");
}

#[test]
fn test_is_vision_model_openai() {
    // OpenAI vision models
    assert!(is_vision_model(&CloudProvider::OpenAI, "gpt-4o"));
    assert!(is_vision_model(&CloudProvider::OpenAI, "gpt-4o-mini"));
    assert!(is_vision_model(&CloudProvider::OpenAI, "gpt-4-turbo"));
    assert!(is_vision_model(
        &CloudProvider::OpenAI,
        "gpt-4-vision-preview"
    ));
    assert!(is_vision_model(
        &CloudProvider::OpenAI,
        "gpt-4-1106-vision-preview"
    ));
    assert!(is_vision_model(&CloudProvider::OpenAI, "o1"));
    // o1-mini is text-only (no vision) — must NOT be reported as multimodal,
    // otherwise image parts get sent and the API rejects them.
    assert!(!is_vision_model(&CloudProvider::OpenAI, "o1-mini"));
    assert!(!is_vision_model(&CloudProvider::OpenAI, "o1-preview"));

    // OpenAI non-vision models
    assert!(!is_vision_model(&CloudProvider::OpenAI, "gpt-4"));
    assert!(!is_vision_model(&CloudProvider::OpenAI, "gpt-4-32k"));
    assert!(!is_vision_model(&CloudProvider::OpenAI, "gpt-3.5-turbo"));
    assert!(!is_vision_model(&CloudProvider::OpenAI, "gpt-3.5"));
}

#[test]
fn test_is_vision_model_anthropic() {
    // Anthropic vision models (all Claude 3+)
    assert!(is_vision_model(&CloudProvider::Anthropic, "claude-3-opus"));
    assert!(is_vision_model(
        &CloudProvider::Anthropic,
        "claude-3-sonnet"
    ));
    assert!(is_vision_model(&CloudProvider::Anthropic, "claude-3-haiku"));
    assert!(is_vision_model(
        &CloudProvider::Anthropic,
        "claude-3-5-sonnet"
    ));
    assert!(is_vision_model(
        &CloudProvider::Anthropic,
        "claude-3.5-sonnet"
    ));

    // Anthropic non-vision models
    assert!(!is_vision_model(&CloudProvider::Anthropic, "claude-2"));
    assert!(!is_vision_model(
        &CloudProvider::Anthropic,
        "claude-instant"
    ));
}

#[test]
fn test_is_vision_model_google() {
    // Google vision models (all Gemini)
    assert!(is_vision_model(&CloudProvider::Google, "gemini-1.5-flash"));
    assert!(is_vision_model(&CloudProvider::Google, "gemini-1.5-pro"));
    assert!(is_vision_model(&CloudProvider::Google, "gemini-pro-vision"));
    assert!(is_vision_model(&CloudProvider::Google, "gemini-2.0-flash"));

    // Non-gemini models
    assert!(!is_vision_model(&CloudProvider::Google, "palm-2"));
}

#[test]
fn test_is_vision_model_qwen() {
    // Qwen explicit VL models
    assert!(is_vision_model(&CloudProvider::Qwen, "qwen-vl"));
    assert!(is_vision_model(&CloudProvider::Qwen, "qwen2-vl"));
    assert!(is_vision_model(&CloudProvider::Qwen, "qwen3-vl"));
    assert!(is_vision_model(&CloudProvider::Qwen, "qwen-max-vl"));

    // Qwen 3.5/3.6/3.7 native-multimodal series (early fusion, all vision)
    assert!(is_vision_model(&CloudProvider::Qwen, "qwen3.5-turbo"));
    assert!(is_vision_model(&CloudProvider::Qwen, "qwen3.5-plus"));
    assert!(is_vision_model(&CloudProvider::Qwen, "qwen3.5-max"));

    // Text-only commercial tiers — MUST stay text. Reporting these as
    // vision causes the API to reject image parts with
    // `unknown variant image_url, expected text`.
    assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-3.5-plus"));
    assert!(!is_vision_model(&CloudProvider::Qwen, "qwen3-turbo"));
    assert!(!is_vision_model(&CloudProvider::Qwen, "qwen3-plus"));
    assert!(!is_vision_model(&CloudProvider::Qwen, "qwen3-max"));
    assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-3-plus"));
    assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-max"));
    assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-plus"));
    assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-turbo"));

    // Non-vision models (older qwen versions without vision support)
    assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-7b"));
    assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-14b"));
    assert!(!is_vision_model(&CloudProvider::Qwen, "qwen-72b"));
}

/// Regression: a text-only model must never receive `image_url` content
/// parts. The original production bug was DeepSeek (text-only) rejecting a
/// whole request with `unknown variant image_url, expected text` because a
/// earlier conversation turn contained an image and the history was replayed
/// verbatim. `messages_to_api` now strips image parts when
/// `supports_multimodal()` is false.
#[test]
fn test_messages_to_api_strips_images_for_text_model() {
    let runtime = CloudRuntime::new(CloudConfig::deepseek("sk-test").with_model("deepseek-chat"))
        .expect("runtime builds");
    // Sanity: this is a text-only model.
    assert!(
        !runtime.supports_multimodal(),
        "deepseek-chat must be detected as text-only for this test to be meaningful"
    );

    // History entry: a user turn with a text part + an image part (e.g. an
    // image that was attached earlier in the conversation).
    let history_msg = Message::new(
        MessageRole::User,
        Content::Parts(vec![
            ContentPart::Text {
                text: "what is in this picture".to_string(),
            },
            ContentPart::ImageBase64 {
                data: "ZmFrZS1pbWFnZS1kYXRh".to_string(),
                mime_type: "image/png".to_string(),
                detail: None,
            },
        ]),
    );
    // An image-only history turn (text was empty / dropped earlier).
    let image_only_msg = Message::new(
        MessageRole::User,
        Content::Parts(vec![ContentPart::ImageBase64 {
            data: "ZmFrZS1pbWFnZS1kYXRh".to_string(),
            mime_type: "image/png".to_string(),
            detail: None,
        }]),
    );
    // Current turn: plain text follow-up sent to the text-only model.
    let followup = Message::new(
        MessageRole::User,
        Content::Text("summarize our conversation".to_string()),
    );

    let api_msgs = runtime.messages_to_api(&[history_msg, image_only_msg, followup]);

    // Walk every content part of every message and assert no image variant
    // survives serialization for a text-only model.
    let mut saw_image = false;
    let mut saw_placeholder = false;
    let mut saw_history_text = false;
    for msg in &api_msgs {
        if let ApiContent::Parts(parts) = &msg.content {
            for part in parts {
                match part {
                    ApiContentPart::ImageUrl { .. } | ApiContentPart::AnthropicImage { .. } => {
                        saw_image = true
                    }
                    ApiContentPart::Text { text } => {
                        if text.starts_with("[image content omitted") {
                            saw_placeholder = true;
                        }
                        if text == "what is in this picture" {
                            saw_history_text = true;
                        }
                    }
                }
            }
        }
    }
    assert!(
        !saw_image,
        "text-only model must not receive image parts in history replay"
    );
    // The text part of a mixed message is preserved (not dropped with the image).
    assert!(
        saw_history_text,
        "text part of a mixed text+image history turn must survive image stripping"
    );
    // The image-only turn collapses to a placeholder so the message is non-empty.
    assert!(
        saw_placeholder,
        "image-only message should be replaced with a text placeholder"
    );
}

/// Counter-test: a vision-capable model keeps the image parts intact.
#[test]
fn test_messages_to_api_keeps_images_for_vision_model() {
    let runtime = CloudRuntime::new(CloudConfig::openai("sk-test").with_model("gpt-4o"))
        .expect("runtime builds");
    assert!(
        runtime.supports_multimodal(),
        "gpt-4o must be detected as multimodal for this test to be meaningful"
    );

    let msg = Message::new(
        MessageRole::User,
        Content::Parts(vec![
            ContentPart::Text {
                text: "describe this".to_string(),
            },
            ContentPart::ImageBase64 {
                data: "ZmFrZS1pbWFnZS1kYXRh".to_string(),
                mime_type: "image/png".to_string(),
                detail: None,
            },
        ]),
    );

    let api_msgs = runtime.messages_to_api(&[msg]);
    let mut saw_image = false;
    if let ApiContent::Parts(parts) = &api_msgs[0].content {
        for part in parts {
            if matches!(part, ApiContentPart::ImageUrl { .. }) {
                saw_image = true;
            }
        }
    }
    assert!(
        saw_image,
        "vision model must retain image parts in serialized output"
    );
}

// ── enable_thinking wiring for DashScope (Qwen) ──────────────────────
//
// Regression test for the silent-drop bug: `LlmInput.params.thinking_enabled`
// was honored by the Ollama path (ollama.rs:826-844) but completely ignored
// by the cloud OpenAI-compatible path. For qwen3.x-plus backends this meant
// `thinking_enabled: Some(false)` set by analyzer.rs / intent.rs /
// tool_result.rs (per gotcha #7) was silently discarded — the model kept
// thinking on, burning tokens and risking DashScope gateway idle timeouts
// on long reasoning under non-streaming mode.
//
// Fix: `ChatCompletionRequest` gained an `enable_thinking: Option<bool>`
// field, populated ONLY for `CloudProvider::Qwen` (DashScope documents
// this field for qwen3 hybrid thinking models). Other providers don't
// accept it; sending it could break strict validators.

#[test]
fn test_qwen_request_emits_enable_thinking_when_disabled() {
    let runtime = CloudRuntime::new(CloudConfig::qwen("sk-test").with_model("qwen3.7-plus"))
        .expect("runtime builds");

    let input = LlmInput {
        messages: vec![Message::new(MessageRole::User, Content::text("hi"))],
        params: GenerationParams {
            thinking_enabled: Some(false),
            ..Default::default()
        },
        model: None,
        stream: false,
        tools: None,
    };

    let request = runtime.build_chat_request(input, false);
    let json = serde_json::to_value(&request).expect("serialize");
    assert_eq!(
        json["enable_thinking"],
        serde_json::Value::Bool(false),
        "qwen backend must serialize enable_thinking:false when thinking_enabled is Some(false)"
    );
}

#[test]
fn test_qwen_request_omits_enable_thinking_when_default() {
    // When thinking_enabled is None, the field MUST be skipped — letting
    // the model use its default. Hard-coding enable_thinking:false would
    // silently turn off vision reasoning for qwen3.7-plus dashboards.
    let runtime = CloudRuntime::new(CloudConfig::qwen("sk-test").with_model("qwen3.7-plus"))
        .expect("runtime builds");

    let input = LlmInput {
        messages: vec![Message::new(MessageRole::User, Content::text("hi"))],
        params: GenerationParams::default(),
        model: None,
        stream: false,
        tools: None,
    };

    let request = runtime.build_chat_request(input, false);
    let json = serde_json::to_value(&request).expect("serialize");
    assert!(
        json.get("enable_thinking")
            .map(|v| v.is_null())
            .unwrap_or(true),
        "enable_thinking must be absent when thinking_enabled is None"
    );
}

#[test]
fn test_non_qwen_request_never_emits_enable_thinking() {
    // DeepSeek / GLM / OpenAI / etc. don't accept `enable_thinking`.
    // Sending it could break strict validators on custom OpenAI-compatible
    // servers. The field is DashScope-specific.
    let runtime = CloudRuntime::new(CloudConfig::deepseek("sk-test").with_model("deepseek-chat"))
        .expect("runtime builds");

    let input = LlmInput {
        messages: vec![Message::new(MessageRole::User, Content::text("hi"))],
        params: GenerationParams {
            thinking_enabled: Some(false),
            ..Default::default()
        },
        model: None,
        stream: false,
        tools: None,
    };

    let request = runtime.build_chat_request(input, false);
    let json = serde_json::to_value(&request).expect("serialize");
    assert!(
        json.get("enable_thinking")
            .map(|v| v.is_null())
            .unwrap_or(true),
        "non-Qwen providers must not receive enable_thinking field"
    );
}

// ── text tool-calling teaching for non-native providers ─────────────
//
// Regression guard for the Custom-endpoint gap: the request always went
// out with the `tools` schema, but models behind custom OpenAI-compatible
// endpoints default to function_calling=false (provider heuristic in
// `capabilities()`) and were never TAUGHT the JSON protocol the
// agent-layer `tool_parser` understands — every tool-aware turn degraded
// to plain prose while the Ollama backend taught its models. The teaching
// must ride the system message exactly when the Ollama backend would
// inject it: no native calling + tools attached.

fn one_tool() -> neomind_core::llm::backend::ToolDefinition {
    neomind_core::llm::backend::ToolDefinition {
        name: "list_devices".to_string(),
        description: "List registered devices".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    }
}

#[test]
fn test_custom_endpoint_teaches_text_tool_calling() {
    let runtime = CloudRuntime::new(
        CloudConfig::custom("sk-test", "http://localhost:8080/v1").with_model("local-model"),
    )
    .expect("runtime builds");

    let input = LlmInput {
        messages: vec![
            Message::new(MessageRole::System, Content::text("You are helpful.")),
            Message::new(MessageRole::User, Content::text("hi")),
        ],
        params: GenerationParams::default(),
        model: None,
        stream: false,
        tools: Some(vec![one_tool()]),
    };

    let request = runtime.build_chat_request(input, false);
    let json = serde_json::to_value(&request).expect("serialize");
    let sys = json["messages"][0]["content"]
        .as_str()
        .expect("system content serializes as text");
    assert!(
            sys.contains("Tool Calling Format (JSON)"),
            "custom endpoints default to no native function calling — the system message must teach the JSON protocol"
        );
    assert!(
        sys.starts_with("You are helpful.\n\n"),
        "teaching is appended after the original system prompt"
    );
    assert!(
        json["tools"].is_array(),
        "tools schema still rides the request alongside the teaching"
    );
}

#[test]
fn test_native_tool_provider_skips_text_tool_teaching() {
    // OpenAI sits in the native-function-calling heuristic — the model
    // gets the tools schema only, byte-identical to pre-teaching requests.
    let runtime = CloudRuntime::new(CloudConfig::openai("sk-test").with_model("gpt-4o"))
        .expect("runtime builds");

    let input = LlmInput {
        messages: vec![
            Message::new(MessageRole::System, Content::text("You are helpful.")),
            Message::new(MessageRole::User, Content::text("hi")),
        ],
        params: GenerationParams::default(),
        model: None,
        stream: false,
        tools: Some(vec![one_tool()]),
    };

    let request = runtime.build_chat_request(input, false);
    let json = serde_json::to_value(&request).expect("serialize");
    let sys = json["messages"][0]["content"].as_str().expect("text");
    assert!(
        !sys.contains("Tool Calling Format"),
        "native tool-calling providers must not receive the teaching"
    );
}

#[test]
fn test_function_calling_override_skips_text_tool_teaching() {
    // A stored/user override that turns native tools ON for a custom
    // endpoint suppresses the injection — the native protocol wins, and
    // double-teaching would only waste tokens.
    let runtime = CloudRuntime::new(
        CloudConfig::custom("sk-test", "http://localhost:8080/v1").with_model("local-model"),
    )
    .expect("runtime builds")
    .with_capabilities_override(false, false, true, 8192);

    let input = LlmInput {
        messages: vec![Message::new(
            MessageRole::System,
            Content::text("You are helpful."),
        )],
        params: GenerationParams::default(),
        model: None,
        stream: false,
        tools: Some(vec![one_tool()]),
    };

    let request = runtime.build_chat_request(input, false);
    let json = serde_json::to_value(&request).expect("serialize");
    let sys = json["messages"][0]["content"].as_str().expect("text");
    assert!(
        !sys.contains("Tool Calling Format"),
        "an override declaring native tool support must suppress the teaching"
    );
}

// ── protocol-first Cloud AI: vendor endpoints via --type openai ──────
//
// The Cloud AI card (and the CLI `--type openai` + vendor endpoint path)
// creates DashScope/DeepSeek backends typed as plain OpenAI-compatible.
// Regression guard: the vendor-specific param wiring must survive —
// `param_provider()` sniffs the endpoint so enable_thinking /
// thinking-toggle follow where the requests actually go, and
// reasoning_effort is NOT sent to vendors that reject it.

fn llm_input_with_thinking_disabled() -> LlmInput {
    LlmInput {
        messages: vec![Message::new(MessageRole::User, Content::text("hi"))],
        params: GenerationParams {
            thinking_enabled: Some(false),
            ..Default::default()
        },
        model: None,
        stream: false,
        tools: None,
    }
}

#[test]
fn test_openai_typed_dashscope_endpoint_keeps_enable_thinking() {
    // backend_type "openai" + DashScope endpoint — exactly what the
    // Cloud AI dialog creates for Qwen today. Covers both regions: cn
    // (dashscope.aliyuncs.com) and intl (dashscope-intl.aliyuncs.com).
    for host in [
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
    ] {
        let cfg = CloudConfig::openai("sk-test")
            .with_model("qwen3.7-plus")
            .with_base_url_opt(Some(host.into()));
        let runtime = CloudRuntime::new(cfg).expect("runtime builds");

        let request = runtime.build_chat_request(llm_input_with_thinking_disabled(), false);
        let json = serde_json::to_value(&request).expect("serialize");
        assert_eq!(
            json["enable_thinking"],
            serde_json::Value::Bool(false),
            "openai-typed DashScope endpoint ({host}) must still wire enable_thinking"
        );
        assert!(
            json.get("reasoning_effort")
                .map(|v| v.is_null())
                .unwrap_or(true),
            "openai-typed DashScope endpoint ({host}) must NOT receive reasoning_effort"
        );
    }
}

#[test]
fn test_openai_typed_deepseek_endpoint_keeps_thinking_toggle() {
    // DeepSeek defaults thinking ON; without the toggle the disable
    // request is silently dropped.
    let cfg = CloudConfig::openai("sk-test")
        .with_model("deepseek-chat")
        .with_base_url_opt(Some("https://api.deepseek.com/v1".into()));
    let runtime = CloudRuntime::new(cfg).expect("runtime builds");

    let request = runtime.build_chat_request(llm_input_with_thinking_disabled(), false);
    let json = serde_json::to_value(&request).expect("serialize");
    assert_eq!(
        json["thinking"]["type"], "disabled",
        "openai-typed DeepSeek endpoint must still emit the thinking disabled toggle"
    );
}

#[test]
fn test_openai_endpoint_unrelated_to_vendors_stays_openai() {
    // A genuinely generic OpenAI-compatible endpoint (vLLM etc.) must not
    // be sniffed into a vendor — reasoning_effort stays available.
    let cfg = CloudConfig::openai("sk-test")
        .with_model("my-model")
        .with_base_url_opt(Some("http://gpu-host:8000/v1".into()));
    let runtime = CloudRuntime::new(cfg).expect("runtime builds");

    let input = LlmInput {
        params: GenerationParams {
            thinking_effort: Some(ThinkingEffort::Medium),
            ..Default::default()
        },
        ..llm_input_with_thinking_disabled()
    };
    let request = runtime.build_chat_request(input, false);
    let json = serde_json::to_value(&request).expect("serialize");
    assert!(
        json.get("enable_thinking")
            .map(|v| v.is_null())
            .unwrap_or(true),
        "generic endpoint must not receive enable_thinking"
    );
    assert!(
        json.get("thinking").map(|v| v.is_null()).unwrap_or(true),
        "generic endpoint must not receive the DeepSeek thinking toggle"
    );
}

/// SFT contract — OpenAI-compatible path. The `openai_trace.jsonl` hook
/// dumps the ChatCompletionRequest; the system prompt MUST survive as
/// `messages[0]` (role "system"). Without it, student traces (MiniCPM5
/// via llama.cpp's /v1 endpoint) can't be used to diagnose why the model
/// grabs file_write/skill/memory instead of shell. See memory:
/// minicpm5-neomind-baseline.
#[test]
fn openai_request_carries_system_prompt_as_message_for_sft() {
    let runtime = CloudRuntime::new(CloudConfig::openai("sk-test").with_model("gpt-test"))
        .expect("runtime builds");

    let input = LlmInput {
        messages: vec![
            Message::new(MessageRole::System, Content::text("You are NeoMind.")),
            Message::new(MessageRole::User, Content::text("hi")),
        ],
        params: GenerationParams::default(),
        model: None,
        stream: false,
        tools: None,
    };

    let request = runtime.build_chat_request(input, false);
    let json = serde_json::to_value(&request).expect("serialize");
    let msgs = json["messages"].as_array().expect("messages array");
    assert!(!msgs.is_empty(), "messages must not be empty");
    assert_eq!(
        msgs[0]["role"], "system",
        "system prompt must be messages[0]"
    );
    assert_eq!(msgs[0]["content"], "You are NeoMind.");
}

/// SFT contract — Anthropic path (= golden teacher traces). The
/// `anthropic_trace.jsonl` hook dumps the AnthropicRequest; the system
/// prompt MUST survive serialization as a top-level `system` field. This
/// is the entire reason the hook exists — history previously stored zero
/// system messages, so SFT data had no prompt to train against.
/// See memory: minicpm5-neomind-baseline.
#[test]
fn anthropic_request_carries_system_prompt_as_field_for_sft() {
    let runtime = CloudRuntime::new(CloudConfig::anthropic("sk-test").with_model("claude-test"))
        .expect("runtime builds");

    let input = LlmInput {
        messages: vec![
            Message::new(
                MessageRole::System,
                Content::text("You are NeoMind. Use the shell tool."),
            ),
            Message::new(MessageRole::User, Content::text("create a device")),
        ],
        params: GenerationParams::default(),
        model: None,
        stream: false,
        tools: None,
    };

    let (request, _url) = runtime.build_anthropic_request(&input, false);
    let json = serde_json::to_value(&request).expect("serialize");
    assert_eq!(
        json["system"].as_str().unwrap(),
        "You are NeoMind. Use the shell tool.",
        "Anthropic trace must carry the system prompt as a top-level field"
    );
}
