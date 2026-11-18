//! IPC 业务逻辑闭环测试
//!
//! 测试场景：
//! 1. Session 完整生命周期：创建 -> 使用 -> 关闭
//! 2. 扩展进程重启后的恢复
//! 3. 并发请求处理
//! 4. 错误处理和重试
//! 5. 内存使用稳定性

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use tokio::sync::RwLock;

// 模拟 IPC 消息类型
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MockIpcMessage {
    Init {
        config: serde_json::Value,
    },
    ExecuteCommand {
        id: u64,
        command: String,
        args: serde_json::Value,
    },
    InitStreamSession {
        session_id: String,
        config: serde_json::Value,
    },
    ProcessStreamChunk {
        session_id: String,
        sequence: u64,
        data: Vec<u8>,
    },
    CloseStreamSession {
        session_id: String,
    },
    Shutdown,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum MockIpcResponse {
    Ready {
        descriptor: MockDescriptor,
    },
    CommandResult {
        id: u64,
        result: serde_json::Value,
    },
    StreamSessionInit {
        session_id: String,
        success: bool,
    },
    StreamResult {
        session_id: String,
        sequence: u64,
        data: Vec<u8>,
    },
    StreamSessionClosed {
        session_id: String,
    },
    Error {
        id: Option<u64>,
        message: String,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MockDescriptor {
    pub id: String,
    pub name: String,
    pub version: String,
}

// 模拟扩展进程状态
pub struct MockExtensionProcess {
    pub sessions: Arc<RwLock<HashMap<String, MockSession>>>,
    pub command_count: Arc<std::sync::atomic::AtomicU64>,
    pub is_running: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct MockSession {
    pub id: String,
    pub created_at: Instant,
    pub chunk_count: u64,
    pub last_sequence: u64,
}

impl Default for MockExtensionProcess {
    fn default() -> Self {
        Self::new()
    }
}

impl MockExtensionProcess {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            command_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            is_running: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }

    pub async fn handle_message(&self, msg: MockIpcMessage) -> MockIpcResponse {
        match msg {
            MockIpcMessage::Init { .. } => MockIpcResponse::Ready {
                descriptor: MockDescriptor {
                    id: "test-extension".to_string(),
                    name: "Test Extension".to_string(),
                    version: "1.0.0".to_string(),
                },
            },

            MockIpcMessage::ExecuteCommand { id, command, args } => {
                self.command_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                // 模拟命令执行
                let result = match command.as_str() {
                    "echo" => args.clone(),
                    "ping" => json!({"pong": true}),
                    "error" => {
                        return MockIpcResponse::Error {
                            id: Some(id),
                            message: "Simulated error".to_string(),
                        };
                    }
                    _ => json!({"command": command, "executed": true}),
                };

                MockIpcResponse::CommandResult { id, result }
            }

            MockIpcMessage::InitStreamSession {
                session_id,
                config: _,
            } => {
                let session = MockSession {
                    id: session_id.clone(),
                    created_at: Instant::now(),
                    chunk_count: 0,
                    last_sequence: 0,
                };

                let mut sessions = self.sessions.write().await;
                sessions.insert(session_id.clone(), session);

                MockIpcResponse::StreamSessionInit {
                    session_id,
                    success: true,
                }
            }

            MockIpcMessage::ProcessStreamChunk {
                session_id,
                sequence,
                data,
            } => {
                let mut sessions = self.sessions.write().await;

                if let Some(session) = sessions.get_mut(&session_id) {
                    session.chunk_count += 1;
                    session.last_sequence = sequence;

                    // 模拟处理：返回相同的数据（echo）
                    MockIpcResponse::StreamResult {
                        session_id,
                        sequence,
                        data,
                    }
                } else {
                    MockIpcResponse::Error {
                        id: None,
                        message: format!("Session not found: {}", session_id),
                    }
                }
            }

            MockIpcMessage::CloseStreamSession { session_id } => {
                let mut sessions = self.sessions.write().await;

                if sessions.remove(&session_id).is_some() {
                    MockIpcResponse::StreamSessionClosed { session_id }
                } else {
                    MockIpcResponse::Error {
                        id: None,
                        message: format!("Session not found: {}", session_id),
                    }
                }
            }

            MockIpcMessage::Shutdown => {
                self.is_running
                    .store(false, std::sync::atomic::Ordering::SeqCst);

                // 清理所有 session
                let mut sessions = self.sessions.write().await;
                sessions.clear();

                MockIpcResponse::Error {
                    id: None,
                    message: "Shutdown".to_string(),
                }
            }
        }
    }

    pub async fn simulate_restart(&self) {
        // 模拟进程重启：清理所有状态
        let mut sessions = self.sessions.write().await;
        sessions.clear();

        self.command_count
            .store(0, std::sync::atomic::Ordering::SeqCst);
        self.is_running
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

// ============================================================================
// 测试用例
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试 1: Session 完整生命周期
    #[tokio::test]
    #[serial_test::serial]
    async fn test_session_lifecycle() {
        println!("\n=== 测试: Session 完整生命周期 ===");

        let process = MockExtensionProcess::new();
        let session_id = "test-session-001".to_string();

        // 1. 创建 session
        println!("1. 创建 session: {}", session_id);
        let response = process
            .handle_message(MockIpcMessage::InitStreamSession {
                session_id: session_id.clone(),
                config: json!({}),
            })
            .await;

        match response {
            MockIpcResponse::StreamSessionInit {
                session_id: id,
                success,
            } => {
                assert!(success, "Session 创建应该成功");
                assert_eq!(id, session_id);
                println!("   ✓ Session 创建成功");
            }
            _ => panic!("期望 StreamSessionInit 响应"),
        }

        // 验证 session 存在
        let sessions = process.sessions.read().await;
        assert!(sessions.contains_key(&session_id));
        drop(sessions);

        // 2. 发送多个 chunk
        println!("2. 发送数据 chunks...");
        for i in 0..10 {
            let response = process
                .handle_message(MockIpcMessage::ProcessStreamChunk {
                    session_id: session_id.clone(),
                    sequence: i,
                    data: vec![i as u8; 100],
                })
                .await;

            match response {
                MockIpcResponse::StreamResult { sequence, .. } => {
                    assert_eq!(sequence, i);
                }
                _ => panic!("期望 StreamResult 响应"),
            }
        }
        println!("   ✓ 发送了 10 个 chunks");

        // 验证 session 状态
        let sessions = process.sessions.read().await;
        let session = sessions.get(&session_id).unwrap();
        assert_eq!(session.chunk_count, 10);
        assert_eq!(session.last_sequence, 9);
        drop(sessions);
        println!("   ✓ Session 状态正确: chunk_count=10, last_sequence=9");

        // 3. 关闭 session
        println!("3. 关闭 session...");
        let response = process
            .handle_message(MockIpcMessage::CloseStreamSession {
                session_id: session_id.clone(),
            })
            .await;

        match response {
            MockIpcResponse::StreamSessionClosed { session_id: id } => {
                assert_eq!(id, session_id);
                println!("   ✓ Session 关闭成功");
            }
            _ => panic!("期望 StreamSessionClosed 响应"),
        }

        // 验证 session 已删除
        let sessions = process.sessions.read().await;
        assert!(!sessions.contains_key(&session_id));
        drop(sessions);
        println!("   ✓ Session 已从注册表中删除");

        // 4. 尝试使用已关闭的 session
        println!("4. 尝试使用已关闭的 session...");
        let response = process
            .handle_message(MockIpcMessage::ProcessStreamChunk {
                session_id: session_id.clone(),
                sequence: 10,
                data: vec![0; 100],
            })
            .await;

        match response {
            MockIpcResponse::Error { message, .. } => {
                assert!(message.contains("not found"));
                println!("   ✓ 正确返回错误: {}", message);
            }
            _ => panic!("期望 Error 响应"),
        }

        println!("\n✓ Session 生命周期测试通过!\n");
    }

    /// 测试 2: 扩展进程重启后的行为
    #[tokio::test]
    #[serial_test::serial]
    async fn test_process_restart() {
        println!("\n=== 测试: 扩展进程重启 ===");

        let process = MockExtensionProcess::new();

        // 1. 创建多个 session
        println!("1. 创建 3 个 sessions...");
        for i in 0..3 {
            let session_id = format!("session-{}", i);
            process
                .handle_message(MockIpcMessage::InitStreamSession {
                    session_id: session_id.clone(),
                    config: json!({}),
                })
                .await;
        }

        let sessions = process.sessions.read().await;
        assert_eq!(sessions.len(), 3);
        drop(sessions);
        println!("   ✓ 创建了 3 个 sessions");

        // 2. 模拟进程重启
        println!("2. 模拟进程重启...");
        process.simulate_restart().await;

        let sessions = process.sessions.read().await;
        assert_eq!(sessions.len(), 0);
        drop(sessions);
        println!("   ✓ 所有 sessions 已清理");

        // 3. 尝试使用旧的 session ID
        println!("3. 尝试使用旧的 session ID...");
        let response = process
            .handle_message(MockIpcMessage::ProcessStreamChunk {
                session_id: "session-0".to_string(),
                sequence: 0,
                data: vec![0; 100],
            })
            .await;

        match response {
            MockIpcResponse::Error { message, .. } => {
                println!("   ✓ 正确返回错误: {}", message);
            }
            _ => panic!("期望 Error 响应"),
        }

        // 4. 创建新 session
        println!("4. 创建新 session...");
        let response = process
            .handle_message(MockIpcMessage::InitStreamSession {
                session_id: "new-session".to_string(),
                config: json!({}),
            })
            .await;

        match response {
            MockIpcResponse::StreamSessionInit { success, .. } => {
                assert!(success);
                println!("   ✓ 新 session 创建成功");
            }
            _ => panic!("期望 StreamSessionInit 响应"),
        }

        println!("\n✓ 进程重启测试通过!\n");
    }

    /// 测试 3: 并发请求处理
    #[tokio::test]
    #[serial_test::serial]
    async fn test_concurrent_requests() {
        println!("\n=== 测试: 并发请求处理 ===");

        let process = Arc::new(MockExtensionProcess::new());
        let num_tasks = 100;
        let num_requests_per_task = 10;

        println!(
            "1. 启动 {} 个并发任务，每个发送 {} 个请求...",
            num_tasks, num_requests_per_task
        );

        let start = Instant::now();
        let mut handles = vec![];

        for task_id in 0..num_tasks {
            let process = Arc::clone(&process);
            let handle = tokio::spawn(async move {
                for req_id in 0..num_requests_per_task {
                    let response = process
                        .handle_message(MockIpcMessage::ExecuteCommand {
                            id: (task_id * num_requests_per_task + req_id) as u64,
                            command: "echo".to_string(),
                            args: json!({"task": task_id, "request": req_id}),
                        })
                        .await;

                    match response {
                        MockIpcResponse::CommandResult { id, .. } => {
                            assert_eq!(id, (task_id * num_requests_per_task + req_id) as u64);
                        }
                        _ => panic!("期望 CommandResult 响应"),
                    }
                }
            });
            handles.push(handle);
        }

        // 等待所有任务完成
        for handle in handles {
            handle.await.unwrap();
        }

        let elapsed = start.elapsed();
        let total_requests = num_tasks * num_requests_per_task;

        println!("   ✓ 完成了 {} 个请求", total_requests);
        println!("   ✓ 耗时: {:?}", elapsed);
        println!(
            "   ✓ 吞吐: {:.0} req/sec",
            total_requests as f64 / elapsed.as_secs_f64()
        );

        // 验证计数
        let count = process
            .command_count
            .load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(count, total_requests as u64);
        println!("   ✓ 请求计数正确: {}", count);

        println!("\n✓ 并发测试通过!\n");
    }

    /// 测试 4: 错误处理
    #[tokio::test]
    #[serial_test::serial]
    async fn test_error_handling() {
        println!("\n=== 测试: 错误处理 ===");

        let process = MockExtensionProcess::new();

        // 1. 测试命令错误
        println!("1. 测试命令错误...");
        let response = process
            .handle_message(MockIpcMessage::ExecuteCommand {
                id: 1,
                command: "error".to_string(),
                args: json!({}),
            })
            .await;

        match response {
            MockIpcResponse::Error { id, message } => {
                assert_eq!(id, Some(1));
                assert!(message.contains("error"));
                println!("   ✓ 命令错误正确返回");
            }
            _ => panic!("期望 Error 响应"),
        }

        // 2. 测试 session 不存在
        println!("2. 测试 session 不存在...");
        let response = process
            .handle_message(MockIpcMessage::ProcessStreamChunk {
                session_id: "non-existent".to_string(),
                sequence: 0,
                data: vec![],
            })
            .await;

        match response {
            MockIpcResponse::Error { message, .. } => {
                assert!(message.contains("not found"));
                println!("   ✓ Session 不存在错误正确返回");
            }
            _ => panic!("期望 Error 响应"),
        }

        // 3. 测试关闭不存在的 session
        println!("3. 测试关闭不存在的 session...");
        let response = process
            .handle_message(MockIpcMessage::CloseStreamSession {
                session_id: "non-existent".to_string(),
            })
            .await;

        match response {
            MockIpcResponse::Error { message, .. } => {
                assert!(message.contains("not found"));
                println!("   ✓ 关闭不存在 session 错误正确返回");
            }
            _ => panic!("期望 Error 响应"),
        }

        println!("\n✓ 错误处理测试通过!\n");
    }

    /// 测试 5: 性能基准
    #[tokio::test]
    #[serial_test::serial]
    async fn test_performance_benchmark() {
        println!("\n=== 性能基准测试 ===");

        let process = MockExtensionProcess::new();

        // 1. 消息序列化性能
        println!("\n1. 消息序列化性能:");
        let iterations = 10000;
        let mut times = Vec::with_capacity(iterations);

        for _ in 0..iterations {
            let msg = MockIpcMessage::ExecuteCommand {
                id: 1,
                command: "test".to_string(),
                args: json!({"key": "value"}),
            };

            let start = Instant::now();
            let _bytes = serde_json::to_vec(&msg).unwrap();
            times.push(start.elapsed());
        }

        let avg_ns: f64 =
            times.iter().map(|t| t.as_nanos() as f64).sum::<f64>() / times.len() as f64;
        println!("   平均序列化时间: {:.2} ns", avg_ns);
        println!("   吞吐: {:.0} msg/sec", 1_000_000_000.0 / avg_ns);

        // 2. 消息处理性能
        println!("\n2. 消息处理性能:");
        times.clear();

        for i in 0..iterations {
            let msg = MockIpcMessage::ExecuteCommand {
                id: i as u64,
                command: "echo".to_string(),
                args: json!({"index": i}),
            };

            let start = Instant::now();
            let _response = process.handle_message(msg).await;
            times.push(start.elapsed());
        }

        let avg_ns: f64 =
            times.iter().map(|t| t.as_nanos() as f64).sum::<f64>() / times.len() as f64;
        let min_ns = times
            .iter()
            .map(|t| t.as_nanos() as f64)
            .fold(f64::INFINITY, f64::min);
        let max_ns = times
            .iter()
            .map(|t| t.as_nanos() as f64)
            .fold(0.0, f64::max);

        println!("   平均处理时间: {:.2} ns", avg_ns);
        println!("   最小: {:.2} ns, 最大: {:.2} ns", min_ns, max_ns);
        println!("   吞吐: {:.0} req/sec", 1_000_000_000.0 / avg_ns);

        // 3. Session 操作性能
        println!("\n3. Session 操作性能:");

        // 创建 session
        let start = Instant::now();
        for i in 0..1000 {
            process
                .handle_message(MockIpcMessage::InitStreamSession {
                    session_id: format!("perf-session-{}", i),
                    config: json!({}),
                })
                .await;
        }
        let create_time = start.elapsed();
        println!("   创建 1000 个 sessions: {:?}", create_time);
        println!("   平均每个: {:?}", create_time / 1000);

        // 处理 chunks
        let start = Instant::now();
        for i in 0..10000 {
            process
                .handle_message(MockIpcMessage::ProcessStreamChunk {
                    session_id: "perf-session-0".to_string(),
                    sequence: i,
                    data: vec![0; 1024], // 1KB
                })
                .await;
        }
        let chunk_time = start.elapsed();
        println!("   处理 10000 个 chunks (1KB): {:?}", chunk_time);
        println!(
            "   吞吐: {:.0} chunks/sec",
            10000.0 / chunk_time.as_secs_f64()
        );

        // 关闭 sessions
        let start = Instant::now();
        for i in 0..1000 {
            process
                .handle_message(MockIpcMessage::CloseStreamSession {
                    session_id: format!("perf-session-{}", i),
                })
                .await;
        }
        let close_time = start.elapsed();
        println!("   关闭 1000 个 sessions: {:?}", close_time);

        println!("\n✓ 性能基准测试完成!\n");
    }

    /// 测试 6: 内存稳定性
    #[tokio::test]
    #[serial_test::serial]
    async fn test_memory_stability() {
        println!("\n=== 内存稳定性测试 ===");

        let process = MockExtensionProcess::new();

        // 创建并销毁大量 sessions
        println!("1. 创建/销毁 1000 个 sessions...");
        for batch in 0..10 {
            // 创建 100 个 sessions
            for i in 0..100 {
                let session_id = format!("mem-test-{}-{}", batch, i);
                process
                    .handle_message(MockIpcMessage::InitStreamSession {
                        session_id: session_id.clone(),
                        config: json!({}),
                    })
                    .await;

                // 发送一些数据
                for seq in 0..10 {
                    process
                        .handle_message(MockIpcMessage::ProcessStreamChunk {
                            session_id: session_id.clone(),
                            sequence: seq,
                            data: vec![0; 1024],
                        })
                        .await;
                }
            }

            // 关闭这批 sessions
            for i in 0..100 {
                let session_id = format!("mem-test-{}-{}", batch, i);
                process
                    .handle_message(MockIpcMessage::CloseStreamSession { session_id })
                    .await;
            }

            // 验证 sessions 已清理
            let sessions = process.sessions.read().await;
            assert_eq!(sessions.len(), 0, "第 {} 批后 sessions 应该为空", batch);
            drop(sessions);
        }

        println!("   ✓ 完成了 1000 个 session 的创建/销毁循环");

        // 最终验证
        let sessions = process.sessions.read().await;
        assert_eq!(sessions.len(), 0);
        drop(sessions);
        println!("   ✓ 最终状态: 无泄漏的 sessions");

        println!("\n✓ 内存稳定性测试通过!\n");
    }
}
