# NeoMind 技术债务清理方案

> 版本: v0.5.0 路线图
> 创建时间: 2025-02-05
> 预计周期: 4-6 周

---

## 📋 总览

| 阶段 | 任务 | 优先级 | 预计工作量 | 风险 |
|------|------|--------|-----------|------|
| Phase 1 | 统一品牌名为 NeoMind | 🔴 高 | 3 天 | 低 |
| Phase 2 | 清理 deprecated 模块 | 🔴 高 | 2 天 | 低 |
| Phase 3 | 统一 Registry 模式 | 🟡 中 | 5 天 | 中 |
| Phase 4 | 拆分 ServerState | 🟡 中 | 7 天 | 中 |
| Phase 5 | 清理 Plugin vs Extension | 🟢 低 | 3 天 | 低 |

---

## Phase 1: 统一品牌名为 NeoMind 🔴

### 目标
将所有 `edge_ai-*` 和 `neomind*` 统一为 `neomind-*`

### 改动范围
```
1774+ 处引用需要修改
17 个 crate 需要重命名
```

### 执行步骤

#### 1.1 准备工作 (半天)
```bash
# 创建重命名映射表
cat > RENAME_MAP.md << 'EOF'
| 旧名称 | 新名称 |
|--------|--------|
| edge-ai-core | neomind-core |
| edge-ai-llm | neomind-llm |
| edge-ai-agent | neomind-agent |
| edge-ai-api | neomind-api |
| edge-ai-devices | neomind-devices |
| edge-ai-rules | neomind-rules |
| edge-ai-messages | neomind-messages |
| edge-ai-memory | neomind-memory |
| edge-ai-storage | neomind-storage |
| edge-ai-tools | neomind-tools |
| edge-ai-commands | neomind-commands |
| edge-ai-automation | neomind-automation |
| edge-ai-sandbox | neomind-sandbox |
| edge-ai-integrations | neomind-integrations |
| edge-ai-cli | neomind-cli |
| edge-ai-testing | neomind-testing |
| neomind-plugin-sdk | neomind-plugin-sdk |
EOF
```

#### 1.2 自动化重命名脚本 (1 天)

创建 `scripts/rename_crate.sh`:
```bash
#!/bin/bash
set -e

OLD_NAME=$1
NEW_NAME=$2

# 1. 重命名目录
mv "crates/$OLD_NAME" "crates/$NEW_NAME"

# 2. 更新 Cargo.toml
sed -i '' "s/name = \"$OLD_NAME\"/name = \"$NEW_NAME\"/g" "crates/$NEW_NAME/Cargo.toml"

# 3. 更新所有依赖引用
find . -type f -name "*.toml" -exec sed -i '' "s/edge-ai-$OLD_NAME/neomind-$NEW_NAME/g" {} \;
find . -type f -name "*.toml" -exec sed -i '' "s/neomind-$OLD_NAME/neomind-$NEW_NAME/g" {} \;

# 4. 更新 Rust 源码中的 use 语句
find . -type f -name "*.rs" -exec sed -i '' "s/use edge_ai::$OLD_NAME/use neomind::$NEW_NAME/g" {} \;
find . -type f -name "*.rs" -exec sed -i '' "s/edge_ai::$OLD_NAME/neomind::$NEW_NAME/g" {} \;

echo "Renamed $OLD_NAME -> $NEW_NAME"
```

#### 1.3 批量执行 (1 天)
```bash
# 按依赖顺序执行（无依赖的先执行）
./scripts/rename_crate.sh testing testing
./scripts/rename_crate.sh storage storage
./scripts/rename_crate.sh sandbox sandbox
./scripts/rename_crate.sh commands commands
./scripts/rename_crate.sh core core
./scripts/rename_crate.sh llm llm
./scripts/rename_crate.sh tools tools
./scripts/rename_crate.sh devices devices
./scripts/rename_crate.sh rules rules
./scripts/rename_crate.sh messages messages
./scripts/rename_crate.sh memory memory
./scripts/rename_crate.sh automation automation
./scripts/rename_crate.sh integrations integrations
./scripts/rename_crate.sh agent agent
./scripts/rename_crate.sh cli cli
./scripts/rename_crate.sh api api
./scripts/rename_crate.sh plugin-sdk plugin-sdk
```

#### 1.4 手动检查修正 (半天)
- 检查文档注释中的引用
- 检查 README.md
- 检查 web/ 目录中的 TypeScript 引用

#### 1.5 验证 (半天)
```bash
# 编译检查
cargo build --all-targets

# 测试检查
cargo test --all

# 克隆到新目录验证发布
cargo publish --dry-run
```

### 回滚方案
```bash
git checkout -b backup-before-rename
git add .
git commit -m "backup before rename"
```

---

## Phase 2: 清理 deprecated 模块 🔴

### 目标
移除已标记为 deprecated 但未删除的代码

### 需要清理的内容

#### 2.1 删除 core/alerts 模块

```bash
# 1. 确认没有代码引用
grep -r "use edge_ai_core::alerts" crates/
grep -r "use crate::alerts" crates/core/src/

# 2. 如果有引用，迁移到 edge_ai_messages
# 将 Alert -> Message, AlertSeverity -> MessageSeverity

# 3. 删除模块
rm crates/core/src/alerts/mod.rs

# 4. 更新 core/src/lib.rs (移除 alerts 导出)
```

#### 2.2 清理 core/src/lib.rs 中的 "Legacy exports"

```rust
// 删除这些
// Legacy exports (backward compatibility)
pub use llm::{GenerationResult, LlmBackend, LlmConfig, LlmError};
```

#### 2.3 清理 core/src/plugin/ (旧的 Plugin 系统)

```bash
# 如果确认 Extension 系统已完全替代
rm -rf crates/core/src/plugin/
```

### 迁移检查清单
- [ ] 所有 `Alert` 使用已迁移到 `Message`
- [ ] 所有 `AlertSeverity` 使用已迁移到 `MessageSeverity`
- [ ] 所有 `AlertChannel` 使用已迁移到 `MessageChannel`
- [ ] 所有旧的 `Plugin` 引用已迁移到 `Extension`

---

## Phase 3: 统一 Registry 模式 🟡

### 目标
明确各 Registry 的职责，统一对外接口

### 当前 Registry 状态

| Registry | 职责 | 是否对外 |
|----------|------|---------|
| `ExtensionRegistry` | 第三方扩展生命周期 | ✅ 是 |
| `LlmBackendRegistry` | LLM 后端管理 | ✅ 是 |
| `ToolRegistry` | 工具注册 | ✅ 是 |
| `DeviceRegistry` | 设备配置存储 | ✅ 是 |
| `WasmLlmPluginRegistry` | WASM LLM 插件 | ❌ 否（内部） |
| `UnifiedPluginRegistry` | 统一插件（旧） | ❌ 待删除 |

### 执行方案

#### 3.1 文档化职责边界

创建 `docs/architecture/registries.md`:
```markdown
# Registry 职责划分

## 用户可见的 Registry

### ExtensionRegistry (crate: core)
- **用途**: 第三方开发者加载扩展
- **类型**: .so/.dylib/.dll/.wasm
- **生命周期**: 发现 → 加载 → 启动 → 停止 → 卸载
- **API**: `/api/extensions/*`

### LlmBackendRegistry (crate: llm)
- **用途**: 用户配置 LLM 后端
- **类型**: 配置驱动的运行时
- **存储**: data/llm_backends.redb
- **API**: `/api/llm-backends/*`

### ToolRegistry (crate: tools)
- **用途**: Agent 可用的工具函数
- **类型**: 编译时注册 + 运行时动态添加
- **API**: `/api/tools/*`

### DeviceRegistry (crate: devices)
- **用途**: 设备配置和类型模板
- **类型**: 持久化存储
- **存储**: data/devices.redb
- **API**: `/api/devices/*`, `/api/device-types/*`

## 内部使用的 Registry

### WasmLlmPluginRegistry (crate: sandbox)
- **用途**: WASM LLM 插件执行
- **可见性**: 私有，由 LlmBackendRegistry 内部使用

## 已废弃

- ~~PluginRegistry~~: 已迁移到 ExtensionRegistry
- ~~UnifiedPluginRegistry~~: 已废弃
```

#### 3.2 添加 Registry trait 统一接口

创建 `crates/core/src/registry.rs`:
```rust
//! Common registry interface.

use async_trait::async_trait;

/// Common operations for all registries.
#[async_trait]
pub trait Registry: Send + Sync {
    type Item;
    type Id;

    /// Get an item by ID.
    async fn get(&self, id: &Self::Id) -> Option<Self::Item>;

    /// List all items.
    async fn list(&self) -> Vec<Self::Item>;

    /// Get the count of items.
    async fn count(&self) -> usize;

    /// Check if an item exists.
    async fn contains(&self, id: &Self::Id) -> bool;
}
```

#### 3.3 更新文档注释

为每个 Registry 添加清晰的职责说明。

---

## Phase 4: 拆分 ServerState 🟡

### 目标
将庞大的 ServerState 拆分为职责明确的子 State

### 当前问题
```rust
pub struct ServerState {
    // 25+ 字段，违反单一职责原则
}
```

### 执行方案

#### 4.1 创建子 State 模块

```
crates/api/src/server/state/
├── mod.rs
├── auth_state.rs          # 认证相关
├── device_state.rs        # 设备相关
├── automation_state.rs    # 自动化相关
├── agent_state.rs         # Agent 相关
├── storage_state.rs       # 存储相关
└── core_state.rs          # 核心服务 (EventBus, SessionManager)
```

#### 4.2 定义子 State 结构

```rust
// crates/api/src/server/state/auth_state.rs
#[derive(Clone)]
pub struct AuthState {
    pub auth_state: Arc<AuthState>,
    pub auth_user_state: Arc<AuthUserState>,
}

// crates/api/src/server/state/device_state.rs
#[derive(Clone)]
pub struct DeviceState {
    pub registry: Arc<DeviceRegistry>,
    pub service: Arc<DeviceService>,
    pub telemetry: Arc<TimeSeriesStorage>,
    pub embedded_broker: Option<Arc<EmbeddedBroker>>,
    pub update_tx: broadcast::Sender<DeviceStatusUpdate>,
}

// crates/api/src/server/state/automation_state.rs
#[derive(Clone)]
pub struct AutomationState {
    pub rule_engine: Arc<RuleEngine>,
    pub rule_store: Option<Arc<RuleStore>>,
    pub automation_store: Option<Arc<SharedAutomationStore>>,
    pub intent_analyzer: Option<Arc<IntentAnalyzer>>,
    pub transform_engine: Option<Arc<TransformEngine>>,
}

// crates/api/src/server/state/agent_state.rs
#[derive(Clone)]
pub struct AgentState {
    pub session_manager: Arc<SessionManager>,
    pub memory: Arc<RwLock<TieredMemory>>,
    pub agent_store: Arc<AgentStore>,
    pub agent_manager: Arc<RwLock<Option<AgentManager>>>,
}

// crates/api/src/server/state/core_state.rs
#[derive(Clone)]
pub struct CoreState {
    pub event_bus: Arc<EventBus>,
    pub command_manager: Arc<CommandManager>,
    pub message_manager: Arc<MessageManager>,
    pub extension_registry: Arc<RwLock<ExtensionRegistry>>,
}
```

#### 4.3 重构 ServerState

```rust
// crates/api/src/server/types.rs
#[derive(Clone)]
pub struct ServerState {
    /// 子状态
    pub auth: AuthState,
    pub devices: DeviceState,
    pub automation: AutomationState,
    pub agents: AgentState,
    pub core: CoreState,

    /// 跨切面的服务
    pub response_cache: Arc<ResponseCache>,
    pub rate_limiter: Arc<RateLimiter>,
    pub started_at: i64,

    /// 内部标志
    agent_events_initialized: Arc<AtomicBool>,
    rule_engine_events_initialized: Arc<AtomicBool>,
    rule_engine_event_service: Arc<Mutex<Option<RuleEngineEventService>>>,
}
```

#### 4.4 更新 Handler 提取方式

```rust
// 之前
State(state): State<ServerState>

// 之后
State(state): State<ServerState>
let devices = &state.devices;
let agents = &state.agents;
```

#### 4.5 渐进式迁移计划

1. 创建新 State 结构（不影响现有代码）
2. 实现兼容层（ServerState 仍可用）
3. 逐个 Handler 迁移到新 State
4. 全部迁移后移除旧字段

---

## Phase 5: 清理 Plugin vs Extension 混乱 🟢

### 目标
明确 Plugin 和 Extension 的语义差异，删除混淆代码

### 概念澄清

| 术语 | 定义 | 使用场景 |
|------|------|---------|
| **Extension** | 动态加载的代码模块 (.so/.wasm) | 第三方扩展 |
| **Plugin** | 编译时注册的功能模块 | 内置功能 |

### 执行方案

#### 5.1 重命名 plugin-sdk

```bash
# crate 重命名
mv crates/plugin-sdk crates/extension-sdk

# 更新名称
sed -i '' 's/neomind-plugin-sdk/neomind-extension-sdk/g' Cargo.toml
```

#### 5.2 更新文档和注释

```markdown
## 插件 vs 扩展

### 内置插件 (Built-in Plugins)
- 编译时链接到主程序
- 使用 `neomind::plugins` 模块
- 示例: LLM backends, Tools

### 动态扩展 (Dynamic Extensions)
- 运行时加载
- 使用 `neomind::extension` 模块
- 支持格式: .so/.dylib/.dll/.wasm
- API: `/api/extensions/*`
```

#### 5.3 清理 plugin 模块中的冗余代码

删除 `crates/core/src/plugin/` 中与 Extension 重复的部分。

---

## 🚀 执行时间表

| 周次 | 任务 | 里程碑 |
|------|------|--------|
| Week 1 | Phase 1-2 | 品牌统一, 清理 deprecated |
| Week 2 | Phase 3 | 统一 Registry 模式 |
| Week 3 | Phase 4.1-4.3 | 拆分 ServerState (结构) |
| Week 4 | Phase 4.4-4.5 | 拆分 ServerState (迁移) |
| Week 5 | Phase 5 | 清理 Plugin 混乱 |
| Week 6 | 测试和文档 | 发布 v0.5.0 |

---

## ✅ 验收标准

### Phase 1
- [ ] 所有 crate 名称统一为 `neomind-*`
- [ ] 代码中无 `edge_ai` 或 `neomind` 引用
- [ ] `cargo build --all-targets` 通过
- [ ] `cargo test --all` 通过

### Phase 2
- [ ] 无 deprecated 警告
- [ ] `core/alerts` 模块已删除
- [ ] 旧的 `plugin` 模块已删除

### Phase 3
- [ ] 所有 Registry 有清晰的文档
- [ ] Registry trait 统一接口已实现

### Phase 4
- [ ] ServerState 字段 < 15 个
- [ ] 所有 Handler 使用子 State

### Phase 5
- [ ] `plugin-sdk` 重命名为 `extension-sdk`
- [ ] 概念文档已更新

---

## 🔄 回滚策略

每个 Phase 完成后：
1. 创建 git tag: `v0.4.x-phaseN-complete`
2. 如果下个 Phase 出问题，可快速回滚

---

## 📝 注意事项

1. **向后兼容**: API 路径保持不变 (`/api/llm-backends` 等)
2. **数据迁移**: redb 数据文件格式不变
3. **配置文件**: 支持 `edge_ai` 到 `neomind` 的别名过渡
4. **发布**: 最后一并发布到 crates.io

---

## 🎯 最终目标

完成后，NeoMind v0.5.0 将拥有：
- ✅ 统一的品牌形象
- ✅ 清晰的模块职责
- ✅ 一致的命名约定
- ✅ 可维护的代码结构
- ✅ 完善的架构文档
