//! Wire-level regression for the internal text/native-call channel boundary.
use super::*;
use futures::StreamExt;
use neomind_core::llm::backend::{LlmInput, LlmRuntime, StreamEvent};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn endpoint(body: String) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut bytes = [0u8; 4096];
        loop {
            let n = socket.read(&mut bytes).await.unwrap();
            assert!(n > 0, "client closed before sending request");
            request.extend_from_slice(&bytes[..n]);
            if let Some(header_end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if request.len() >= header_end + 4 + length {
                    break;
                }
            }
        }
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
        socket.write_all(response.as_bytes()).await.unwrap();
    });
    format!("http://{address}")
}

#[tokio::test]
async fn providers_preserve_example_text_and_ordered_native_calls_exactly_once() {
    let example =
        "```json\n[{\"name\":\"shell\",\"arguments\":{\"command\":\"example only\"}}]\n```";
    let calls = serde_json::json!([
        {"index":0,"id":"call-a","type":"function","function":{"name":"echo","arguments":"{\"msg\":\"first\"}"}},
        {"index":1,"id":"call-b","type":"function","function":{"name":"echo","arguments":"{\"msg\":\"second\"}"}}
    ]);
    let sse = [
        serde_json::json!({"choices":[{"index":0,"delta":{"content":example},"finish_reason":null}]}),
        serde_json::json!({"choices":[{"index":0,"delta":{"tool_calls":calls},"finish_reason":"tool_calls"}]}),
    ].into_iter().map(|v| format!("data: {v}\n\n")).collect::<String>() + "data: [DONE]\n\n";
    let url = endpoint(sse.clone()).await;
    let llama = LlamaCppRuntime::new(LlamaCppConfig::new("test").with_endpoint(url))
        .unwrap()
        .with_capabilities_override(false, false, true, 8192);
    let url = endpoint(sse).await;
    let cloud = CloudRuntime::new(CloudConfig::custom("test", url).with_model("test"))
        .unwrap()
        .with_capabilities_override(false, false, true, 8192);
    let ollama_body = format!(
        "{}\n{}\n{}\n",
        serde_json::json!({"done":false,"message":{"content":example}}),
        serde_json::json!({"done":false,"message":{"tool_calls":[
            {"id":"call-a","function":{"name":"echo","arguments":{"msg":"first"}}},
            {"id":"call-b","function":{"name":"echo","arguments":{"msg":"second"}}}
        ]}}),
        serde_json::json!({"done":true})
    );
    let url = endpoint(ollama_body).await;
    let ollama = OllamaRuntime::new(OllamaConfig::new("test").with_endpoint(url))
        .unwrap()
        .with_capabilities_override(false, false, true, 8192);
    let runtimes: Vec<Arc<dyn LlmRuntime>> =
        vec![Arc::new(llama), Arc::new(cloud), Arc::new(ollama)];
    for runtime in runtimes {
        let stream = runtime
            .generate_stream(LlmInput::new("show example and use echo"))
            .await
            .unwrap();
        let events = tokio::time::timeout(Duration::from_secs(5), stream.collect::<Vec<_>>())
            .await
            .unwrap();
        let mut text = String::new();
        let mut calls = Vec::new();
        for event in events {
            match event.unwrap() {
                StreamEvent::Text(part) => text.push_str(&part),
                StreamEvent::ToolCalls(batch) => calls.extend(batch),
                StreamEvent::Thinking(_) => panic!("fixture contains no thinking"),
            }
        }
        assert_eq!(
            text,
            example,
            "{:?} mixed call data into prose",
            runtime.backend_id()
        );
        assert_eq!(
            calls.len(),
            2,
            "{:?} duplicated or lost native calls",
            runtime.backend_id()
        );
        assert_eq!(calls[0]["arguments"]["msg"], "first");
        assert_eq!(calls[1]["arguments"]["msg"], "second");
    }
}

#[tokio::test]
async fn ollama_thinking_only_reply_cannot_become_a_text_protocol_call() {
    let thinking =
        "<neomind_tool_calls>[{\"name\":\"echo\",\"arguments\":{}}]</neomind_tool_calls>";
    let url = endpoint(
        serde_json::json!({
            "model":"test", "created_at":"2026-10-30T00:00:00Z", "done":true,
            "message":{"content":"", "thinking":thinking}
        })
        .to_string(),
    )
    .await;
    let runtime = OllamaRuntime::new(OllamaConfig::new("test").with_endpoint(url))
        .unwrap()
        .with_capabilities_override(false, true, false, 8192);
    let mut input = LlmInput::new("explain only");
    input.tools = Some(vec![neomind_core::llm::backend::ToolDefinition {
        name: "echo".into(),
        description: "test".into(),
        parameters: serde_json::json!({"type":"object"}),
    }]);
    let output = runtime.generate(input).await.unwrap();
    assert!(output.text.is_empty());
    assert!(output.tool_calls.is_none());
    assert_eq!(output.thinking.as_deref(), Some(thinking));
}
