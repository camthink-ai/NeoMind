//! Explicit text-protocol adapter for requests without native function calling.
//! Only a complete, dedicated execution envelope is decoded. Plain JSON,
//! prose and fenced examples remain ordinary output.

use neomind_core::llm::backend::ToolDefinition;
use neomind_core::message::{Content, ContentPart, Message, MessageRole};

/// Format-teaching instructions appended to the system message for models
/// without native tool support. Only format rules and the protocol shape —
/// tool descriptions are NOT repeated here; the agent system prompt already
/// carries the "Available Tools" section this text points at.
pub(crate) fn format_teaching() -> String {
    "## Tool protocol\nWhen an actual tool operation is needed, respond with exactly \
     <neomind_tool_calls>[{\"name\":\"tool_name\",\"arguments\":{\"param\":\"value\"}}]</neomind_tool_calls>. \
     Use only available tool names and documented parameters. This envelope is an execution request; \
     do not put it in explanations or examples. For ordinary answers, plans, or explanations, \
     respond normally without this envelope. Follow the user's requested scope."
        .to_string()
}

/// Only non-native requests that actually carry tools use the explicit text envelope.
pub(crate) fn legacy_tools(
    native: bool,
    tools: Option<&[ToolDefinition]>,
) -> Option<Vec<ToolDefinition>> {
    needed(native, tools).then(|| tools.unwrap_or_default().to_vec())
}

fn decode(text: &str, tools: &[ToolDefinition]) -> Option<Vec<serde_json::Value>> {
    let json = text
        .trim()
        .strip_prefix("<neomind_tool_calls>")?
        .strip_suffix("</neomind_tool_calls>")?;
    let calls: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    if calls.is_empty()
        || !calls.iter().all(|call| {
            call.get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|name| tools.iter().any(|tool| tool.name == name))
                && call
                    .get("arguments")
                    .is_some_and(serde_json::Value::is_object)
        })
    {
        return None;
    }
    Some(calls)
}

pub(crate) fn adapt_output(
    mut output: neomind_core::llm::backend::LlmOutput,
    tools: Option<Vec<ToolDefinition>>,
) -> neomind_core::llm::backend::LlmOutput {
    if output.tool_calls.as_ref().is_none_or(Vec::is_empty) {
        if let Some(calls) = tools.as_ref().and_then(|tools| decode(&output.text, tools)) {
            output.text.clear();
            output.tool_calls = Some(calls);
            output.finish_reason = neomind_core::llm::backend::FinishReason::ToolCalls;
        }
    }
    output
}

/// Text-only protocols buffer content until the complete envelope is known.
/// Thinking and native calls retain their channel. Native-capable requests pass through.
pub(crate) fn adapt_stream(
    mut stream: std::pin::Pin<
        Box<dyn futures::Stream<Item = neomind_core::llm::backend::StreamChunk> + Send>,
    >,
    tools: Option<Vec<ToolDefinition>>,
) -> std::pin::Pin<Box<dyn futures::Stream<Item = neomind_core::llm::backend::StreamChunk> + Send>>
{
    let Some(tools) = tools else {
        return stream;
    };
    Box::pin(async_stream::stream! {
        use futures::StreamExt;
        use neomind_core::llm::backend::StreamEvent;
        let mut text = String::new();
        let mut native_seen = false;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(StreamEvent::Text(part)) => {
                    // Usage markers are transport metadata, not part of a text envelope.
                    let (content, prompt_tokens) = neomind_core::llm::backend::strip_token_marker(&part);
                    text.push_str(&content);
                    if let Some(tokens) = prompt_tokens {
                        yield Ok(StreamEvent::Text(format!("\n__NEOMIND_TOKEN_PROMPT:{tokens}__")));
                    }
                }
                Ok(StreamEvent::ToolCalls(calls)) => {
                    native_seen = true;
                    yield Ok(StreamEvent::ToolCalls(calls));
                }
                Err(error) => {
                    if !text.is_empty() { yield Ok(StreamEvent::Text(std::mem::take(&mut text))); }
                    yield Err(error);
                    return;
                }
                thinking => yield thinking,
            }
        }
        if let Some(calls) = (!native_seen).then(|| decode(&text, &tools)).flatten() {
            yield Ok(StreamEvent::ToolCalls(calls));
        } else if !text.is_empty() {
            yield Ok(StreamEvent::Text(text));
        }
    })
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

    #[tokio::test]
    async fn explicit_envelope_survives_chunk_boundaries_but_examples_stay_text() {
        use futures::StreamExt;
        use neomind_core::llm::backend::StreamEvent;
        let wrapped = "<neomind_tool_calls>[{\"name\":\"echo\",\"arguments\":{\"msg\":\"hello\"}}]</neomind_tool_calls>";
        let tools = vec![tool("echo")];
        for text in [
            "```json\n[{\"name\":\"echo\",\"arguments\":{}}]\n```".to_string(),
            "[]".to_string(),
            format!("Example: {wrapped}"),
            format!("```\n{wrapped}\n```"),
            wrapped.replace("echo", "unknown"),
        ] {
            let stream = futures::stream::iter(vec![Ok(StreamEvent::Text(text.clone()))]);
            let events: Vec<_> = adapt_stream(Box::pin(stream), Some(tools.clone()))
                .collect()
                .await;
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].as_ref().unwrap(), &StreamEvent::Text(text));
        }
        let chunks = wrapped
            .chars()
            .map(|c| Ok(StreamEvent::Text(c.to_string())))
            .collect::<Vec<_>>();
        let events: Vec<_> = adapt_stream(Box::pin(futures::stream::iter(chunks)), Some(tools))
            .collect()
            .await;
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], Ok(StreamEvent::ToolCalls(calls)) if calls[0]["name"] == "echo")
        );
    }

    #[tokio::test]
    async fn usage_metadata_does_not_prevent_a_text_protocol_call() {
        use futures::StreamExt;
        use neomind_core::llm::backend::StreamEvent;
        let envelope =
            "<neomind_tool_calls>[{\"name\":\"echo\",\"arguments\":{}}]</neomind_tool_calls>";
        let stream = futures::stream::iter(vec![
            Ok(StreamEvent::Text(envelope.into())),
            Ok(StreamEvent::Text("\n__NEOMIND_TOKEN_PROMPT:123__".into())),
        ]);
        let events: Vec<_> = adapt_stream(Box::pin(stream), Some(vec![tool("echo")]))
            .collect()
            .await;
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], Ok(StreamEvent::Text(text)) if text.contains("__NEOMIND_TOKEN_PROMPT:123__"))
        );
        assert!(matches!(&events[1], Ok(StreamEvent::ToolCalls(calls)) if calls.len() == 1));
    }

    #[tokio::test]
    async fn native_requests_do_not_interpret_even_the_text_envelope() {
        use futures::StreamExt;
        use neomind_core::llm::backend::StreamEvent;
        let text =
            "<neomind_tool_calls>[{\"name\":\"echo\",\"arguments\":{}}]</neomind_tool_calls>";
        let stream = futures::stream::iter(vec![Ok(StreamEvent::Text(text.into()))]);
        let events: Vec<_> = adapt_stream(Box::pin(stream), None).collect().await;
        assert!(matches!(&events[0], Ok(StreamEvent::Text(got)) if got == text));
        assert!(!format_teaching().contains("ALWAYS"));
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
                assert!(text.contains("Tool protocol"));
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
