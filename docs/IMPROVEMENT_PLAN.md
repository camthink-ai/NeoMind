# NeoMind 改进优先级工作计划

## 文档信息

| 项目 | 值 |
|------|-----|
| 创建日期 | 2026-01-30 |
| 基准项目 | moltbot/moltbot |
| 预计工期 | 8-9 周 |
| 当前版本 | NeoMind v0.x |

---

## 一、问题分析总结

### 1.1 当前超时和中断问题

**问题日志**：
```
2026-01-30T01:59:30.532226Z [ollama.rs] Max thinking chars reached (10001). Skipping remaining thinking chunks, waiting for content.
2026-01-30T02:00:44.223375Z Stream timeout after 120s
```

**问题根源**：

| 配置项 | 当前值 | 位置 | 问题 |
|--------|--------|------|------|
| `MAX_THINKING_CHARS` | 10,000 | `crates/llm/src/backends/ollama.rs:30` | 硬编码，超过后跳过思考内容 |
| `MAX_THINKING_TIME_SECS` | 60 | `crates/llm/src/backends/ollama.rs:37` | 硬编码，思考超时 |
| `max_stream_duration` | 120s | `crates/agent/src/agent/streaming.rs:50` | 流式传输总超时 |
| `stream_timeout` | 120s | `crates/api/src/handlers/sessions.rs:38` | API会话超时 |

**问题链**：
1. qwen3-vl:2b 生成思考内容超过 10,000 字符
2. 超过后跳过剩余思考，等待内容生成
3. 思考 + 内容生成总时长超过 120 秒
4. 无状态保存机制，任务直接中断丢失

### 1.2 与 Moltbot 对比

| 特性 | Moltbot | NeoMind | 差距 |
|------|---------|---------|------|
| **上下文压缩** | `reserveTokensFloor`, `maxHistoryShare`, `softThresholdTokens` | 无配置 | ❌ 无上下文管理策略 |
| **记忆刷新** | `memoryFlush.prompt` 触发持久化 | 无自动刷新 | ❌ 中断后状态丢失 |
| **思考限制** | 可配置 per-agent | 硬编码 10,000 | ❌ 无法调整 |
| **超时处理** | 分阶段警告 + 状态保存 | 硬超时中断 | ❌ 无恢复机制 |
| **嵌入模型** | OpenAI/Gemini 批处理 | SimpleHash 假嵌入 | ❌ 无语义搜索 |
| **全文搜索** | FTS5 + BM25 | 无 | ❌ 无关键词搜索 |
| **混合搜索** | Vector + BM25 融合 | 无 | ❌ 搜索精度低 |
| **嵌入缓存** | LRU + 持久化 | 无 | ❌ 重复计算 |

---

## 二、优先级工作计划

### 🔴 P0 - 紧急修复（超时与中断问题）

> **预计工期**: 1-3 周
> **预期效果**: 超时率从 30% 降至 <5%，中断后可恢复

#### P0.1 配置化思考限制和超时

**目标**：将硬编码的限制改为可配置参数

**效果预期**：
- 思考字符限制从 10K 增加到 50K，覆盖 99% 的复杂推理场景
- 超时时间从 120s 增加到 300s，给图像推理等耗时任务足够时间
- 配置可按模型调整，适配不同能力的模型

**后端改动**：

```rust
// crates/core/src/llm/backend.rs 新增配置结构
pub struct StreamConfig {
    /// Maximum thinking characters before cutoff
    pub max_thinking_chars: usize,

    /// Maximum thinking time in seconds
    pub max_thinking_time_secs: u64,

    /// Total stream timeout in seconds
    pub max_stream_duration_secs: u64,

    /// Progressive warning thresholds (in seconds)
    pub warning_thresholds: Vec<u64>,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_thinking_chars: 50000,      // 增加到 50K
            max_thinking_time_secs: 120,    // 增加到 120s
            max_stream_duration_secs: 300,  // 增加到 300s (5分钟)
            warning_thresholds: vec![60, 120, 180, 240],
        }
    }
}
```

**前端改动**：

```typescript
// web/src/types/index.ts 新增配置类型
export interface StreamConfig {
  maxThinkingChars: number
  maxThinkingTimeSecs: number
  maxStreamDurationSecs: number
  warningThresholds: number[]
}

// ServerMessage 新增事件类型
export interface ServerMessage {
  type: 'Thinking' | 'Content' | 'ToolCallStart' | 'ToolCallEnd' | 'StreamProgress' | 'Error' | 'end'

  // 新增 StreamProgress 事件数据
  elapsed?: number
  warning?: string
  remainingTime?: number
}
```

**影响文件清单**：
| 文件 | 改动类型 |
|------|----------|
| `crates/llm/src/backends/ollama.rs:25-40` | 移除硬编码常量 |
| `crates/agent/src/agent/streaming.rs:45-55` | 使用配置替代默认值 |
| `crates/api/src/handlers/sessions.rs:38` | 使用配置 |
| `web/src/types/index.ts:339-359` | 新增类型定义 |
| `web/src/components/chat/ChatContainer.tsx:83-143` | 处理进度事件 |
| `config.toml` | 新增 stream 配置节 |

**工作量**: 2-3 天

---

#### P0.2 分阶段超时警告机制

**目标**：在超时前向用户显示进度和警告

**效果预期**：
- 用户可实时看到任务执行进度
- 提前知晓可能的超时，减少焦虑
- 明确显示当前阶段（思考/生成/工具执行）

**后端改动**：

```rust
// crates/agent/src/agent/streaming.rs 新增进度报告
async fn report_stream_progress(
    safeguards: &StreamSafeguards,
    config: &StreamConfig,
    tx: &Sender,
) -> Result<()> {
    let start = Instant::now();
    let mut last_warning_idx = 0usize;

    loop {
        let elapsed = start.elapsed().as_secs();

        // 检查警告阈值
        for (i, threshold) in config.warning_thresholds.iter().enumerate() {
            if elapsed >= *threshold && i == last_warning_idx {
                send_event(&tx, ServerEvent::Progress {
                    elapsed,
                    message: format!("执行中... 已耗时 {} 秒", threshold),
                    stage: StreamStage::from_elapsed(elapsed),
                }).await;
                last_warning_idx = i + 1;
            }
        }

        // 计算剩余时间
        let remaining = safeguards.max_stream_duration.saturating_sub(elapsed);
        if remaining <= 30 && remaining % 10 == 0 {
            send_event(&tx, ServerEvent::Warning {
                message: format!("剩余时间约 {} 秒", remaining),
            }).await;
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[derive(Debug, Clone)]
pub enum StreamStage {
    Thinking,
    Generating,
    ToolExecution,
}

#[derive(Debug, Clone)]
pub enum ServerEvent {
    Progress { elapsed: u64, message: String, stage: StreamStage },
    Warning { message: String },
    // ... existing events
}
```

**前端改动**：

```tsx
// web/src/components/chat/StreamProgress.tsx 新组件
import { Progress } from "@/components/ui/progress"
import { Activity, AlertTriangle } from "lucide-react"

interface StreamProgressProps {
  elapsed: number
  totalDuration: number
  stage: 'thinking' | 'generating' | 'tool_execution'
  warning?: string
}

export function StreamProgress({
  elapsed,
  totalDuration,
  stage,
  warning
}: StreamProgressProps) {
  const progress = Math.min((elapsed / totalDuration) * 100, 100)

  const stageLabels = {
    thinking: '思考中',
    generating: '生成中',
    tool_execution: '工具执行'
  }

  return (
    <div className="flex items-center gap-3 text-sm text-muted-foreground px-4 py-2 bg-muted/30 rounded-lg">
      <Activity className="h-4 w-4 animate-pulse" />
      <div className="flex-1">
        <div className="flex items-center justify-between mb-1">
          <span>{stageLabels[stage]}</span>
          <span className="text-xs">{elapsed}s / {totalDuration}s</span>
        </div>
        <div className="h-1.5 bg-muted rounded-full overflow-hidden">
          <div
            className={`h-full transition-all duration-300 ${
              progress > 80 ? 'bg-yellow-500' : 'bg-blue-500'
            }`}
            style={{ width: `${progress}%` }}
          />
        </div>
      </div>
      {warning && (
        <span className="text-yellow-600 text-xs flex items-center gap-1">
          <AlertTriangle className="h-3 w-3" />
          {warning}
        </span>
      )}
    </div>
  )
}
```

**影响文件清单**：
| 文件 | 改动类型 |
|------|----------|
| `crates/agent/src/agent/streaming.rs:1106-1150` | 进度报告逻辑 |
| `web/src/components/chat/StreamProgress.tsx` | 新文件 |
| `web/src/components/chat/ChatContainer.tsx:322-328` | 集成进度条 |
| `web/src/i18n/locales/en/common.json` | 新增翻译键 |
| `web/src/i18n/locales/zh/common.json` | 新增翻译键 |

**工作量**: 1-2 天

---

#### P0.3 任务状态持久化与恢复

**目标**：中断后可恢复任务状态

**效果预期**：
- 任务中断后不丢失进度
- 用户可选择恢复或丢弃
- 中断恢复率 > 80%

**后端改动**：

```rust
// crates/storage/src/task_state.rs 新增文件
use redb::{Database, ReadableTable, WritableTable};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: String,
    pub session_id: String,
    pub user_message: String,
    pub stage: TaskStage,
    pub thinking_content: String,
    pub partial_response: String,
    pub tool_calls: Vec<ToolCallState>,
    pub elapsed_seconds: u64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStage {
    Thinking,
    Generating,
    ToolExecuting,
    Interrupted,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallState {
    pub name: String,
    pub arguments: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub completed: bool,
}

const TASK_STATES_TABLE: &str = "task_states";

pub struct TaskStateManager {
    db: Database,
}

impl TaskStateManager {
    pub fn new(db_path: &str) -> Result<Self> {
        let db = Database::create(db_path)?;
        Ok(Self { db })
    }

    pub fn save(&self, state: TaskState) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TASK_STATES_TABLE)?;
            let key = state.id.as_str();
            let value = serde_json::to_vec(&state)?;
            table.insert(key, value)?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn load(&self, task_id: &str) -> Result<Option<TaskState>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TASK_STATES_TABLE)?;
        Ok(table.get(task_id)?
            .map(|value| serde_json::from_slice(&value.value()).ok())
            .flatten())
    }

    pub fn list_interrupted(&self, session_id: &str) -> Result<Vec<TaskState>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(TASK_STATES_TABLE)?;

        let mut results = Vec::new();
        for item in table.iter()? {
            let (_, value) = item?;
            if let Ok(state) = serde_json::from_slice::<TaskState>(&value) {
                if state.session_id == session_id
                    && matches!(state.stage, TaskStage::Interrupted) {
                    results.push(state);
                }
            }
        }
        results.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(results)
    }

    pub fn delete(&self, task_id: &str) -> Result<()> {
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(TASK_STATES_TABLE)?;
            table.remove(task_id)?;
        }
        write_txn.commit()?;
        Ok(())
    }
}
```

**API 端点**：

```rust
// crates/api/src/handlers/tasks.rs 新增文件
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct ResumeTaskRequest {
    pub task_id: String,
}

#[derive(Serialize)]
pub struct ResumeTaskResponse {
    pub task_id: String,
    pub resumed: bool,
    pub message: String,
}

/// GET /api/tasks/interrupted?session_id=xxx
pub async fn list_interrupted_tasks(
    State(manager): State<Arc<TaskStateManager>>,
    Query(params): Query<TaskQueryParams>,
) -> Result<Json<Vec<TaskState>>, AppError> {
    let tasks = manager.list_interrupted(&params.session_id)?;
    Ok(Json(tasks))
}

/// POST /api/tasks/resume
pub async fn resume_task(
    State(agent): State<Arc<Agent>>,
    State(manager): State<Arc<TaskStateManager>>,
    Json(req): Json<ResumeTaskRequest>,
) -> Result<Json<ResumeTaskResponse>, AppError> {
    let task = manager.load(&req.task_id)?
        .ok_or_else(|| AppError::NotFound("Task not found".to_string()))?;

    // 从中断点恢复执行
    agent.resume_from_state(task).await?;

    Ok(Json(ResumeTaskResponse {
        task_id: req.task_id,
        resumed: true,
        message: "Task resumed successfully".to_string(),
    }))
}

/// DELETE /api/tasks/:id
pub async fn discard_task(
    State(manager): State<Arc<TaskStateManager>>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    manager.delete(&id)?;
    Ok(StatusCode::NO_CONTENT)
}
```

**前端改动**：

```tsx
// web/src/components/chat/InterruptedTaskDialog.tsx 新组件
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Progress } from "@/components/ui/progress"
import { AlertTriangle, Clock } from "lucide-react"
import type { TaskState } from "@/types"

interface InterruptedTaskDialogProps {
  taskState: TaskState | null
  onResume: (task: TaskState) => void
  onDiscard: () => void
}

export function InterruptedTaskDialog({
  taskState,
  onResume,
  onDiscard,
}: InterruptedTaskDialogProps) {
  if (!taskState) return null

  const progress = (taskState.elapsed_seconds / 300) * 100

  return (
    <Dialog open={!!taskState}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <AlertTriangle className="h-5 w-5 text-yellow-500" />
            任务被中断
          </DialogTitle>
          <DialogDescription>
            您的任务在执行过程中被中断，但已保存部分进度。
          </DialogDescription>
        </DialogHeader>

        <div className="py-4 space-y-4">
          {/* 进度信息 */}
          <div className="space-y-2">
            <div className="flex items-center justify-between text-sm">
              <span className="flex items-center gap-1 text-muted-foreground">
                <Clock className="h-3 w-3" />
                执行时间
              </span>
              <span>{taskState.elapsed_seconds}s / 300s</span>
            </div>
            <Progress value={progress} className="h-2" />
          </div>

          {/* 用户消息 */}
          <div className="text-sm">
            <span className="text-muted-foreground">原始请求：</span>
            <p className="mt-1 p-2 bg-muted rounded text-xs">
              {taskState.user_message}
            </p>
          </div>

          {/* 思考内容（如果有） */}
          {taskState.thinking_content && (
            <details className="text-sm">
              <summary className="cursor-pointer text-muted-foreground hover:text-foreground">
                思考内容 ({taskState.thinking_content.length} 字符)
              </summary>
              <pre className="mt-2 p-2 bg-muted rounded text-xs overflow-auto max-h-32">
                {taskState.thinking_content}
              </pre>
            </details>
          )}

          {/* 部分响应（如果有） */}
          {taskState.partial_response && (
            <div className="text-sm">
              <span className="text-muted-foreground">已生成内容：</span>
              <p className="mt-1 p-2 bg-muted rounded text-xs">
                {taskState.partial_response.slice(0, 200)}
                {taskState.partial_response.length > 200 ? '...' : ''}
              </p>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onDiscard}>
            丢弃
          </Button>
          <Button onClick={() => onResume(taskState)}>
            恢复任务
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
```

**类型定义**：

```typescript
// web/src/types/index.ts 新增
export interface TaskState {
  id: string
  session_id: string
  user_message: string
  stage: 'thinking' | 'generating' | 'tool_executing' | 'interrupted' | 'completed'
  thinking_content: string
  partial_response: string
  tool_calls: ToolCallState[]
  elapsed_seconds: number
  created_at: number
  updated_at: number
}

export interface ToolCallState {
  name: string
  arguments: unknown
  result?: unknown
  completed: boolean
}
```

**影响文件清单**：
| 文件 | 改动类型 |
|------|----------|
| `crates/storage/src/task_state.rs` | 新文件 |
| `crates/storage/src/lib.rs` | 导出新模块 |
| `crates/agent/src/agent/streaming.rs:121-143` | 保存状态 |
| `crates/agent/src/agent/mod.rs` | 新增 resume_from_state 方法 |
| `crates/api/src/handlers/tasks.rs` | 新文件 |
| `crates/api/src/lib.rs` | 注册新路由 |
| `web/src/components/chat/InterruptedTaskDialog.tsx` | 新文件 |
| `web/src/lib/api.ts` | 新增 API 调用 |
| `web/src/types/index.ts` | 新增类型定义 |
| `web/src/components/chat/ChatContainer.tsx` | 集成对话框 |

**工作量**: 3-4 天

---

### 🟠 P1 - 上下文管理改进

> **预计工期**: 1-2 周
> **预期效果**: 长对话稳定性提升，避免上下文溢出

#### P1.1 上下文压缩策略

**目标**：实现类似 Moltbot 的上下文管理

**效果预期**：
- 自动检测上下文接近限制
- 触发记忆刷新，持久化重要信息
- 压缩历史消息，保持对话连贯性
- 长对话不会因上下文过长而失败

**参考配置（Moltbot）**：
```json
{
  "compaction": {
    "mode": "default",
    "reserveTokensFloor": 20000,
    "maxHistoryShare": 0.5,
    "memoryFlush": {
      "enabled": true,
      "softThresholdTokens": 4000,
      "prompt": "Write any lasting notes to memory/YYYY-MM-DD.md; reply with NO_REPLY if nothing to store."
    }
  }
}
```

**触发时机**：
```
contextWindow - reserveTokensFloor - softThresholdTokens
```

**后端改动**：

```rust
// crates/core/src/llm/compaction.rs 新增文件
use serde::{Deserialize, Serialize};
use crate::message::Message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    /// Minimum tokens to reserve for system prompt and response
    pub reserve_tokens_floor: usize,

    /// Maximum share of context for history (0.0-1.0)
    pub max_history_share: f64,

    /// Threshold for triggering memory flush
    pub soft_threshold_tokens: usize,

    /// Memory flush prompt
    pub memory_flush_prompt: String,

    /// Whether compaction is enabled
    pub enabled: bool,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            reserve_tokens_floor: 20000,
            max_history_share: 0.5,
            soft_threshold_tokens: 4000,
            memory_flush_prompt: "Please store any important information from this conversation into memory. Reply with NO_REPLY if there's nothing to store.".to_string(),
            enabled: true,
        }
    }
}

pub struct ContextCompactor {
    config: CompactionConfig,
    context_window: usize,
    token_counter: Arc<TokenCounter>,
}

impl ContextCompactor {
    pub fn new(config: CompactionConfig, context_window: usize, token_counter: Arc<TokenCounter>) -> Self {
        Self { config, context_window, token_counter }
    }

    /// Calculate the threshold at which compaction should be triggered
    pub fn compaction_threshold(&self) -> usize {
        self.context_window
            .saturating_sub(self.config.reserve_tokens_floor)
            .saturating_sub(self.config.soft_threshold_tokens)
    }

    /// Check if compaction is needed
    pub fn should_compact(&self, current_tokens: usize) -> bool {
        if !self.config.enabled {
            return false;
        }
        current_tokens >= self.compaction_threshold()
    }

    /// Estimate token count for messages
    pub fn estimate_tokens(&self, messages: &[Message]) -> usize {
        messages.iter()
            .map(|msg| self.token_counter.count_message_tokens(msg))
            .sum()
    }

    /// Calculate max history tokens allowed
    pub fn max_history_tokens(&self) -> usize {
        (self.context_window as f64 * self.config.max_history_share) as usize
    }

    /// Compact and flush memory
    pub async fn compact_and_flush(
        &self,
        session_id: &str,
        messages: Vec<Message>,
        memory_store: Arc<dyn MemoryStore>,
    ) -> Result<Vec<Message>> {
        // 1. 检查是否需要记忆刷新
        let should_flush = self.should_compact(self.estimate_tokens(&messages));

        if !should_flush {
            return Ok(messages);
        }

        tracing::info!("Compaction triggered for session {}, current tokens: {}",
            session_id, self.estimate_tokens(&messages));

        // 2. 构建记忆刷新请求
        let flush_messages = vec![
            Message::system(&self.config.memory_flush_prompt),
            // 添加最近的对话上下文
        ];

        // 3. 调用 LLM 生成记忆摘要
        // memory_store.store(session_id, summary).await?;

        // 4. 压缩历史消息
        let max_history = self.max_history_tokens();
        let mut compacted = Vec::new();
        let mut current_tokens = 0;

        // 保留最近的 N 条消息
        for msg in messages.iter().rev() {
            let msg_tokens = self.token_counter.count_message_tokens(msg);
            if current_tokens + msg_tokens > max_history {
                break;
            }
            compacted.insert(0, msg.clone());
            current_tokens += msg_tokens;
        }

        // 在开头添加压缩摘要
        let summary = memory_store.get_latest_summary(session_id).await?;
        if let Some(summary) = summary {
            compacted.insert(0, Message::system(&format!(
                "[Previous conversation summary]\n{}",
                summary
            )));
        }

        tracing::info!("Compaction complete: {} messages -> {} messages, {} -> {} tokens",
            messages.len(), compacted.len(),
            self.estimate_tokens(&messages), self.estimate_tokens(&compacted));

        Ok(compacted)
    }
}
```

**前端改动**：

```tsx
// web/src/components/chat/MemoryFlushIndicator.tsx 新组件
import { Activity, Database, AlertTriangle } from "lucide-react"

interface MemoryFlushIndicatorProps {
  currentTokens: number
  threshold: number
  contextWindow: number
  isCompacting: boolean
}

export function MemoryFlushIndicator({
  currentTokens,
  threshold,
  contextWindow,
  isCompacting,
}: MemoryFlushIndicatorProps) {
  const percentage = (currentTokens / contextWindow) * 100
  const thresholdPercentage = (threshold / contextWindow) * 100

  const status = isCompacting ? 'compacting' :
                 percentage > thresholdPercentage ? 'warning' :
                 'normal'

  const statusConfig = {
    normal: { color: 'text-green-500', text: '上下文正常' },
    warning: { color: 'text-yellow-500', text: '接近上下文限制' },
    compacting: { color: 'text-blue-500', text: '正在压缩上下文...' },
  }

  const config = statusConfig[status]

  return (
    <div className="flex items-center gap-2 text-xs text-muted-foreground px-2 py-1">
      <Activity className={`h-3 w-3 ${config.color} ${isCompacting ? 'animate-pulse' : ''}`} />
      <span>上下文: {currentTokens}/{contextWindow} tokens</span>

      {/* 阈值标记 */}
      <div className="flex-1 h-1.5 bg-muted rounded-full overflow-hidden relative">
        <div
          className="h-full bg-blue-500 transition-all duration-300"
          style={{ width: `${percentage}%` }}
        />
        <div
          className="absolute top-0 h-full w-0.5 bg-yellow-500"
          style={{ left: `${thresholdPercentage}%` }}
        />
      </div>

      {status !== 'normal' && (
        <span className={`${config.color} flex items-center gap-1`}>
          {status === 'warning' && <AlertTriangle className="h-3 w-3" />}
          {status === 'compacting' && <Database className="h-3 w-3 animate-spin" />}
          {config.text}
        </span>
      )}
    </div>
  )
}
```

**影响文件清单**：
| 文件 | 改动类型 |
|------|----------|
| `crates/core/src/llm/compaction.rs` | 新文件 |
| `crates/core/src/lib.rs` | 导出新模块 |
| `crates/agent/src/agent/session.rs` | 集成压缩 |
| `crates/memory/src/lib.rs` | 记忆刷新触发 |
| `crates/api/src/handlers/sessions.rs` | 添加上下文信息 API |
| `web/src/components/chat/MemoryFlushIndicator.tsx` | 新文件 |
| `web/src/components/chat/ChatContainer.tsx` | 显示指标 |
| `web/src/lib/api.ts` | 新增 API 调用 |
| `config.toml` | 新增 compaction 配置节 |

**工作量**: 4-5 天

---

#### P1.2 Token 计数器

**目标**：准确估算消息 token 数量

**效果预期**：
- 准确预估何时触发压缩
- 避免因 token 估算错误导致的溢出
- 支持不同模型的 token 计算

**后端改动**：

```rust
// crates/core/src/llm/token_counter.rs 新增文件
use tiktoken_rs::tiktoken;
use crate::message::{Message, MessageRole};

pub struct TokenCounter {
    bpe: tiktoken_rs::CoreBPE,
}

impl TokenCounter {
    pub fn new(model: &str) -> Result<Self> {
        // 根据 model 选择合适的编码器
        let encoding = match model {
            m if m.starts_with("gpt-4") => "cl100k_base",
            m if m.starts_with("gpt-3.5") => "cl100k_base",
            m if m.contains("qwen") => "cl100k_base", // 近似
            _ => "cl100k_base", // 默认
        };

        let bpe = tiktoken(encoding)?;
        Ok(Self { bpe })
    }

    pub fn count_tokens(&self, text: &str) -> usize {
        self.bpe.encode_with_special_tokens(text).len()
    }

    pub fn count_message_tokens(&self, msg: &Message) -> usize {
        // 参考 OpenAI 的 token 计数规则
        // https://github.com/openai/openai-cookbook/blob/main/examples/How_to_count_tokens_with_tiktoken.ipynb

        let base = 4; // 每条消息的基础开销 (<im_start>{role/name}\n{content}<im_end>\n)

        // 角色标记
        let role = match msg.role() {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
        };
        let role_tokens = self.count_tokens(role);

        // 名称标记（如果有）
        let name_tokens = if let Some(name) = msg.name() {
            1 + self.count_tokens(name) // name 字段
        } else {
            0
        };

        // 内容 token
        let content_tokens = self.count_tokens(msg.content());

        // 每个字段后的分隔符
        let separators = 2;

        base + role_tokens + name_tokens + content_tokens + separators
    }

    pub fn count_messages_tokens(&self, messages: &[Message]) -> usize {
        // 消息总数
        let messages_len = messages.len();

        // 每条消息的开销
        let per_message: usize = messages.iter()
            .map(|msg| self.count_message_tokens(msg))
            .sum();

        // 回复的开销
        let reply = 3; // <im_start>assistant\n<im_end>\n

        messages_len + per_message + reply
    }

    /// 估算上下文窗口使用量
    pub fn estimate_context_usage(
        &self,
        messages: &[Message],
        system_prompt: &str,
        reserve: usize,
    ) -> (usize, usize) {
        let system_tokens = self.count_tokens(system_prompt);
        let messages_tokens = self.count_messages_tokens(messages);
        let total = system_tokens + messages_tokens + reserve;
        (total, messages_tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_simple_text() {
        let counter = TokenCounter::new("gpt-4").unwrap();
        let tokens = counter.count_tokens("Hello, world!");
        assert!(tokens > 0);
    }

    #[test]
    fn test_count_message() {
        let counter = TokenCounter::new("gpt-4").unwrap();
        let msg = Message::user("What is the capital of France?");
        let tokens = counter.count_message_tokens(&msg);
        assert!(tokens > 4); // 基础开销 + 内容
    }
}
```

**配置更新**：

```toml
# config.toml 新增配置
[llm]
# 使用的 token 计数器编码器
# 可选: cl100k_base (GPT-4/GPT-3.5), p50k_base (GPT-3), r50k_base (GPT-2)
token_encoding = "cl100k_base"

# 上下文窗口大小（根据模型调整）
context_window = 128000  # qwen3-vl:2b 约为 32K，GPT-4 为 128K
```

**影响文件清单**：
| 文件 | 改动类型 |
|------|----------|
| `crates/core/src/llm/token_counter.rs` | 新文件 |
| `crates/core/src/lib.rs` | 导出新模块 |
| `Cargo.toml` | 添加 `tiktoken-rs = "0.5"` 依赖 |
| `crates/agent/src/agent/session.rs` | 使用 TokenCounter |
| `config.toml` | 新增 token_encoding 配置 |

**工作量**: 2 天

---

### 🟡 P2 - 记忆系统改进

> **预计工期**: 2-3 周
> **预期效果**: 搜索精度提升 50%+，支持真实语义搜索

#### P2.1 真实嵌入模型支持

**目标**：替换 SimpleEmbedding 为真实嵌入

**效果预期**：
- 实现真正的语义搜索
- 搜索精度大幅提升
- 支持 Ollama 和 OpenAI 嵌入模型

**当前问题**：
```rust
// crates/memory/src/mid_term.rs 的 SimpleEmbedding 是假的！
pub fn embed(&self, text: &str) -> Vec<f32> {
    // 这只是 hash，不是真实的语义嵌入
    for (i, byte) in text.bytes().enumerate() {
        let pos = i % self.dim;
        embedding[pos] = embedding[pos] * 31.0 + (byte as f32) * 0.1;
    }
}
```

**后端改动**：

```rust
// crates/memory/src/embeddings.rs 新增文件
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// 嵌入模型 trait
#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError>;
    fn dimension(&self) -> usize;
}

#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("API error: {0}")]
    Api(String),
}

/// Ollama 嵌入模型
pub struct OllamaEmbedding {
    client: Client,
    model: String,
    endpoint: String,
    dimension: usize,
}

#[derive(Debug, Serialize)]
struct OllamaEmbedRequest<'a> {
    model: &'a str,
    input: &'a str,
}

#[derive(Debug, Deserialize)]
struct OllamaEmbedResponse {
    embedding: Vec<f32>,
}

impl OllamaEmbedding {
    pub fn new(model: &str, endpoint: &str) -> Self {
        Self {
            client: Client::new(),
            model: model.to_string(),
            endpoint: endpoint.to_string(),
            dimension: 768, // nomic-embed-text 默认维度
        }
    }

    pub fn with_dimension(mut self, dimension: usize) -> Self {
        self.dimension = dimension;
        self
    }
}

#[async_trait]
impl EmbeddingModel for OllamaEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let url = format!("{}/api/embed", self.endpoint);
        let req = OllamaEmbedRequest {
            model: &self.model,
            input: text,
        };

        let resp = self.client
            .post(&url)
            .json(&req)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(EmbeddingError::Api(format!("{}: {}", status, text)));
        }

        let data: OllamaEmbedResponse = resp.json().await?;
        Ok(data.embedding)
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        // Ollama 不支持原生批处理，顺序执行
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

/// OpenAI 嵌入模型
pub struct OpenAIEmbedding {
    client: Client,
    model: String,
    api_key: String,
}

#[derive(Debug, Serialize)]
struct OpenAIEmbedRequest<'a> {
    model: &'a str,
    input: Vec<&'a str>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbedResponse {
    data: Vec<OpenAIEmbedData>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbedData {
    embedding: Vec<f32>,
}

impl OpenAIEmbedding {
    pub fn new(model: &str, api_key: &str) -> Self {
        Self {
            client: Client::new(),
            model: model.to_string(),
            api_key: api_key.to_string(),
        }
    }
}

#[async_trait]
impl EmbeddingModel for OpenAIEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let results = self.embed_batch(&[text.to_string()]).await?;
        Ok(results.into_iter().next().unwrap())
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let url = "https://api.openai.com/v1/embeddings";
        let inputs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();

        let resp = self.client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&OpenAIEmbedRequest {
                model: &self.model,
                input: inputs,
            })
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(EmbeddingError::Api(format!("{}: {}", status, text)));
        }

        let data: OpenAIEmbedResponse = resp.json().await?;
        Ok(data.data.into_iter().map(|d| d.embedding).collect())
    }

    fn dimension(&self) -> usize {
        match self.model.as_str() {
            "text-embedding-3-small" => 1536,
            "text-embedding-3-large" => 3072,
            "text-embedding-ada-002" => 1536,
            _ => 1536,
        }
    }
}

/// 嵌入提供者配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingProviderConfig {
    pub provider: String,
    pub model: String,
    pub endpoint: Option<String>,
    pub api_key: Option<String>,
}

/// 创建嵌入模型实例
pub fn create_embedding_model(config: EmbeddingProviderConfig) -> Result<Box<dyn EmbeddingModel>> {
    match config.provider.as_str() {
        "ollama" => {
            let endpoint = config.endpoint.unwrap_or_else(|| "http://localhost:11434".to_string());
            Ok(Box::new(OllamaEmbedding::new(&config.model, &endpoint)))
        }
        "openai" => {
            let api_key = config.api_key.ok_or_else(|| {
                EmbeddingError::InvalidResponse("OpenAI API key is required".to_string())
            })?;
            Ok(Box::new(OpenAIEmbedding::new(&config.model, &api_key)))
        }
        _ => Err(EmbeddingError::InvalidResponse(format!("Unknown provider: {}", config.provider))),
    }
}
```

**更新 mid_term.rs**：

```rust
// crates/memory/src/mid_term.rs 修改
use super::embeddings::{EmbeddingModel, create_embedding_model, EmbeddingProviderConfig};

pub struct MidTermMemory {
    // 替换 SimpleEmbedding
    embedding: Box<dyn EmbeddingModel>,
    // ... 其他字段
}

impl MidTermMemory {
    pub fn new(config: &MemoryConfig) -> Result<Self> {
        let embedding_config = EmbeddingProviderConfig {
            provider: config.embedding_provider.clone(),
            model: config.embedding_model.clone(),
            endpoint: config.embedding_endpoint.clone(),
            api_key: config.embedding_api_key.clone(),
        };

        let embedding = create_embedding_model(embedding_config)?;

        Ok(Self {
            embedding,
            // ...
        })
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        // 使用真实嵌入进行搜索
        let query_embedding = self.embedding.embed(query).await?;

        // 计算余弦相似度
        let mut results: Vec<_> = self.entries.iter()
            .map(|entry| {
                let similarity = cosine_similarity(&query_embedding, &entry.embedding);
                (entry.clone(), similarity)
            })
            .filter(|(_, sim)| *sim > 0.5) // 相似度阈值
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        results.truncate(limit);

        Ok(results.into_iter().map(|(entry, _)| entry).collect())
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_a)
}
```

**配置更新**：

```toml
# config.toml 新增配置
[memory.embedding]
provider = "ollama"  # 或 "openai"
model = "nomic-embed-text"
endpoint = "http://localhost:11434"  # 可选
api_key = ""  # OpenAI 需要
```

**影响文件清单**：
| 文件 | 改动类型 |
|------|----------|
| `crates/memory/src/embeddings.rs` | 新文件 |
| `crates/memory/src/mid_term.rs` | 替换 SimpleEmbedding |
| `crates/memory/src/lib.rs` | 导出新模块 |
| `crates/memory/src/long_term.rs` | 使用真实嵌入 |
| `Cargo.toml` | 添加 `async-trait`, `thiserror` 依赖 |
| `config.toml` | 新增 memory.embedding 配置节 |

**工作量**: 3-4 天

---

#### P2.2 BM25 全文搜索

**目标**：添加关键词搜索能力

**效果预期**：
- 支持精确关键词搜索
- 与语义搜索互补
- 提升搜索召回率

**后端改动**：

```rust
// crates/memory/src/bm25.rs 新增文件
use tantivy::{
    schema::*,
    index::{Index, IndexWriter, IndexReader, SegmentReader},
    query::QueryParser,
    collector::TopDocs,
    DocAddress,
    Score,
};
use std::path::Path;
use serde::{Deserialize, Serialize};

/// BM25 搜索索引
pub struct BM25Index {
    index: Index,
    reader: IndexReader,
    schema: Schema,
}

/// 搜索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BM25Result {
    pub id: String,
    pub content: String,
    pub session_id: String,
    pub score: f32,
    pub timestamp: i64,
}

impl BM25Index {
    /// 创建新的 BM25 索引
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let schema = Schema::builder()
            .add_text_field("id", STRING | STORED)
            .add_text_field("content", TEXT | STORED)
            .add_text_field("session_id", STRING | STORED)
            .add_u64_field("timestamp", INDEXED | STORED)
            .build();

        let index = Index::create_in_dir(path, schema.clone())?;
        let reader = index.reader()?;

        Ok(Self {
            index,
            reader,
            schema,
        })
    }

    /// 获取 schema 字段
    fn fields(&self) -> SchemaFields {
        self.schema.fields()
    }

    /// 添加文档到索引
    pub fn add_document(
        &self,
        id: &str,
        content: &str,
        session_id: &str,
        timestamp: i64,
    ) -> Result<()> {
        let mut writer = self.index.writer(50_000_000)?;

        let id_field = self.schema.get_field("id").unwrap();
        let content_field = self.schema.get_field("content").unwrap();
        let session_id_field = self.schema.get_field("session_id").unwrap();
        let timestamp_field = self.schema.get_field("timestamp").unwrap();

        let doc = doc!(
            id_field => id,
            content_field => content,
            session_id_field => session_id,
            timestamp_field => timestamp
        );

        writer.add_document(doc)?;
        writer.commit()?;

        Ok(())
    }

    /// 搜索文档
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<BM25Result>> {
        let content_field = self.schema.get_field("content").unwrap();
        let id_field = self.schema.get_field("id").unwrap();
        let session_id_field = self.schema.get_field("session_id").unwrap();
        let timestamp_field = self.schema.get_field("timestamp").unwrap();

        let query_parser = QueryParser::for_index(&self.index, vec![content_field]);
        let query = query_parser.parse_query(query)?;

        let searcher = self.reader.searcher();
        let top_docs = searcher.search(&query, &TopDocs::with_limit(limit))?;

        let mut results = Vec::new();
        for (score, doc_address) in top_docs {
            let doc = searcher.doc(doc_address)?;

            results.push(BM25Result {
                id: doc.get_first(id_field).unwrap().to_string(),
                content: doc.get_first(content_field).unwrap().to_string(),
                session_id: doc.get_first(session_id_field).unwrap().to_string(),
                score: score as f32,
                timestamp: doc.get_first(timestamp_field).unwrap().into_u64().unwrap() as i64,
            });
        }

        Ok(results)
    }

    /// 删除文档
    pub fn delete_document(&self, id: &str) -> Result<()> {
        let mut writer = self.index.writer(50_000_000)?;
        let id_field = self.schema.get_field("id").unwrap();
        let term = Term::from_field_text(id_field, id);
        writer.delete_term(term)?;
        writer.commit()?;
        Ok(())
    }

    /// 更新文档
    pub fn update_document(
        &self,
        id: &str,
        content: &str,
        session_id: &str,
        timestamp: i64,
    ) -> Result<()> {
        self.delete_document(id)?;
        self.add_document(id, content, session_id, timestamp)?;
        Ok(())
    }
}

/// 将 BM25 排名转换为分数 (倒数排名)
pub fn bm25_rank_to_score(rank: usize) -> f32 {
    let normalized = if rank < 999 { rank } else { 999 };
    1.0 / (1.0 + normalized as f32)
}
```

**集成到记忆系统**：

```rust
// crates/memory/src/mid_term.rs 新增方法
impl MidTermMemory {
    pub async fn search_bm25(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        if let Some(ref bm25) = self.bm25_index {
            let results = bm25.search(query, limit)?;
            // 转换为 MemoryEntry
            Ok(results.into_iter().map(|r| MemoryEntry {
                id: r.id,
                content: r.content,
                timestamp: r.timestamp,
                // ...
            }).collect())
        } else {
            Ok(Vec::new())
        }
    }
}
```

**API 端点**：

```rust
// crates/api/src/handlers/memory.rs 新增
/// GET /api/memory/search?query=xxx&method=bm25
pub async fn search_memory(
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<MemoryEntry>>, AppError> {
    let results = match params.method.as_deref() {
        Some("bm25") => memory.search_bm25(&params.query, params.limit).await?,
        Some("vector") | None => memory.search(&params.query, params.limit).await?,
        Some(_) => return Err(AppError::BadRequest("Invalid search method".to_string())),
    };
    Ok(Json(results))
}
```

**影响文件清单**：
| 文件 | 改动类型 |
|------|----------|
| `crates/memory/src/bm25.rs` | 新文件 |
| `crates/memory/src/lib.rs` | 导出新模块 |
| `crates/memory/src/mid_term.rs` | 集成 BM25 搜索 |
| `crates/api/src/handlers/memory.rs` | 新增搜索 API |
| `Cargo.toml` | 添加 `tantivy = "0.22"` 依赖 |

**工作量**: 3 天

---

#### P2.3 混合搜索（向量 + BM25）

**目标**：融合语义和关键词搜索

**效果预期**：
- 结合语义和关键词搜索的优势
- 搜索精度提升 50%+
- 更好的用户体验

**后端改动**：

```rust
// crates/memory/src/hybrid.rs 新增文件
use std::collections::HashMap;
use super::bm25::BM25Result;
use super::embeddings::EmbeddingModel;

/// 混合搜索结果
#[derive(Debug, Clone)]
pub struct HybridResult {
    pub id: String,
    pub vector_score: f32,
    pub bm25_score: f32,
    pub combined: f32,
}

/// 混合搜索器
pub struct HybridSearcher {
    vector_weight: f32,
    bm25_weight: f32,
}

impl HybridSearcher {
    pub fn new(vector_weight: f32, bm25_weight: f32) -> Self {
        Self {
            vector_weight: vector_weight / (vector_weight + bm25_weight),
            bm25_weight: bm25_weight / (vector_weight + bm25_weight),
        }
    }

    /// 融合向量搜索和 BM25 搜索结果
    pub fn merge_results(
        &self,
        vector_results: Vec<(String, f32)>,  // (id, score)
        bm25_results: Vec<BM25Result>,
        limit: usize,
    ) -> Vec<HybridResult> {
        let mut fused: HashMap<String, HybridResult> = HashMap::new();

        // 处理向量搜索结果 (倒数排名转分数)
        for (i, (id, raw_score)) in vector_results.into_iter().enumerate() {
            let normalized_score = if raw_score <= 1.0 {
                raw_score
            } else {
                1.0 / (1.0 + i as f32)
            };

            fused.entry(id.clone()).or_insert_with(|| HybridResult {
                id: id.clone(),
                vector_score: 0.0,
                bm25_score: 0.0,
                combined: 0.0,
            }).vector_score = normalized_score;
        }

        // 处理 BM25 搜索结果
        for (i, result) in bm25_results.into_iter().enumerate() {
            let normalized_score = if result.score <= 1.0 {
                result.score
            } else {
                bm25_rank_to_score(i)
            };

            fused.entry(result.id.clone()).or_insert_with(|| HybridResult {
                id: result.id.clone(),
                vector_score: 0.0,
                bm25_score: 0.0,
                combined: 0.0,
            }).bm25_score = normalized_score;
        }

        // 计算融合分数
        for result in fused.values_mut() {
            result.combined =
                self.vector_weight * result.vector_score +
                self.bm25_weight * result.bm25_score;
        }

        // 排序并返回
        let mut results: Vec<_> = fused.into_values().collect();
        results.sort_by(|a, b| b.combined.partial_cmp(&a.combined).unwrap());
        results.truncate(limit);

        results
    }
}

/// 将 BM25 排名转换为分数
fn bm25_rank_to_score(rank: usize) -> f32 {
    let normalized = if rank < 999 { rank } else { 999 };
    1.0 / (1.0 + normalized as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_results() {
        let searcher = HybridSearcher::new(0.7, 0.3);

        let vector_results = vec![
            ("doc1".to_string(), 0.9),
            ("doc2".to_string(), 0.8),
            ("doc3".to_string(), 0.7),
        ];

        let bm25_results = vec![
            BM25Result {
                id: "doc2".to_string(),
                content: "test".to_string(),
                session_id: "s1".to_string(),
                score: 0.9,
                timestamp: 0,
            },
            BM25Result {
                id: "doc4".to_string(),
                content: "test2".to_string(),
                session_id: "s1".to_string(),
                score: 0.8,
                timestamp: 0,
            },
        ];

        let merged = searcher.merge_results(vector_results, bm25_results, 10);

        // doc2 应该排第一 (同时出现在两个搜索中)
        assert_eq!(merged[0].id, "doc2");
        // doc2 的 combined score 应该是融合后的值
        assert!(merged[0].combined > merged[0].vector_score);
        assert!(merged[0].combined > merged[0].bm25_score);
    }
}
```

**API 端点**：

```rust
// crates/api/src/handlers/memory.rs 更新
/// GET /api/memory/search?query=xxx&method=hybrid&vector_weight=0.7&bm25_weight=0.3
pub async fn search_memory_hybrid(
    Query(params): Query<HybridSearchParams>,
    State(memory): State<Arc<MidTermMemory>>,
) -> Result<Json<Vec<MemoryEntry>>, AppError> {
    let vector_weight = params.vector_weight.unwrap_or(0.7);
    let bm25_weight = params.bm25_weight.unwrap_or(0.3);

    let results = memory
        .search_hybrid(&params.query, params.limit, vector_weight, bm25_weight)
        .await?;

    Ok(Json(results))
}
```

**影响文件清单**：
| 文件 | 改动类型 |
|------|----------|
| `crates/memory/src/hybrid.rs` | 新文件 |
| `crates/memory/src/lib.rs` | 导出新模块 |
| `crates/memory/src/mid_term.rs` | 集成混合搜索 |
| `crates/api/src/handlers/memory.rs` | 新增混合搜索 API |

**工作量**: 2-3 天

---

#### P2.4 嵌入缓存

**目标**：避免重复计算嵌入

**效果预期**：
- 减少 API 调用
- 提升响应速度
- 降低成本（使用 OpenAI 时）

**后端改动**：

```rust
// crates/memory/src/cache.rs 新增文件
use lru::LruCache;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::sync::{Arc, Mutex};
use std::num::NonZeroUsize;

/// 嵌入缓存
pub struct EmbeddingCache {
    cache: Arc<Mutex<LruCache<u64, Vec<f32>>>>,
    max_size: usize,
}

impl EmbeddingCache {
    /// 创建新的嵌入缓存
    pub fn new(max_size: usize) -> Self {
        let capacity = NonZeroUsize::new(max_size).unwrap();
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(capacity))),
            max_size,
        }
    }

    /// 计算文本的哈希值
    fn hash_text(text: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }

    /// 获取缓存的嵌入
    pub fn get(&self, text: &str) -> Option<Vec<f32>> {
        let key = Self::hash_text(text);
        let mut cache = self.cache.lock().unwrap();
        cache.get(&key).cloned()
    }

    /// 存储嵌入到缓存
    pub fn put(&self, text: &str, embedding: Vec<f32>) {
        let key = Self::hash_text(text);
        let mut cache = self.cache.lock().unwrap();
        cache.put(key, embedding);
    }

    /// 批量获取
    pub fn get_batch(&self, texts: &[String]) -> Vec<Option<Vec<f32>>> {
        texts.iter()
            .map(|text| self.get(text))
            .collect()
    }

    /// 批量存储
    pub fn put_batch(&self, texts: &[String], embeddings: &[Vec<f32>]) {
        for (text, embedding) in texts.iter().zip(embeddings.iter()) {
            self.put(text, embedding.clone());
        }
    }

    /// 清空缓存
    pub fn clear(&self) {
        let mut cache = self.cache.lock().unwrap();
        cache.clear();
    }

    /// 获取缓存大小
    pub fn len(&self) -> usize {
        let cache = self.cache.lock().unwrap();
        cache.len()
    }

    /// 获取缓存容量
    pub fn capacity(&self) -> usize {
        self.max_size
    }
}

/// 带缓存的嵌入模型
pub struct CachedEmbeddingModel {
    inner: Box<dyn super::embeddings::EmbeddingModel>,
    cache: EmbeddingCache,
}

#[async_trait::async_trait]
impl super::embeddings::EmbeddingModel for CachedEmbeddingModel {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, super::embeddings::EmbeddingError> {
        // 尝试从缓存获取
        if let Some(cached) = self.cache.get(text) {
            return Ok(cached);
        }

        // 计算嵌入
        let embedding = self.inner.embed(text).await?;

        // 存入缓存
        self.cache.put(text, embedding.clone());

        Ok(embedding)
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, super::embeddings::EmbeddingError> {
        let mut results = Vec::with_capacity(texts.len());
        let mut uncached_indices = Vec::new();
        let mut uncached_texts = Vec::new();

        // 检查缓存
        for (i, text) in texts.iter().enumerate() {
            if let Some(cached) = self.cache.get(text) {
                results.push(Some(cached));
            } else {
                results.push(None);
                uncached_indices.push(i);
                uncached_texts.push(text.clone());
            }
        }

        // 批量计算未缓存的
        if !uncached_texts.is_empty() {
            let uncached_embeddings = self.inner.embed_batch(&uncached_texts).await?;

            for (idx, embedding) in uncached_indices.into_iter().zip(uncached_embeddings.into_iter()) {
                results[idx] = Some(embedding.clone());
                self.cache.put(&texts[idx], embedding);
            }
        }

        Ok(results.into_iter().map(|r| r.unwrap()).collect())
    }

    fn dimension(&self) -> usize {
        self.inner.dimension()
    }
}
```

**使用示例**：

```rust
// crates/memory/src/mid_term.rs
use super::cache::{EmbeddingCache, CachedEmbeddingModel};

impl MidTermMemory {
    pub fn new(config: &MemoryConfig) -> Result<Self> {
        let embedding = create_embedding_model(config.embedding_config)?;
        let cache = EmbeddingCache::new(config.cache_size);
        let cached_embedding = CachedEmbeddingModel::new(embedding, cache);

        Ok(Self {
            embedding: cached_embedding,
            // ...
        })
    }
}
```

**影响文件清单**：
| 文件 | 改动类型 |
|------|----------|
| `crates/memory/src/cache.rs` | 新文件 |
| `crates/memory/src/lib.rs` | 导出新模块 |
| `crates/memory/src/mid_term.rs` | 使用缓存嵌入 |
| `Cargo.toml` | 添加 `lru = "0.12"` 依赖 |
| `config.toml` | 新增 memory.cache_size 配置 |

**工作量**: 1 天

---

### 🔵 P3 - 可选增强（低优先级）

以下功能来自 Moltbot 的学习，但优先级较低，可在 P0-P2 完成后考虑：

#### P3.1 协议版本控制
- WebSocket 协议版本协商
- 向后兼容性支持

#### P3.2 序列号机制
- 消息顺序保证
- 间隙检测

#### P3.3 多粒度路由
- peer/guild/team/account/channel 级别路由

#### P3.4 Tool 执行钩子
- beforeToolExecute, afterToolExecute, onToolError

#### P3.5 心跳可见性控制
- 配置心跳事件发送频率

---

## 三、实施时间表

| 周次 | 任务 | 工期 | 优先级 |
|------|------|------|--------|
| **第1周** | P0.1 配置化思考限制和超时 | 2-3天 | 🔴 P0 |
| **第1-2周** | P0.2 分阶段超时警告机制 | 1-2天 | 🔴 P0 |
| **第2-3周** | P0.3 任务状态持久化与恢复 | 3-4天 | 🔴 P0 |
| **第4周** | P1.2 Token 计数器 | 2天 | 🟠 P1 |
| **第5-6周** | P1.1 上下文压缩策略 | 4-5天 | 🟠 P1 |
| **第7周** | P2.1 真实嵌入模型支持 | 3-4天 | 🟡 P2 |
| **第8周** | P2.2 BM25 全文搜索 | 3天 | 🟡 P2 |
| **第9周** | P2.3 混合搜索 | 2-3天 | 🟡 P2 |
| **第9周** | P2.4 嵌入缓存 | 1天 | 🟡 P2 |

**总计：约 8-9 周**

---

## 四、关键指标

### 修复前

| 指标 | 当前值 |
|------|--------|
| 平均超时率 | ~30% (复杂查询) |
| 中断后恢复率 | 0% |
| 思考内容完整保留 | 否 (>10K 字符丢失) |
| 上下文管理 | 无 |
| 语义搜索质量 | 低 (假嵌入) |
| 关键词搜索 | 无 |

### 修复后目标

| 指标 | 目标值 |
|------|--------|
| 平均超时率 | <5% |
| 中断后恢复率 | >80% |
| 思考内容完整保留 | 是 (最多 50K 字符) |
| 上下文自动压缩 | 是 |
| 语义搜索精度 | 提升 50%+ |
| 混合搜索 | 支持 |

---

## 五、风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 依赖库兼容性 | 中 | 充分测试，使用成熟版本 |
| 配置迁移 | 低 | 提供默认值，向后兼容 |
| 性能影响 | 低 | 缓存、批处理优化 |
| 前端复杂度 | 低 | 组件化，复用现有 UI |

---

## 六、依赖清单

### 新增 Rust 依赖

```toml
[dependencies]
# Token 计数
tiktoken-rs = "0.5"

# 全文搜索
tantivy = "0.22"

# LRU 缓存
lru = "0.12"

# 错误处理
thiserror = "1.0"
async-trait = "0.1"
```

### 新增前端依赖

```bash
npm install
# 无需新增，使用现有组件
```

---

## 七、变更日志

| 日期 | 版本 | 变更内容 |
|------|------|----------|
| 2026-01-30 | v0.1 | 初始计划文档 |
