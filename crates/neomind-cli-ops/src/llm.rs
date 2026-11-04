use crate::types::CliResponse;
use crate::ApiClient;
use anyhow::Result;
use serde_json::json;

/// List configured LLM backends
pub async fn list_backends(client: &ApiClient) -> Result<CliResponse> {
    let mut data = client.get("/llm-backends").await?;

    // Add capability tags to each backend for easy scanning
    if let Some(backends) = data.get_mut("backends").and_then(|b| b.as_array_mut()) {
        for backend in backends.iter_mut() {
            let mut tags: Vec<String> = Vec::new();

            if backend
                .get("is_active")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                tags.push("active".to_string());
            }
            if let Some(caps) = backend.get("capabilities") {
                if caps
                    .get("multimodal")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    tags.push("multimodal".to_string());
                }
                if caps
                    .get("supports_images")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    tags.push("vision".to_string());
                }
                if caps
                    .get("function_calling")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    tags.push("tool-use".to_string());
                }
                if caps
                    .get("thinking_display")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    tags.push("thinking".to_string());
                }
                if caps
                    .get("streaming")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    tags.push("streaming".to_string());
                }
                if let Some(ctx) = caps.get("max_context").and_then(|v| v.as_u64()) {
                    if ctx > 0 {
                        tags.push(format!("ctx:{:.0}k", ctx as f64 / 1024.0));
                    }
                }
            }
            if let Some(healthy) = backend.get("healthy").and_then(|v| v.as_bool()) {
                if !healthy {
                    tags.push("unhealthy".to_string());
                }
            }

            backend["_tags"] = json!(tags);
        }
    }

    Ok(CliResponse::success(data, "LLM backends listed"))
}

/// Get LLM backend by ID
pub async fn get_backend(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let data = client.get(&format!("/llm-backends/{}", id)).await?;
    Ok(CliResponse::success(data, "LLM backend retrieved"))
}

/// List available models from Ollama
///
/// `endpoint` overrides the Ollama server to query (the API defaults to
/// http://localhost:11434 when absent) — the skill teaches
/// `llm models --endpoint http://gpu-host:11434` for remote servers.
pub async fn list_ollama_models(client: &ApiClient, endpoint: Option<&str>) -> Result<CliResponse> {
    let path = match endpoint.map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) => format!("/llm-backends/ollama/models?endpoint={}", urlencode(e)),
        None => "/llm-backends/ollama/models".to_string(),
    };
    let data = client.get(&path).await?;
    Ok(CliResponse::success(data, "Ollama models listed"))
}

/// Percent-encode a query value (keep URL-safe characters as-is).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Create a new LLM backend
pub async fn create_backend(
    client: &ApiClient,
    name: &str,
    backend_type: &str,
    endpoint: &str,
    model: &str,
    api_key: Option<&str>,
    temperature: Option<f64>,
) -> Result<CliResponse> {
    let mut body = json!({
        "name": name,
        "backend_type": backend_type,
        "endpoint": endpoint,
        "model": model,
    });
    if let Some(k) = api_key {
        body["api_key"] = json!(k);
    }
    if let Some(t) = temperature {
        body["temperature"] = json!(t);
    }

    let data = client.post("/llm-backends", &body).await?;

    Ok(CliResponse::success(data, "LLM backend created"))
}

/// Update an existing LLM backend
pub async fn update_backend(
    client: &ApiClient,
    id: &str,
    name: Option<&str>,
    model: Option<&str>,
    endpoint: Option<&str>,
    api_key: Option<&str>,
    temperature: Option<f64>,
) -> Result<CliResponse> {
    let mut body = json!({});
    if let Some(n) = name {
        body["name"] = json!(n);
    }
    if let Some(m) = model {
        body["model"] = json!(m);
    }
    if let Some(e) = endpoint {
        body["endpoint"] = json!(e);
    }
    if let Some(k) = api_key {
        body["api_key"] = json!(k);
    }
    if let Some(t) = temperature {
        body["temperature"] = json!(t);
    }

    let data = client.put(&format!("/llm-backends/{}", id), &body).await?;
    Ok(CliResponse::success(data, "LLM backend updated"))
}

/// Delete an LLM backend
pub async fn delete_backend(client: &ApiClient, id: &str) -> Result<CliResponse> {
    // DELETE is idempotent in the API; a nonexistent ID must not look like a real deletion.
    client
        .get(&format!("/llm-backends/{}", id))
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "Cannot verify LLM backend ID; no deletion attempted. Run `neomind llm list` \
             and use the exact `id`, not the backend name. Verification error: {error:#}"
            )
        })?;
    client.delete(&format!("/llm-backends/{}", id)).await?;
    Ok(CliResponse::success(
        json!({ "id": id }),
        "LLM backend deleted",
    ))
}

/// Activate an LLM backend (set as default)
pub async fn activate_backend(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let body = json!({});
    let data = client
        .post(&format!("/llm-backends/{}/activate", id), &body)
        .await?;
    Ok(CliResponse::success(data, "LLM backend activated"))
}

/// Test connection to an LLM backend
pub async fn test_backend(client: &ApiClient, id: &str) -> Result<CliResponse> {
    let body = json!({});
    let data = client
        .post(&format!("/llm-backends/{}/test", id), &body)
        .await?;
    Ok(CliResponse::success(data, "LLM backend test completed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::mpsc::{self, Sender};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    fn mock_api(codes: Vec<u16>) -> (ApiClient, Sender<()>, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let (stop, stopped) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut codes = VecDeque::from(codes);
            let mut requests = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if stopped.try_recv().is_ok() {
                    break;
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let request = {
                            let mut reader = BufReader::new(&stream);
                            let mut first = String::new();
                            reader.read_line(&mut first).unwrap();
                            loop {
                                let mut line = String::new();
                                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                                    break;
                                }
                            }
                            first.trim().to_string()
                        };
                        requests.push(request);
                        let status = codes.pop_front().unwrap_or(200);
                        let body = if status == 200 {
                            r#"{"success":true,"data":{"backend":{"id":"existing-id"}}}"#
                        } else {
                            r#"{"error":"verification or mutation rejected"}"#
                        };
                        write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(e) => panic!("test listener: {e}"),
                }
            }
            requests
        });
        (
            ApiClient::with_base_url(&format!("http://{address}/api")),
            stop,
            thread,
        )
    }

    fn finish(stop: Sender<()>, thread: JoinHandle<Vec<String>>) -> Vec<String> {
        let _ = stop.send(());
        thread.join().unwrap()
    }

    #[tokio::test]
    async fn delete_rejects_unknown_id_without_sending_delete() {
        let (client, stop, thread) = mock_api(vec![404]);
        let result = delete_backend(&client, "friendly-name").await;
        let requests = finish(stop, thread);
        let message = result.unwrap_err().to_string();
        assert!(message.contains("neomind llm list"));
        assert!(!message.contains("neomind device list"));
        assert!(message.contains("404"));
        assert_eq!(
            requests,
            vec!["GET /api/llm-backends/friendly-name HTTP/1.1"]
        );
    }

    #[tokio::test]
    async fn delete_verifies_existing_id_before_mutation() {
        let (client, stop, thread) = mock_api(vec![200, 200]);
        let result = delete_backend(&client, "existing-id").await;
        let requests = finish(stop, thread);
        assert!(result.is_ok());
        assert_eq!(
            requests,
            vec![
                "GET /api/llm-backends/existing-id HTTP/1.1",
                "DELETE /api/llm-backends/existing-id HTTP/1.1"
            ]
        );
    }

    #[tokio::test]
    async fn delete_does_not_mutate_when_verification_is_unavailable() {
        let (client, stop, thread) = mock_api(vec![503]);
        let result = delete_backend(&client, "existing-id").await;
        let requests = finish(stop, thread);
        let message = result.unwrap_err().to_string();
        assert!(message.contains("503"));
        assert_eq!(requests, vec!["GET /api/llm-backends/existing-id HTTP/1.1"]);
    }

    #[tokio::test]
    async fn delete_propagates_rejected_mutation_after_valid_lookup() {
        let (client, stop, thread) = mock_api(vec![200, 409]);
        let result = delete_backend(&client, "existing-id").await;
        let requests = finish(stop, thread);
        assert!(result.is_err());
        assert_eq!(requests.len(), 2);
        assert!(requests[1].starts_with("DELETE "));
    }
}
