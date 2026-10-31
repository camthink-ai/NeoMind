//! Exercise persisted backend selection through to the actual HTTP request.
use super::*;
use futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn capture_endpoint() -> (String, tokio::sync::mpsc::Receiver<serde_json::Value>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            let body = loop {
                let n = socket.read(&mut buffer).await.unwrap();
                assert_ne!(n, 0);
                request.extend_from_slice(&buffer[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let length = String::from_utf8_lossy(&request[..end])
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        if length == 0 {
                            break serde_json::Value::Null;
                        }
                        break serde_json::from_slice::<serde_json::Value>(
                            &request[end + 4..end + 4 + length],
                        )
                        .unwrap();
                    }
                }
            };
            if body.is_null() {
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
                continue;
            }
            let streaming = body["stream"].as_bool().unwrap_or(false);
            tx.send(body).await.unwrap();
            let (content_type, response) = if streaming {
                ("text/event-stream", "data: {\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n".to_owned())
            } else {
                ("application/json", serde_json::json!({"id":"test", "model":"test", "created":0, "object":"chat.completion", "choices":[{"message":{"role":"assistant","content":"done"},"finish_reason":"stop"}], "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}).to_string())
            };
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
        }
    });
    (format!("http://{address}"), rx)
}

async fn assert_sampling(
    rx: &mut tokio::sync::mpsc::Receiver<serde_json::Value>,
    temperature: f64,
    top_p: f64,
) {
    let request = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        (request["temperature"].as_f64().unwrap() - temperature).abs() < 1e-6,
        "{request}"
    );
    assert!(
        (request["top_p"].as_f64().unwrap() - top_p).abs() < 1e-6,
        "{request}"
    );
    assert_eq!(request["top_k"], 40);
}

#[serial_test::serial]
#[tokio::test]
async fn selected_backend_defaults_reach_http_and_sessions_remain_isolated() {
    let (endpoint, mut requests) = capture_endpoint().await;
    let dir = tempfile::tempdir().unwrap();
    let store = neomind_storage::LlmBackendStore::open(dir.path().join("backends.redb")).unwrap();
    let backends = Arc::new(LlmBackendInstanceManager::new(store));
    for (id, temperature, top_p) in [("first", 1.0, 0.95), ("second", 0.2, 0.6)] {
        let mut instance = LlmBackendInstance::new(
            id.into(),
            id.into(),
            neomind_storage::LlmBackendType::LlamaCpp,
        );
        instance.endpoint = Some(endpoint.clone());
        instance.model = "test".into();
        instance.temperature = temperature;
        instance.top_p = top_p;
        instance.top_k = 40;
        backends.upsert_instance(instance).await.unwrap();
    }
    backends.set_active("first").await.unwrap();
    crate::llm_backends::set_instance_manager(backends.clone());
    let sessions = SessionManager::with_path(dir.path().join("sessions.redb")).unwrap();
    sessions
        .configure_llm_from_instance_manager()
        .await
        .unwrap();
    let first = sessions.create_session().await.unwrap();
    let iface = sessions.get_session(&first).await.unwrap().llm_interface();
    let mut stream = iface.chat_stream_with_history("hello", &[]).await.unwrap();
    while let Some(event) = stream.next().await {
        event.unwrap();
    }
    assert_sampling(&mut requests, 1.0, 0.95).await;

    let overridden = sessions
        .create_session_with_options(CreateSessionOptions {
            temperature: Some(0.4),
            ..Default::default()
        })
        .await
        .unwrap();
    sessions
        .configure_agent_by_backend_id(&overridden, "second")
        .await
        .unwrap();
    sessions
        .get_session(&overridden)
        .await
        .unwrap()
        .llm_interface()
        .chat("hello")
        .await
        .unwrap();
    assert_sampling(&mut requests, 0.4, 0.6).await;

    // Changing the globally active backend cannot retune an already-bound session.
    backends.set_active("second").await.unwrap();
    iface.chat("hello again").await.unwrap();
    assert_sampling(&mut requests, 1.0, 0.95).await;
    sessions
        .configure_agent_by_backend_id(&first, "second")
        .await
        .unwrap();
    iface.chat("after explicit switch").await.unwrap();
    assert_sampling(&mut requests, 0.2, 0.6).await;
    // The frontend activation endpoint uses this path for existing and future chats.
    let active = backends.get_instance("first").unwrap();
    sessions.set_llm_backend_instance(&active).await.unwrap();
    iface.chat("after activation").await.unwrap();
    assert_sampling(&mut requests, 1.0, 0.95).await;
    let new_session = sessions.create_session().await.unwrap();
    sessions
        .get_session(&new_session)
        .await
        .unwrap()
        .llm_interface()
        .chat("new chat")
        .await
        .unwrap();
    assert_sampling(&mut requests, 1.0, 0.95).await;
    sessions
        .get_session(&overridden)
        .await
        .unwrap()
        .llm_interface()
        .chat("explicit override still applies")
        .await
        .unwrap();
    assert_sampling(&mut requests, 0.4, 0.95).await;
}
