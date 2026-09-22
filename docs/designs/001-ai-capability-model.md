# NeoMind 智能能力概念重构 — 详细设计（提案）

| | |
|---|---|
| **状态** | 待评审（与 002 同场） |
| **定位** | **技术落地设计（附录）**。产品价值、业务对象、交互与使用逻辑以 [002](./002-product-and-interaction-design.md) 为准；本文回答"怎么实现"，并在 §5/§7 被 002 §7 的增补指令修订 |
| **范围** | ai_agent 概念/业务模型重构 + 智能算子新能力 + 多源触发 + kernel 统一衔接 |
| **不涉及** | `.nep` ABI、设备类型 schema、仪表盘组件 schema（无跨仓库破坏性变更） |
| **证据基准** | main@07f59b09，行号以该提交为准 |

---

## 0. 评审导航（先读这页）

**本文要你拍板的是第 8 节的 7 个决定。** 其余章节是论据：§1 问题、§2 产品定位、§4 四种思路对比、§5 推荐方案详细设计、§7 分期实施。

一句话版本：

> 把现在"ai_agent 一个概念包打天下"拆成**会算的（智能算子）、会跑的（任务）、会想的（智能体）**三层；算子作为数据面概念落地（复用 transform 骨架 + agent kernel 的单次推理通路），任务/智能体沿用现有 agent 存储只做语义重组与多源触发增强；不新建工作流引擎，用 DataSourceId 把三者组合起来。

---

## 1. 背景与问题

### 1.1 业务场景（设计目标）

客户要讲的故事四则：

- **S1 图片→理解→存数据**：摄像头出图，理解内容，把有效信息（区域人数、仪表读数、整洁度评分）变成**平台可用的数据**。
- **S2 图片/数据→理解→执行**：理解结果满足语义条件时执行动作（告警、下发命令）。
- **S3 多源触发→复杂多步骤任务**：多种数据源/触发源组合，多步骤分析处置。
- **S4 定时巡检**：周期性查看、对比历史、出报告。

### 1.2 现状概念模型的结构问题

现在 `AiAgent`（`crates/neomind-storage/src/agents.rs:34`）把三个正交维度绑死在一个概念里：

1. **I/O 契约**：输入=绑定的 resources，输出=自由文本+journal，下游（规则/仪表盘/data-push）无法消费；
2. **运行策略**：`AgentSchedule`（:201）四种触发 + 5 分钟超时 + 退避重试；
3. **自主性**：`ExecutionMode::Focused|Free`（:254）只有 L1/L2 两档。

绑死的直接代价（均为代码事实，可核）：

| # | 代价 | 证据 |
|---|------|------|
| C1 | **结构化抽取付全价**：一次执行要走 collect→intent→analyze 前置 LLM 环节才进正题，抽取类需求（S1）每条数据多付两次 LLM 调用 | `ai_agent/executor/mod.rs:1208-1639`（execute_internal 主管道） |
| C2 | **失败语义错配**：抽取失败是数据质量问题（应保旧值/标 stale/熔断降频），现在却走 agent 的 Error 状态+退避重试 | CLAUDE.md Agent 节 + scheduler.rs 重试逻辑 |
| C3 | **输出不可组合**：agent 产出只有 journal 文本，没有结构化字段汇入 DataSourceId 体系 | executor 输出路径全查无发布 |
| C4 | **触发模型太弱**：`event_filter` 只是 `Option<String>`，单源匹配；无 AND/OR、无时间窗聚合（S3 的核心缺口） | `agents.rs:209`；CLAUDE.md"Free 模式无 filter 永不触发" |
| C5 | **巡检无趋势**：journal 只存条目，跨执行学习提取已被禁用，报告没有"比上周"叙事 | executor/mod.rs:1438、:1592 注释自认 |
| C6 | **双栈维护**：chat 与 ai_agent 各一套 runtime 工厂/缓存/限流/提示词构建，感知升级两边改 | `llm_runtime.rs:115-278` vs `instance_manager.rs:220`；`tool_prompt.rs:35-49` 引用已不存在的 system_prompt.md |

### 1.3 现有的地基（比想象中好）

设计不是从零开始，三块地基已存在：

- **数据面骨架**：`TransformAutomation`（`neomind-api/src/automation/types.rs:648`）已有 scope（Global/DeviceType/Device）+ `intent`（自然语言意图）+ AI 生成代码 + `output_prefix` 输出到数据源的完整链路；JS 执行器支持预执行 `extensions.invoke()` 调 YOLO 等扩展（`js_executor.rs:42-63`）；图片输入解析已有（`transform/image.rs` resolve_image_data）。
- **事件总线**：EventBus 是脊柱（ADR-0001），规则引擎 v2 已证明"组合原语、不造引擎"路线（ADR-0009）。
- **规则升级通道**：rule v2 的 `TriggerAgent` 动作已是"确定性层→智能层"的升级通道雏形。

**缺的是**：数据面上的"运行时模型推理"这一种变换（transform 现在只跑确定性代码）、多源触发、agent 结构化输出、以及 L0 的轻量执行通路。

---

## 2. 产品定位分析

### 2.1 定位约束

NeoMind 是**边缘部署**的 AIoT 平台（ADR-0003 单机假设）。三条硬约束：

1. **算力约束**：不能对每帧图片跑多轮 LLM 循环；VLM 只在小模型（如 MiniCPM-V 级别）可用的场合承担开放语义理解。
2. **成本约束**：LLM 调用（本地推理时间或云 API 费用）必须按价值分级——高频廉价路径给确定性变换，低频高价路径给智能体。
3. **可审计约束**：无人值守的智能行为必须可追溯（journal/执行记录），数据面的产出必须可观测（数据质量视图）。

### 2.2 能力阶梯（对客户的心智模型）

| 层 | 概念 | 自主性 | 每次成本 | 失败直觉 | 现实场景 |
|----|------|--------|----------|----------|----------|
| 数据面 | **智能算子** | L0 单次调用 | ≈0~低 | "数据怎么旧了/不准了" | S1 |
| 控制面 | **规则 + 任务** | 确定性 | 0 | "规则没触发，查条件" | S2、S4 编排 |
| 智能面 | **智能体** | L2/L3 工具循环+记忆 | 高 | "它做错了决定" | S3、S4 报告 |
| 交互面 | **chat** | 人在回路 | 人在场才计 | "答得不好，追问" | 临时问询 |

**判断某个需求落在哪层的试金石：这个 LLM 调用失败时，用户的第一反应是什么？** 三种失败直觉对应三套产品语义（数据质量 / 作业运维 / 决策审计），这是概念边界最可靠的划法，也是本设计的根基。

---

## 3. 目标概念模型

三根轴解耦（详见对话沉淀，此处只列结论）：

- **I/O 契约**：typed 输入源 → 输出三选一（结构化字段 / 人类文本 / 动作）；
- **运行契约**：触发（输入变化/定时/事件/手动）+ 频率 + 预算；
- **自主性**：L0 单次调用 → L1 固定流程 → L2 工具循环 → L3 跨执行记忆。

三个客户可见概念：

```
智能算子 (AI Operator)   "把非结构化数据变成结构化数据"      → 发布 ai:<id>:<field> 数据源
自动化任务 (Job)         "什么时候、看什么、做什么、给谁"     = 现有 agent 的产品话术重组（不新建存储）
智能体 (Agent)           "需要它自己决定怎么做"              = Free 模式 + journal 记忆
```

组合关系：任务可以包算子或触发智能体；算子与智能体的结构化输出都发布为数据源；规则消费任意数据源并可 TriggerAgent。**不新建工作流引擎**——规则做确定性的边，agent/算子做智能的节点，DataSourceId 是边上流动的载荷。

---

## 4. 四种思路对比（多种思路 × 产品定位匹配）

### 思路 A：算子 = Transform 的一般化（数据面扩展）

在 `automation` 体系里给变换加一种新 kind：**模型推理**。`OperatorConfig` 与 `TransformAutomation` 共用 scope/pipeline/输出发布骨架，只是执行体从"跑 JS"换成"单次 LLM/VLM 调用 + schema 解析"。

- **匹配定位**：边缘效率优先、"一切皆数据源"的平台叙事。S1 是主打场景。
- 优点：
  - 数据面基础设施全部白拿：scope 挂载、输出前缀、数据源 API（`/api/automations/transforms/data-sources` 一族）、仪表盘绑定；
  - L0 通路天然独立于 agent 执行器，不用碰 68k 行的 neomind-agent 编排；
  - 失败语义放对了地方（保旧值/stale/熔断属于数据面词汇）。
- 缺点：
  - transform 目前是"设备数据到达才跑"的挂载模型，触发模型要扩（interval/变更防抖/手动）；
  - LLM 调用能力在 neomind-agent crate，automation 在 neomind-api crate，需要 agent 侧暴露一个薄的单次推理接口（依赖方向：api → agent，现状已如此）；
  - "transform=代码、operator=模型"两个概念共处一页，UI 命名要处理好。

### 思路 B：算子 = ai_agent 的新执行模式（控制面扩展，最快上市）

给 `ExecutionMode` 加 `Structured`（或加 `autonomy_level` 字段），executor 里走跳过 intent/analyze 的轻通路直奔单次调用。

- **匹配定位**："一切皆 agent"的叙事；上市最快。
- 优点：改动集中在一个 crate；调度/事件/退避/执行记录全复用；UI 加一种类型即可。
- 缺点：概念继续绑死（C2 失败语义错配不解决反而固化）；数据面观测要硬塞进 agent 体系；高频场景背 5 分钟超时/退避/journal 语义；neomind-agent 继续膨胀，与"拆解伪模块"的重构方向相反。

### 思路 C：独立智能服务层（长期架构最优）

新建 crate（如 `neomind-inference`）统一算子引擎 + 触发 + 输出发布，transform 与 agent 都调用它。

- **匹配定位**：平台化叙事，若预期算子将来要独立伸缩/独立部署（多机、云边协同）则值得。
- 优点：概念最干净，kernel 落点自然。
- 缺点：工作量最大；与现有 transform 骨架重复建设；MVP 阶段为未来付费；迁移面大。

### 思路 D：纯产品层重组（baseline，最小方案）

后端不动，只改 UI 话术与创建向导：把 Focused agent 包装成"数据提取"，Free 包装成"智能体"。

- **匹配定位**：只想先验证市场话术。
- 优点：一周工作量。
- 缺点：C1（每次执行多两次 LLM 调用）、C3（输出不可组合）、C4（单源触发）一个都没解决——话术与实现脱节会被客户试用戳穿。

### 对比矩阵

| 维度 | A 数据面 | B agent模式 | C 独立层 | D 纯UI |
|------|:---:|:---:|:---:|:---:|
| S1 抽取的每次成本 | **优**（无前置环节） | 差（C1 仍在） | 优 | 差 |
| S1 数据质量语义（保旧值/stale/熔断） | **优**（数据面原生） | 差 | 优 | 差 |
| S3 多源触发 | 中（M2 补） | 中（在 agent 侧补） | 优 | 无 |
| 组合性（输出→数据源→规则/仪表盘） | **优** | 中（要给 agent 加输出契约） | 优 | 无 |
| 上市速度 | 中 | **优** | 差 | **优** |
| 与重构方向（拆解 agent crate）的协同 | **协同** | 冲突 | 协同 | 中性 |
| 工作量 | 中 | 小 | 大 | 极小 |
| 对存量用户的破坏 | 无 | 无 | 小 | 无 |

### 推荐：**A 为主体，B 为桥梁，分两步走**

> **2026-09-22 评审修订：算子改落 ai_agent 域（思路 B 的家 + 思路 A 的语义）。** 产品层已决定"算子是智能体的一种类型、一个列表"（002），独立 automation 实体与该决定矛盾；M0-2 的 kernel 统一也消解了当年否决 B 的双栈理由。保留 A 的核心设计：L0 轻通路（无 intent/analyze 前置）、schema 校验+单次重试、预算/熔断字段、`ai:{agent_id}:{field}` 发布（executor 直写 telemetry + DeviceMetric 事件）、dry-run。存储为 `ExecutionMode` 尾加 `Structured` 变体 + `AiAgent` 尾部追加字段（bincode 兼容）；CRUD/编辑器/详情/调度全部复用 `/api/agents` 与现有 scheduler。

1. 算子落数据面（思路 A）——概念放对地方，边缘成本模型成立，且与 transform 的 AI 原生化方向连续（`intent` 字段已经存在，算子是它的自然延伸）；
2. 现有 agent 不改名不改存储，产品话术重组为"任务/智能体"（吸收思路 B 的上市速度，但不加新执行模式——Focused 的归宿是引导迁移到算子，见 §6.2）；
3. 思路 C 的"独立 crate"保留为演进选项：若 M1 后发现 automation 模块装不下（多模型并发、队列、优先级），再把算子引擎抽成 crate，接口按本设计的 `InferenceClient` 边界切，届时迁移成本可控。

**否决 D**：话术与实现脱节；**否决 B 作为主方案**：把最重要的 S1 场景钉死在错误的语义上。

---

## 5. 推荐方案详细设计

### 5.1 智能算子（AI Operator）

#### 5.1.1 数据模型

新增存储于 `automations.redb`（与 transform 同域，不新建库文件）：

```rust
/// crates/neomind-api/src/automation/types.rs（新增）
pub struct OperatorAutomation {
    #[serde(flatten)]
    pub metadata: AutomationMetadata,          // 复用 id/name/enabled
    pub scope: TransformScope,                 // 复用：Global | DeviceType | Device

    // ---- 输入 ----
    /// 输入数据源（typed），如 ["device:cam-01:image", "extension:yolo:count"]
    pub inputs: Vec<String>,
    /// 可选：输入样本（few-shot / 输出校准用）
    pub input_example: Option<serde_json::Value>,

    // ---- 模型与指令 ----
    /// LLM backend id（空 = 平台默认；能力探测复用 ADR-0014 链）
    pub llm_backend_id: Option<String>,
    /// 自然语言指令模板，支持 {{input}} / {{input.device:cam-01:image}} 插值
    pub prompt_template: String,
    /// 输出契约：字段 → 类型（number|string|boolean|enum(v1..vn)|json）
    pub output_schema: Vec<OperatorField>,

    // ---- 输出 ----
    /// 数据源前缀，输出发布为 ai:<prefix>:<field>
    pub output_prefix: String,

    // ---- 运行契约 ----
    pub trigger: OperatorTrigger,              // 见 5.1.3
    pub budget: OperatorBudget,                // 见 5.1.5

    // ---- 输出平滑 ----
    /// 状态枚举类输出的防抖动策略：VLM 逐帧判定会抖（整洁→混乱→整洁），
    /// 盯守类必须配置，否则判定链下游被噪声轰炸
    pub smoothing: Option<SmoothingPolicy>,
}

pub enum SmoothingPolicy {
    /// 连续 n 次判定一致才变更发布的状态（漏报敏感型适用）
    ConsecutiveConfirmations { n: u8 },
    /// 时间窗内多数表决（误报敏感型适用）
    WindowMajority { window_secs: u32 },
}

pub struct OperatorField {
    pub name: String,
    pub field_type: OperatorFieldType,
    pub unit: Option<String>,        // 仪表盘展示用
    pub description: Option<String>, // 写进 prompt 的字段语义
}
```

**DataSourceId 约定**：类型段用 `ai`，即 `ai:<prefix>:<field>`；仪表盘绑定沿用 transform 的点号规则（`extensionMetric`/`aiMetric: "<prefix>.<field>"`），与 CLAUDE.md DataSourceId 节的双规则保持一致。选 `ai` 而非 `operator` 作为类型段：短、语义面向客户，且为将来 agent 结构化输出（§5.2.3）预留同一命名空间。

#### 5.1.2 执行通路（L0 轻通路）

```
触发 → 采集输入 → 组装 prompt → 单次推理 → schema 校验(修复一次) → 发布
```

| 步骤 | 实现 | 来源 |
|------|------|------|
| 采集输入 | 从 telemetry.redb / 扩展注册表拉各 DataSourceId 最新值；图片输入经 `image_utils` 解析（复用单源事实，CLAUDE.md canonical 表） | image_utils.rs；transform/image.rs 的 URL/base64 解析逻辑合并进来 |
| 组装 prompt | 模板插值 + output_schema 的字段说明（name/type/description 自动展开为 JSON 输出格式要求） | 新代码（automation/transform/operator_prompt.rs） |
| 单次推理 | **复用 agent kernel 的 runtime 工厂**（M0 重构产物，见 5.3），走 `InferenceClient` 薄接口；thinking 模型强制 `thinking_enabled: false`（gotcha #7） | llm_runtime.rs 瘦身后的工厂 |
| schema 校验 | 类型检查 + 枚举校验；失败则把错误信息附上重试**一次**；再失败即本次失败 | 新代码（operator/parse.rs） |
| 发布 | 每字段写 telemetry（`ai:<prefix>:<field>`）+ EventBus 发布，走 transform 现有的输出发布管线 | pipeline.rs 扩展 |

**明确不做**：L0 通路无工具调用、无多轮、无 journal、无意图/态势前置环节。这是安全边界也是成本边界——算子输出只进数据面，不直接执行动作（要执行就由规则消费其输出，S2 由 rule + 算子组合实现，见 5.5 走查）。

#### 5.1.3 触发模型

```rust
pub enum OperatorTrigger {
    /// 输入变化防抖：任一输入新值到达后 debounce_secs 内合并（默认 5s）
    OnInputChange { debounce_secs: u64 },
    /// 固定周期
    Interval { seconds: u64 },
    /// 手动/外部调用（API、规则动作、chat 工具）
    Manual,
}
```

MVP 实现前两种 + 手动；事件触发（规则动作 `RunOperator`）进 M2。**防抖是成本护栏的核心**：摄像头类输入 1fps 时，OnInputChange(30s) 意味着每 30 秒最多一次推理。

**摄像头输入的三档采样策略**（"VLM 替代传统 CV"场景的成本骨架，002 §2.1 选位表的实现侧）：

| 档位 | 机制 | 适用 |
|------|------|------|
| 周期采样 | Interval 每 N 秒取一帧（典型 5-30s） | 通用巡检/抄表 |
| 变化检测采样 | 本地廉价画面差分，超阈值才送 VLM（差分不花推理） | 场景多数时间静止 |
| CV 预筛门控 | 传统 CV 扩展指标作门（如 `extension:yolo:person_count > 0` 才跑 VLM） | **级联模式**：CV 出候选，VLM 出语义判定 |

配套的 `SmoothingPolicy`（见 5.1.1）作用于发布环节：采样决定"何时看"，平滑决定"何时才算变了"——两者共同保证进入规则/通知的状态序列是干净的。

#### 5.1.4 状态机（数据质量语义，非 agent 语义）

```
Healthy ──连续 N 次失败──▶ Degraded（熔断：停止推理，保旧值，标 stale）
Degraded ──冷却期到（默认 5min）──▶ 探针一次──▶ Healthy / 维持 Degraded
任意 ──用户操作──▶ Paused
```

- 旧值保留策略：输出字段维持最后一次成功值，随 `stale_ttl_secs`（默认 300s）标记过期，过期后仪表盘显示"过期"而非假新鲜（复用设备 offline_timeout 的三态思路）。
- 这套状态机是**算子专属**的，与 `AgentStatus` 无关——这就是 C2 修复的本体。

#### 5.1.5 预算与成本控制

```rust
pub struct OperatorBudget {
    pub max_calls_per_day: Option<u32>,     // 超额自动进 Paused，次日恢复
    pub max_output_tokens: u32,             // 默认 512，schema 解析用不了更多
    pub timeout_secs: u32,                  // 默认 60（对比 agent 的 300s）
    pub consecutive_failure_threshold: u8,  // 默认 3 → 熔断
}
```

观测指标：成功率、P95 时延、今日调用数、当前状态——进数据质量视图（前端"自动化"页的算子 tab），不进 agent 执行历史。

#### 5.1.6 API

```
GET/POST        /api/ai-operators                 # 列表/创建
GET/PUT/DELETE  /api/ai-operators/:id
POST            /api/ai-operators/:id/run         # 手动触发（同步返回结果，供测试）
POST            /api/ai-operators/test            # dry-run：不入库不发布，返回解析结果（向导预览用）
GET             /api/ai-operators/:id/executions  # 最近执行（含失败原因，滚动保留 100 条）
GET             /api/automations/data-sources     # 扩展现有数据源列表 API，纳入 ai:* 条目
```

dry-run 是向导体验的关键：用户写完 prompt 立即用真实输入试跑，看到结构化输出再保存——与 transform 已有的 `/test`、`/test-code` 体验对齐。

#### 5.1.7 前端 UX（概要，实施前读 DESIGN_SPEC.md）

"自动化"页新增**算子** tab（与 transform 并列）。三步向导（UnifiedFormDialog / FullScreenDialog 按表单复杂度选）：

1. 选输入：数据源选择器（现有组件）+ 防抖/周期；
2. 写指令：prompt 编辑 + **由 AI 从指令预生成 output_schema**（复用现有 intent 链）+ dry-run 实时预览；
3. 确认输出：字段名/类型/单位表单，保存后字段自动出现在仪表盘与规则的数据源选择器里。

i18n 双语；设计令牌照旧。

### 5.2 任务与智能体（现有 agent 的语义重组 + 增强）

#### 5.2.1 不新建存储、不改 `/api/agents`

"任务"是现有 agent 的产品话术（向导里按模板呈现：巡检任务/事件任务/单次任务），存储与 API 完全不动——存量零破坏。

#### 5.2.2 多源触发（EventFilter 结构化）

`event_filter: Option<String>` 升级为多态（先例：`.nep` 的 `FrontendField` 字符串/结构体双态，CLAUDE.md 有案）：

```rust
pub struct EventFilter {
    /// 任一来源命中即触发（OR）
    pub any: Vec<ResourceId>,
    /// 时间窗内全部命中才触发（AND + 时间窗聚合）
    pub all: Vec<ResourceId>,
    /// all 的聚合窗口；None = 无窗口（需同时刻同事件）
    pub within_secs: Option<u64>,
}
```

- serde 兼容：旧字符串按"绑定资源集合"语义反序列化为 `any = 绑定资源`，行为不变；
- `all` 语义：窗口内**首个**命中事件开启计时，全部来源命中即触发并清窗，超时清窗；
- 现有 60s per-source dedup（gotcha #12）保留并扩展为按 (agent, source) 记录；
- 修复 CLAUDE.md 已记录的坑：Free 模式无 filter 永不触发 → 创建向导强制选择触发源。
- **触发证据透传**：TriggerAgent 动作载荷携带触发上下文（触发源执行记录 id、关键帧/数值快照、当时的判定与置信度），被触发的智能体首轮上下文直接"看到现场"，不必重新拉取感知——这是"实时盯守升级为智能体处置"链路（002 §3.1）的关键一环：智能体与用户看的是同一份证据。

#### 5.2.3 智能体输出契约（Free 模式增强）

`AiAgent` 增加可选字段（bincode 兼容：只加不删，参考 `enable_tool_chaining` 的处理教训——**追加到结构体尾部**）：

```rust
/// 可选结构化输出契约；Free 模式执行收尾时最后一步要求 LLM 产出符合 schema 的 JSON，
/// 发布为 ai:<agent_id>:<field>；不设置则维持现状（纯文本+journal）
pub output_contract: Option<Vec<OperatorField>>,
```

这让 S3 的"复杂任务结论"也能被下游消费——智能体成为数据源的生产者。

#### 5.2.4 巡检的趋势意识（S4 的灵魂）

不恢复已禁用的跨执行学习提取（成本原因，尊重现状），做便宜的替代：**执行时将最近 N 条 journal（默认 10）做时间线摘要注入上下文**（预取机制已有，`executor/memory.rs` prefetch），prompt 模板加一段"与历史对比"指令。零新存储，一次模板改动 + 一次注入点。

#### 5.2.5 Focused 模式的处置

定位收窄为"报告生成器"（低频、文本输出、给通知渠道）。UI 上不再作为新用户默认选项，提供**一键转算子**（服务端从 resources + user_prompt + 输出示例生成 OperatorConfig 草稿）。不设硬删除时间表，观察算子采用率后再决定（开放问题 O7）。

#### 5.2.6 看护项即 chat 工具（chat 调度智能体）

对话中的 AI 选取智能体获取**已理解和分析过的信息**，而非自己啃原始数据（002 §4.6 信息金字塔的实现侧）。三个组件：

1. **动态工具目录**：chat agent 的工具列表在会话建立时由看护项/agent 注册表实时生成，每个看护项暴露两个动作——`query_conclusion`（读最近执行记录 + output_contract 字段 + 状态/新鲜度，零 LLM 成本）与 `run_now`（按需执行，走现有 `/api/agents/:id/invoke` 通路）。工具名与描述取自用户自己的命名——这是 mapper.rs 硬编码中英文别名词典（约 120 行）的根治方向：工具词汇来自用户词汇，语义匹配天然成立，词典只保留平台内置工具的兜底别名。
2. **执行进度中继**：`run_now` 触发的执行，其进度/thinking 事件（executor 已有 send_progress/send_thinking）中继进 chat 的流式通道——用户在对话里看到"正在调取冷库数据…"，两侧基建均已存在，只需桥接。
3. **成本与并发**：chat 触发的执行与调度执行共享现有信号量（全局 10/每后端 2）；`run_now` 设 chat 侧超时（默认 120s）防对话挂死；对同一看护项的重复 `run_now` 计数是沉淀提示（002 §4.6）的信号源。

一致性约束：chat 引用的结论必须来自执行记录（含 output_contract 字段与判定链引用），禁止 chat 自行复述加工——保证"chat 说"与"看护中心说"同源。

#### 5.2.7 槽位化创建（对话即创建）

对话完成创建的可靠性来自平台侧的语义结构，而非模型能力（002 §3.1 三层结构：槽位/实体/回显）。三个技术件：

1. **CareItemDraft IR**：五问的类型化中间表示——`target: Vec<ResourceRef>`、`mode: enum{Watch,Guard,Investigate,Report}`、`delivery: 输出契约+路由`、`handling: enum`、`cadence: 触发规格`，逐槽带默认值与校验规则。取代现有 ParsedIntent（intent_type + `Vec<String>` 的松散结构，`agents.rs`）；ParsedIntent 保留反序列化兼容，新创建全走 IR。chat 工具面：`draft_care_item`（产出草稿）、`update_care_item_slot`（会话内改槽，草稿状态存会话）。
2. **语义资源目录服务**：把 chat 内部的 ResourceIndex（name/alias/keyword/location/capability/type 六级倒排索引）从 `agent/` 私有实现升格为平台级目录服务，消费方 = chat 的 `resolve_entity` 工具、创建向导的实体搜索、规则构建器的数据源选择器。种子数据吸收两处硬编码词典：semantic_mapper.rs 的 LOCATION_ALIASES、tools/mapper.rs 的中英文别名表（词典外置从"待办重构"变为本设计的一部分）；**用户纠正落定为持久别名**（"我说除湿机指这台"），一次教、处处生效。
3. **草稿-确认协议**：chat 侧禁止无确认落库——确认动作最终走现有 `CreateAgentRequest`/`create_agent` 通路（后端 API 不变，前端形态变）；试跑复用算子 `/test` 同款 dry-run；草稿卡与向导表单（002 入口二/入口四）渲染同一结构。

### 5.3 与 kernel 统一重构的衔接（M0）

算子的"单次推理"与 agent 的工具循环共享同一 kernel。M0（重构第一阶段）先行：

```
M0-1  删除死链：session.rs process_message_stream、Agent::process_stream、
      events_to_string_stream（全仓零引用，探索已证实）
M0-2  统一 runtime 工厂：llm_runtime.rs 的手写 match 委托 instance_manager::create_runtime，
      删除第二套缓存；agent 与算子与 chat 三方共用
M0-3  Free/Focused 重复的记忆收尾序列抽 finalize_memory()（mod.rs:1389 vs :1560）
M0-4  从 data_collector.rs 抽出"按 DataSourceId 拉最新值"共享函数 → 算子采集复用
```

M0 完成后，`InferenceClient`（单次调用 + schema 约束）作为 kernel 的公共薄接口落地，算子通路（api/automation）与 agent 通路（agent crate）都调它。**视觉/多模态是第一受益者**：image_utils 保持单源（canonical 表），两条通路的图片解析自然一致。

### 5.4 失败语义总表（概念边界的产品化体现）

| | 智能算子 | 任务/智能体 | chat |
|---|---|---|---|
| 失败表现 | 保旧值 + stale 标记 + 熔断 | Error 状态 + journal 失败记录（gotcha #10）+ 退避 | 用户即时可见，可追问 |
| 重试 | 冷却后探针 | 指数退避（现有） | 用户手动 |
| 观测 | 数据质量视图（成功率/时延/新鲜度） | 执行历史 + journal | 会话记录 |
| 成本护栏 | 预算 + 防抖 + 熔断 | 全局/每后端信号量（现有 10/2/6） | 会话级 |

### 5.5 四场景端到端走查（定位匹配的落地证明）

**S1 图片→存数据**：NE301 摄像头设备 → 算子（scope=Device(cam-01)，inputs=[device:cam-01:image]，OnInputChange 防抖 30s，VLM backend，schema=[{area_clutter: enum(整洁,一般,混乱)}]）→ 发布 `ai:cam01_view:area_clutter` → 仪表盘直接绑定。整链无 agent，单次推理。

**S2 图片→执行**：S1 的输出 `ai:cam01_view:area_clutter` → 规则（condition: `== 混乱`，cooldown 10min）→ 动作 Notify + ExecuteCommand。确定性层执行动作，语义判断在算子——**算子永不直接执行动作**，动作出口收敛在规则/智能体。

**S3 多源复杂处置**：EventFilter `{any:[temp-01, cam-01], within_secs: null}` 触发 Free 智能体（绑定两源资源 + 工具）→ 多轮分析 → output_contract 发布 `ai:root_cause:conclusion` + 通知动作 → 结论成为数据源，进周报算子的输入。

**S4 定时巡检**：Cron 任务（Free，低频）→ 采集 + journal 时间线摘要（5.2.4）→ 报告文本 → 通知渠道；同时一个便宜的 Interval 算子持续产出巡检指标，报告里引用。

### 5.6 组合能力全景

```
设备 ──┬─▶ 扩展模型(YOLO/OCR) ──▶ extension:* 数据源 ──┐
       │                                              ├─▶ 规则 ──▶ Notify/命令/TriggerAgent
       └─▶ 原始遥测 ──▶ device:* ──────────────────────┤
                                                        ├─▶ 仪表盘 / data-push
transform:* (代码变换) ─────────────────────────────────┤
ai:operator:* (模型变换, 新) ───────────────────────────┤
ai:agent:* (智能体结论, 新) ────────────────────────────┘
              ▲                          ▲
              └── 规则 TriggerAgent ──────┘  (确定性→智能的升级通道)
```

---

## 6. 迁移设计

### 6.1 存量兼容

- `/api/agents` 全 API 不动；`agents.redb` 只增字段（尾部追加，bincode 兼容）；EventFilter 多态反序列化，旧字符串行为不变。
- 算子新表存 `automations.redb`，无 schema 破坏。
- **无跨仓库影响**：不动 ABI/设备类型/组件 schema；Extensions、DeviceTypes、Dashboard-Components 三仓库零感知。

### 6.2 Focused 迁移路径

不强制。向导默认不再提供 Focused；存量 Focused agent 继续可用；提供"转算子"导出（服务端把 resources + user_prompt 翻译成 OperatorConfig 草稿，用户确认输出 schema 后落库）。

### 6.3 transform 与算子的关系（防概念混淆）

同页两个 tab，一句话区分写死在 UI 文案：**transform 用代码算（快、确定、免费），算子用模型看（开放语义、按次计成本）**。二者可串联（transform 清洗 → 算子理解）。

### 6.4 文档联动（wiki）

按 neomind-guide 的"Done includes wiki"：`user-guide/6-agent.md` 增补任务/智能体定位与多源触发；新增 `user-guide/11-ai-operators.md`（中英双语都改）；`developer-guide/2-architecture.md` 增补三层概念与 DataSourceId 新类型段。落地于 M2/M3 各自里程碑内，不延后。

---

## 7. 实施计划（PR 粒度，每期可独立验收）

> 本节为概要。**全量代码影响面（前后端）与细化工作项见 [003 实施计划](./003-implementation-plan.md)**——它以 2026-09-22 代码盘点为准，取代下表的粗粒度划分。

| 期 | 内容 | PR | 验收标准 |
|----|------|----|----------|
| **M0** kernel 统一（§5.3） | 死链删除；runtime 工厂合并；finalize_memory 抽取；DataSourceId 采集函数抽取 | PR1-2 | cargo test 全绿；agent 执行行为回归（现有 eval 套件）；`llm_runtime.rs` 手写 match 清零 |
| **M1** 算子 MVP | OperatorAutomation 存储 + L0 通路 + ai:* 发布 + API + dry-run + 前端三步向导 | PR3-6 | S1 场景端到端演示：摄像头→整洁度字段→仪表盘；熔断/保旧值/stale 演示；预算生效 |
| **M2** 多源触发 + 输出契约 | EventFilter 结构化（any/all/within）+ agent output_contract + RunOperator 规则动作 | PR7-8 | S3 场景演示：双源 AND 触发智能体、结论字段进仪表盘；旧 event_filter 回归 |
| **M3** 巡检增强 + 模板化 | journal 时间线注入 + 巡检向导模板 + 感知管道模板（设备→扩展→算子一键配置）+ wiki 同步 | PR9-10 | S4 场景演示：巡检报告含历史对比；向导从零创建巡检任务 ≤3 分钟 |

M0 与 M1 的前端部分可并行。每个 PR 过 CI 门（clippy 1.92.0 -D warnings 既有门）。

---

## 8. 评审决定清单（明早拍板）

| # | 决定 | 建议 |
|---|------|------|
| D1 | 思路选择：A+B 桥接（推荐）/ B 先行 / C | A+B。若"两周内必须上市 S1"压倒一切，退而求 B 但要接受 C1/C2 固化 |
| D2 | 概念命名：智能算子/AI Operator；数据源前缀 `ai:` | 如上；备选前缀 `operator:`（更长但更直白） |
| D3 | L0 是否允许调用工具（如查设备列表） | **不允许**。保持纯函数安全边界；要查就让上游 transform 把数据备好 |
| D4 | 算子触发 MVP 范围 | OnInputChange + Interval + Manual；事件/规则触发进 M2 |
| D5 | 预算默认值松紧 | 建议 max 200 次/天（本地小模型可放宽）、熔断阈值 3、冷却 5min——面向边缘成本保守取值 |
| D6 | 算子存储位置 | `automations.redb` 同域（建议），不新建库文件 |
| D7 | Focused 处置时间表 | 先只做向导降权 + 转算子导出；M3 后按采用率决定是否 deprecate |

## 9. 风险与开放问题

| 风险 | 缓解 |
|------|------|
| 高频算子写 telemetry 的写入放大 | 防抖兜底；输出字段走批量写（复用 telemetry 批量路径）；M1 压测一次 1Hz 防抖 30s 的写入量 |
| prompt 注入（图片 OCR 内容、设备字符串字段可携带恶意指令） | L0 无工具无动作权限（D3），最坏污染一个字段值；schema 枚举类型进一步收窄；文档明示"算子输出仅作数据" |
| 边缘 VLM 可用性参差 | 能力探测链已有（ADR-0014）；向导在选模型时过滤不支持视觉的后端；文档给推荐模型清单（对接 docs/edge-models.md） |
| OQ：算子并发模型（多算子同 tick 竞争单后端） | M1 用全局 per-backend 信号量（复用现有 2 并发策略）；不够再排队 |
| OQ：`ai:` 字段的 data-push 映射 | data-push 按 DataSourceId 前缀匹配，理论自动生效；M1 加一条集成测试确认 |

---

## 附录 A：证据索引

| 事实 | 位置 |
|------|------|
| AiAgent 结构（user_prompt/parsed_intent/resources/schedule/memory…） | `crates/neomind-storage/src/agents.rs:34` |
| AgentSchedule（event_filter 为 Option\<String\>） | `agents.rs:201-213` |
| 执行主管道（collect→intent→analyze→分支） | `crates/neomind-agent/src/ai_agent/executor/mod.rs:1208-1639` |
| Free/Focused 重复记忆收尾 | `executor/mod.rs:1389-1436` vs `:1560-1590` |
| runtime 工厂双轨 | `executor/llm_runtime.rs:115-278` vs `llm_backends/instance_manager.rs:220` |
| 双套缓存/限流 | `llm_runtime_cache`（executor）vs instance_manager 缓存；ConcurrencyLimiter（llm.rs）vs BackendSemaphores（scheduler.rs:23） |
| TransformAutomation（intent/js_code/output_prefix/scope） | `crates/neomind-api/src/automation/types.rs:648` |
| JS 执行器扩展预调用 | `automation/transform/js_executor.rs:42-63, 179-196` |
| 图片输入解析 | `automation/transform/image.rs`（resolve_image_data） |
| transform 数据源 API | `server/router.rs:808-838` |
| agent API 面 | `server/router.rs:840-858` |
| 死链（process_message_stream 零引用） | `session.rs:1558`、`agent/process.rs:607`、`streaming/stream_core.rs:1610` |
| 跨执行学习提取已禁用 | `executor/mod.rs:1438, 1592` 注释 |
| 60s 事件 dedup / gotcha #12 | CLAUDE.md |
| 字段尾部追加的 bincode 教训 | `agents.rs` enable_tool_chaining 字段注释 |

## 附录 B：术语表

- **L0~L3**：自主性等级——单次调用 / 固定流程 / 工具循环 / 跨执行记忆。
- **数据面/控制面/智能面**：数据变换与发布 / 触发与策略 / 自主决策。
- **算子（Operator）**：数据面上的模型推理变换单元，输入 typed 数据源，输出结构化字段。
- **任务（Job）**：现有 ai_agent 的产品话术——触发 + 编排 + 输出路由的包装。
- **InferenceClient**：kernel 暴露的单次受约束推理薄接口（M0 产物）。
