//! Shared text-based tool-calling fallback for models without native
//! function calling.
//!
//! The agent layer's `tool_parser` understands several text protocols (JSON
//! array / JSON object / XML / Hermes), but a model only emits one if it has
//! been taught the format. The Ollama backend always injected this teaching
//! into the system message for models reported as `function_calling=false`;
//! the OpenAI-compatible and llama.cpp backends did not — so custom
//! OpenAI-compatible endpoints (whose provider heuristic in `openai.rs`
//! defaults to no function calling) silently lost all tool use: the request
//! carried the `tools` schema, but the model never saw a protocol it could
//! answer in and the reply parsed as plain prose. This module holds the one
//! teaching text and the gating/injection helpers all backends share.

use neomind_core::llm::backend::ToolDefinition;
use neomind_core::message::{Content, ContentPart, Message, MessageRole};

/// Format-teaching instructions appended to the system message for models
/// without native tool support. Only format rules and the protocol shape —
/// tool descriptions are NOT repeated here; the agent system prompt already
/// carries the "Available Tools" section this text points at.
pub(crate) fn format_teaching() -> String {
    let mut result = String::from("## Tool Calling Format (JSON)\n");
    result.push_str("You must call tools using JSON format. Do not just describe what to do.\n\n");
    result.push_str("Format:\n");
    result.push_str("[{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}]\n\n");

    result.push_str("## Important Rules\n");
    result.push_str("1. ALWAYS output tool calls as a JSON array\n");
    result.push_str("2. Don't explain, just call the tool directly\n");
    result.push_str(
        "3. Use the exact tool names and parameters from the Available Tools section above\n",
    );

    result
}

/// Whether a request must carry the format-teaching injection: the effective
/// capability says the model has no native function calling AND tools are
/// actually attached to the request.
pub(crate) fn needed(supports_native_tools: bool, tools: Option<&[ToolDefinition]>) -> bool {
    !supports_native_tools && tools.is_some_and(|t| !t.is_empty())
}

/// Prepare the messages for one LLM request: when [`needed`] says the model
/// can only answer in the text protocol, append [`format_teaching`] to every
/// system message (mirrors the original Ollama injection point — in practice
/// there is exactly one). Otherwise the messages pass through untouched.
pub(crate) fn prepare_messages(
    mut messages: Vec<Message>,
    tools: Option<&[ToolDefinition]>,
    supports_native_tools: bool,
) -> Vec<Message> {
    if needed(supports_native_tools, tools) {
        let teaching = format_teaching();
        append_to_system_messages(&mut messages, &teaching);
    }
    messages
}

/// Append `teaching` to every system message. Text content grows a suffix;
/// multimodal content gains a trailing text part so image parts survive.
fn append_to_system_messages(messages: &mut [Message], teaching: &str) {
    for msg in messages
        .iter_mut()
        .filter(|m| m.role == MessageRole::System)
    {
        match &mut msg.content {
            Content::Text(text) => {
                text.push_str("\n\n");
                text.push_str(teaching);
            }
            Content::Parts(parts) => parts.push(ContentPart::text(teaching)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: "test tool".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }
    }

    #[test]
    fn needed_gates_on_native_support_and_tools() {
        let tools = vec![tool("a")];
        assert!(needed(false, Some(&tools)));
        assert!(
            !needed(true, Some(&tools)),
            "native tool caller must stay untouched"
        );
        assert!(!needed(false, None), "no tools → nothing to teach");
        assert!(
            !needed(false, Some(&[])),
            "empty tool list → nothing to teach"
        );
    }

    #[test]
    fn prepare_appends_to_system_only_and_survives_multimodal() {
        let tools = vec![tool("a")];
        let messages = vec![
            Message::new(MessageRole::System, Content::text("base prompt")),
            Message::new(MessageRole::User, Content::text("hi")),
        ];

        let prepared = prepare_messages(messages, Some(&tools), false);
        assert_eq!(prepared.len(), 2);
        match &prepared[0].content {
            Content::Text(text) => {
                assert!(text.starts_with("base prompt\n\n"));
                assert!(text.contains("Tool Calling Format (JSON)"));
            }
            other => panic!("text content must stay text, got {other:?}"),
        }
        assert!(
            matches!(&prepared[1].content, Content::Text(t) if t == "hi"),
            "user messages must not receive the teaching"
        );

        // Multimodal system message: teaching becomes a trailing text part,
        // image parts are preserved.
        let messages = vec![Message {
            role: MessageRole::System,
            content: Content::Parts(vec![ContentPart::image_url("http://x/img.png")]),
            tool_name: None,
            timestamp: None,
        }];
        let prepared = prepare_messages(messages, Some(&tools), false);
        match &prepared[0].content {
            Content::Parts(parts) => {
                assert_eq!(parts.len(), 2, "image part + teaching part");
                assert!(parts[0].is_image());
                assert!(!parts[1].is_image());
            }
            other => panic!("parts content must stay parts, got {other:?}"),
        }
    }

    #[test]
    fn prepare_passes_through_when_native() {
        let tools = vec![tool("a")];
        let messages = vec![Message::new(MessageRole::System, Content::text("base"))];
        let prepared = prepare_messages(messages, Some(&tools), true);
        assert!(
            matches!(&prepared[0].content, Content::Text(t) if t == "base"),
            "native tool calling must not mutate messages"
        );
    }
}
