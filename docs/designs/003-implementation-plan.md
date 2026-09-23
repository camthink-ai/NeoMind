# NeoMind 智能能力重构 — 实施计划（全量代码影响面）

| | |
|---|---|
| **状态** | 待评审（与 001/002 同场） |
| **定位** | 细化并取代 [001](./001-ai-capability-model.md) §7 的粗粒度分期；以 2026-09-22 全量代码盘点为准（main@07f59b09），覆盖**前后端所有相关模块** |
| **配套** | 需求来源 = 002 的 F1-F15；架构依据 = 001 §5 |

---

## 1. 代码影响面全景

### 1.1 后端地图

| 模块 | 关键文件（行数） | 实施相关结论 |
|------|------------------|--------------|
| agent crate 内部 | `neomind-agent/src/ai_agent/`（executor 12 文件、scheduler、event_trigger）；`agent/`（chat 链路） | M0 重构对象（死链、双栈、伪模块），已由前期分析定位 |
| agent handlers | `neomind-api/src/handlers/agents.rs`（2827 行，22 端点） | create_agent **不调 LLM**——intent 惰性解析于执行时（executor/mod.rs:1327）；F14 槽位创建要在创建时接 `parse_intent`（intent.rs:4 已有） |
| App state | `server/state/agent_state.rs`（惰性 AiAgentManager）；`server/types.rs:2902-3130`（构造/启动） | 算子引擎**不进** AgentState，进 `automation_state.rs`（已有 `automation_store`/`transform_engine` 字段位） |
| 事件接线 | `types.rs:3345 init_agent_events`（DeviceMetric/ExtensionOutput → agent 触发）；`event_services.rs:18 TransformEventService`（DeviceMetric → transform，跳过 is_virtual） | 全平台仅这两个 DeviceMetric 消费者；算子引擎 = 第三个，照抄 TransformEventService 模式 |
| 输出发布 | **三条内联发布范式**：transform 虚拟指标（event_services.rs:335-411 双命名空间 DeviceMetric+双写 telemetry）、REST（devices/metrics.rs:108）、扩展（extension_metrics.rs:175） | 无公共函数——M0 抽取 `publish_virtual_metric()`，算子/agent 输出共用 |
| chat/session | `handlers/sessions.rs`（1909 行；WS `ws_chat_handler`:1106 + REST `chat_handler`:904 双流式） | 结构稳定不动；F13 chat 工具在此层加工具注册 |
| automations | `handlers/automations.rs`（1309 行，含 transforms data-sources 系列）；`automation/transform/` 引擎 | 算子 handler 挂同族；`/test` dry-run 范式已有（test_transform_handler:924） |
| 事件类型 | `neomind-core/src/event.rs`（NeoMindEvent；无 DataSourceUpdated，数据流走 DeviceMetric/ExtensionOutput） | 新增 `ai` 类型数据源**不加新事件**，复用 DeviceMetric{is_virtual} |
| 通知 | `neomind-messages/manager.rs`（create_message:300 及 alert 族） | 算子/agent 通知统一入口，注入链路已通（AgentExecutorConfig.message_manager） |
| CLI | `neomind-cli-ops/src/agent_cmd.rs`（纯 HTTP 客户端）+ dispatch/commands.rs:1456 | 新端点按需补子命令，零耦合 |
| 桌面端 | `web/src-tauri/` 只调 `edge_api::start_server()` | **零影响**（仅当改 neomind-api 公共 re-export 时需 grep，风险极低） |

### 1.2 前端地图（web/src/）

| 区域 | 关键文件（行数） | 实施相关结论 |
|------|------------------|--------------|
| Agents 页族 | `pages/agents.tsx`（781，4 tab）+ `agents-components/`（编辑器 2189、执行时间线 1003、详情 726 等约 9k 行） | 编辑器是**单页表单非分步**；M2 多源触发编辑、M4 智能体 tab 原地升级在此页族（菜单/路由/tab 结构不动） |
| 状态管理 | **无 agent slice**——列表在页面本地 state；Zustand+fetchCache 模式在 `lib/utils/async.ts`（187） | 运行态数据需新建 `store/slices/` slice（照搬 extensionSlice 模式） |
| API client | `lib/api/agents.ts`（560，混杂 dashboards/memory/timezone） | M1 顺手拆分；新增 `lib/api/operators.ts` |
| Chat | `hooks/useChatStream.ts`（343，共享流状态机）+ `components/chat/`（Composer 401 含图片压缩上传） | F13 进度中继直接消费 useChatStream；pageAssistant.ts 有 agents 页提示词 |
| 自动化页 | `pages/automation.tsx`（759，rules/transforms 双 tab）+ `components/automation/`（TransformBuilderSplit 1470、BuilderShell、FullScreenDialog 257） | **算子 tab 落点**；BuilderShell/FullScreenDialog/UnifiedFormDialog 直接复用 |
| 数据源选择 | `components/dashboard/config/UnifiedDataSourceConfig.tsx`（1605，支持 device/extension/transform 多类型+多选） | 算子向导的输入选择器**复用它**，避免 TransformBuilderSplit 的自实现第三套 picker |
| 仪表盘类型 | `types/dashboard.ts`（684）——**`'ai'` source 类型已存在**（:20 类型定义、:341 旧数据迁移映射 agentId+status、:395 `ai:{id}` 事件前缀）；`store/persistence/types.ts:246 fromDashboardDTO` | `ai:` 数据源**不是从零建**，是语义扩展（现仅 status 字段）→ M1 收编为 `ai:<prefix>:<field>` 并兼容旧映射；改类型必经 fromDashboardDTO（gotcha #1） |
| 三方交叉点 | `ai-analyst/useAnalystSession.ts`（1162，资源格式与编辑器对齐）、`AgentMonitorWidget.tsx`（1127） | **改 agent 资源 schema 必须联动这两处** |
| 导航/路由 | `components/layout/navItems.ts`（59）+ `MobileNav.tsx`（385，**自维护一份列表**）；`App.tsx`（688 lazy 路由） | 改菜单三处同步 |
| i18n | `i18n/locales/{en,zh}/`（agents 702、automation 1120 行/语言） | 新 key 成对加；namespace 在 config.ts 注册 |
| 遗留死代码 | `components/alerts/UnifiedAlertChannelsTab.tsx`（819，无引用） | 顺手删除 |

### 1.3 决定实施方案的十个事实

1. 前端 `ai` 数据源类型已存在（语义=agent status）——设计是**语义收编**而非新建；
2. `AutomationState` 已有 store/engine 字段位——算子引擎零接线成本挂载；
3. TransformEventService 是算子事件服务的现成模板（含 is_virtual 防回馈环处理）；
4. 输出发布三处内联——M0 抽公共函数是一切数据面工作的前置；
5. create_agent 不调 LLM（惰性 intent）——槽位创建接现成 parse_intent 即可；
6. invoke_agent 已同步等待+超时 detach——F13 现场跑的后端通路现成；
7. 前端无 agent slice、编辑器单页 2189 行——智能体 tab 走**原地升级**（卡片增强，无新 tab、无新页面）；
8. UnifiedDataSourceConfig 就是统一 picker——向导输入选择零新造；
9. useChatStream 共享状态机已落地（ADR-0015）——chat 侧新交互的地基在；
10. OpenAPI 漂移守卫在 CI（ADR-0013，136 schema）——**每个新端点必须同步注册 schema**，否则 CI 红。

---

## 2. 分期计划

规模标记：S=≤2 天 / M=约一周 / L=约两周（单人等效，供排期参考）。

### M0 — Kernel 与发布地基（纯后端，零产品变化）

| # | 工作项 | 文件 | 规模 |
|---|--------|------|------|
| M0-1 ✅ | ~~删死链~~（已完成 2026-09-22：三个死函数 + 两处 re-export 删除，715 测试全绿，未提交） | neomind-agent | S |
| M0-2 ✅ | ~~统一 runtime 工厂~~（已完成 2026-09-22：agent 侧 165 行 match 与 api 侧默认 runtime 构造均委托 `get_instance_manager()`，第二套缓存删除；env 超时收进 IM 单点；类型规范为 `Arc<dyn LlmRuntime>`。**行为升级**：agent 路径获得 /api/show、/props 能力探测与 user_override 尊重；cloud 默认超时统一 300s） | neomind-agent + neomind-api | M |
| M0-3 ✅ | ~~Free/Focused 重复记忆收尾抽 `finalize_execution_memory()`~~（已完成 2026-09-22：抽取至 executor/memory.rs，顺带修复 Focused 分支缺失 FIFO 裁剪的漂移） | neomind-agent | S |
| M0-4 ✅ | ~~抽 `publish_virtual_metric()`~~（已完成 2026-09-22：`automation/metric_publish.rs` 落地——转换函数族 + VirtualMetricPublisher 双命名空间发布/双写/规则刷新，三处内联收编，附转换器单测） | neomind-api | M |
| M0-5 ✅ | ~~抽 fetch_latest_by_datasource~~（**无需新代码**：`TimeSeriesStorage::latest/latest_batch` 与 `query_latest_batch` 已是按 DataSourceId 读最新值的现成 API，算子引擎直接调用；抽取假设作废） | — | S |
| M0-6 ✅ | ~~删 UnifiedAlertChannelsTab.tsx~~（已完成 2026-09-22：连同 alerts/index.ts 整目录删除，tsc 通过） | web | S |

**验证**：workspace `cargo test` 全绿（含既有 eval 套件）；clippy 1.92.0 `-D warnings` 门通过；三处发布路径行为回归（transform 虚拟指标/REST 摄入/扩展指标各一条集成测试）；前端 `tsc + build`。

### M1 — 智能算子 MVP（S1 端到端：图片/数据 → 结构化字段 → 仪表盘）

**后端**：

| # | 工作项 | 文件 | 规模 |
|---|--------|------|------|
| M1-1 | 算子落 ai_agent 域：`ExecutionMode` 尾加 `Structured` + `AiAgent` 尾部追加字段（输出 schema/防抖/平滑/预算，bincode 兼容）；存储 agents.redb（2026-09-22 评审修订，见 001） | neomind-storage/agents.rs | M |
| M1-2 | `InferenceClient` 薄接口（单次受约束调用 + schema 解析，thinking 强制关闭 gotcha #7） | neomind-agent（基于 M0-2 工厂） | M |
| M1-3 | `OperatorEngine`：触发（OnInputChange/Interval/Manual + 摄像头三档采样）→ 采集（M0-5）→ prompt 组装（含纠正样本注入钩子）→ 推理 → schema 校验（重试一次）→ `publish_virtual_metric`（M0-4） | automation/transform/operator/（新目录） | L |
| M1-4 | 状态机 + 熔断 + 预算（Healthy/Degraded/Paused，保旧值+stale TTL） | 同上 | M |
| M1-5 | 触发复用现有 AgentScheduler（interval/cron/manual；OnInputChange 防抖映射为 interval+事件冷却）——无需新事件服务 | — | S |
| M1-6 | handlers：CRUD + `/run`（手动同步）+ `/test`（dry-run 不入库）+ `/executions`（滚动 100 条）+ 数据源列表纳入 `ai:*` | handlers/automations.rs + router.rs + **OpenAPI schema 注册** | M |
| M1-7 | 证据存储：执行记录携带输入引用（图片路径/来源值），evidence 可回查 | operator 执行记录 | S |

**前端**：

| # | 工作项 | 文件 | 规模 |
|---|--------|------|------|
| M1-8 | automation 页新增**算子 tab**（与 rules/transforms 并列，**过渡形态**——终态统一进智能体页：算子是智能体的一种类型、不分组，M4 落地后此 tab 保留为高级入口或移除）；`lib/api/operators.ts` 新建 + `lib/api/agents.ts` 顺手拆分 | pages/automation.tsx、lib/api/ | M |
| M1-9 | 算子编辑器：照现有 AgentEditorFullScreen 单页表单模式做增量（类型字段、调度扩展行、预算护栏、"输出与试跑"区块含 dry-run 即时预览 + schema 预生成），复用 FullScreenDialog 与资源选择/AI 推荐组件 | automation-components/（新目录） | L |
| M1-10 | `ai:` 数据源语义收编：types/dashboard.ts 旧 'ai'（agentId+status）映射兼容 + `ai:<prefix>:<field>` 新形态；UnifiedDataSourceConfig 列表纳入；fromDashboardDTO 全链过测 | types/dashboard.ts、UnifiedDataSourceConfig、persistence/ | M |
| M1-11 | i18n en/zh 成对 + namespace 注册 | i18n/ | S |

**CLI**：`neomind operator list/run/test`（agent_cmd.rs 旁新增，纯 HTTP）——S。

**验证**：S1 场景端到端演示（摄像头→整洁度/质检字段→仪表盘绑定实时更新）；dry-run 向导交互演示；熔断→保旧值→stale 标记演示；预算超限自动 Paused 演示；旧 'ai' 数据源仪表盘回归不破。

### M2 — 盯守组合与多源触发（S2/S3 核心 + 判定链）

| # | 工作项 | 文件 | 规模 |
|---|--------|------|------|
| M2-1 ✅ | ~~`EventFilter` 结构化~~（已完成 **2026-09-23**：`EventSource`/`EventFilter`/`WindowState` 落地在 neomind-storage，**磁盘类型不动**——多态落在 `parsed_event_filter()` 解析层，`sources` 作为 `any` 的别名，旧 `event_type` 形态翻译而非解析成空；窗口聚合在 executor 的 `event_fires()`，部分命中**不消耗** 60s 冷却。**前端**：编辑器任一/全部卡片 + 时间窗一句化 + 单源时隐去选择） | neomind-storage/agents.rs + event_trigger.rs + agents-components | L |
| M2-2 ✅ | ~~agent `output_contract`~~（已完成：`AiAgent.output_schema` 尾部追加 + `ai:<agent_id>:<field>` 发布，结构化分支与推理 agent 收尾两条路都覆盖） | agents.rs + executor | M |
| M2-3 ⏸ | ~~TriggerAgent 证据透传~~ —— **按约定并入「证据专题」**（与 M2-5 第二跳、F4、M3-4 同批做，避免分三次各碰一半） | rules TriggerAgent action + executor 触发上下文 | M |
| M2-4 ✅ | ~~规则动作 `RunOperator`~~（已完成：`RuleAction::RunOperator` 尾追加 + 两个动作共用抽取出的 `spawn_agent_run()`（保持原有的 spawn 不阻塞语义）+ 校验/预览/API DTO 三处齐） | neomind-rules actions | S |
| M2-5 🔶 | ~~判定链 v1~~ —— **第一跳已交付**（通知 → 规则执行：告警 metadata 带 `(rule_id, triggered_at)`，`GET /api/messages/:id/chain` 走回来，含三种未解析态）；**第二跳**（→ 算子执行 → 原始输入）并入证据专题 | storage + handlers | M |
| M2-6 ✅ | ~~前端：多源触发 UI + 告警详情判定链~~（两半都完成：编辑器任一/全部卡片 + 时间窗；告警详情「为什么发出来的」一节，含依据/时刻/已执行动作 + 「查看依据」跳执行） | agents-components/、messages 页族 | L |
| M2-7 ❌ | ~~两个交叉点联动~~ —— **实测不需要联动**：M1 全为尾部追加，两处读取侧本来就防御，没有静默错。真正的缺口是它们**不认识结构化模式**（能力缺口非 bug），**并入 M4-1** 一起做 | 两文件 | M |

**验证**：S2（算子输出→规则→通知+命令）与 S3（双源 AND 时间窗触发智能体→结论字段进仪表盘）端到端演示；旧 event_filter 字符串行为回归；判定链从通知点开可回溯到原始输入。

### M2 期间额外完成（不在本表内，但同批交付）

| 项 | 说明 |
|---|---|
| 002 §3.4 反馈闭环 | 两半齐了：**联动回显**（判定链，见 M2-5）+ **误报反馈**（`MessageStatus::FalsePositive` → 按规则累计 → 阈值建议）。建议只提示，从不改规则 —— 端到端测试断言规则一字节未动 |
| F4 证据随输出 | 发布的 AI 字段带 `metadata.execution_id`（回指那次执行）+ `quality`（**模型自报的置信度**，不是常量）；详情页「输出字段」可看新鲜度/置信度并跳到那次执行 |
| 静默失败三修 | ① 结构化 agent **零输入不再发布结论**（原先拿自己的旧结论当输入循环）；② 采集器那道「没采到数据」的守卫**从未触发过**（在混入 memory 之后才判空）；③ 启动的**键迁移会把 `ai:` 前缀误伤**成 `device:ai:`，导致重启后 AI 指标历史为空 —— 已修并**回搬**受损数据 |

### M3 — 交互深化（chat 三身份落地）

| # | 工作项 | 文件 | 规模 |
|---|--------|------|------|
| M3-1 | 看护项即 chat 工具：动态工具目录（会话建立时由注册表生成）+ `query_conclusion`/`run_now` + invoke 进度中继进 useChatStream 事件流 | toolkit/ 新工具 + sessions 层 + stream 事件桥接 | L |
| M3-2 | 槽位化创建：`CareItemDraft` IR（五槽类型化）+ chat `draft_care_item`/`update_care_item_slot` + 草稿-确认协议（落库走现有 create_agent） | 新 IR + handlers/agents.rs 创建路径接 parse_intent | L |
| M3-3 | 语义资源目录服务：ResourceIndex 升格平台服务 + 吸收 LOCATION_ALIASES/mapper 词典种子 + 用户纠正落别名 | context/resource_index.rs 迁出 + 目录服务 | M |
| M3-4 | 纠错样本：就地纠正端点 + 样本库存储 + prompt 注入（接 M1-3 钩子）+ 前端纠正入口（判定链详情/字段卡） | operator 执行记录扩展 + 前端 | M |
| M3-5 | chat 前端：草稿确认卡组件（对话内渲染，与向导同构）+ 沉淀提示（重复 run_now 计数） | components/chat/ | M |

**验证**：对话内完成"想→建→用"全程演示（002 §4.1 入口四剧本）；chat 问"冷库怎么样"秒答带新鲜度+判定链引用；纠正一次→重跑改善演示。

### M4 — 产品化收口（智能体页升级与价值可见）

| # | 工作项 | 文件 | 规模 |
|---|--------|------|------|
| M4-1 | 智能体 tab 原地升级：新 `store/slices/` slice（fetchCache 模式）+ 现有列表卡片增强（在岗/降级/新鲜度/今日动作）+ 详情面板四 tab 增量（总览插"运行状态/输出字段"section、记忆 tab 加纠正样本）+ 编辑器增量（类型字段）+ 底部价值回顾条；**菜单/路由/tab 结构均不动**（`/agents` 保持，列表不分算子/智能体组） | web/agents 页族 | M |
| M4-2 | 模板库：看护项模板（含 prompt/schema/规则/样本库）导入导出 + 首发模板集（按 §8 Q3 定）+ 安装向导 | storage + 前端 | M |
| M4-3 | 误报反馈闭环：标记误报→纠正样本→阈值调优建议 + 消息详情挂判定链；认领/派单/升级链移出范围 | messages 页族 + neomind-messages | S |
| M4-4 | 巡检增强：journal 时间线摘要注入（近 10 条）+ 巡检向导模板 | executor/memory.rs prefetch + prompt | S |
| M4-5 | 价值回顾：月度聚合任务（值守时长/作业次数/异常数/替代工时）+ 推送 + 页面 | 聚合任务 + messages | M |
| M4-6 | Focused 降权 + 一键转算子导出 | 编辑器 + 服务端转换 | S |
| M4-7 | wiki 同步（user-guide agent 页改版 + 新算子页中英双语 + developer-guide 架构） | wiki-documents 仓库 | M |

**验证**：S4 场景演示（巡检报告含历史对比）；升级后的智能体页一屏总览；月度价值回顾推送；wiki 发布。

---

## 3. 需求覆盖矩阵（F×里程碑）

| 需求 | 优先级 | M0 | M1 | M2 | M3 | M4 |
|------|:---:|:---:|:---:|:---:|:---:|:---:|
| F1 看护项+编译器 | P0 | | ●(看/盯编译) | ●(查编译+多源) | ●(对话创建) | ●(模板) |
| F2 试跑 dry-run | P0 | | ● | | ●(对话内) | |
| F3 判定链 | P0 | | ●(证据) | ●(三层关联+展示) | | ●(处置关联) |
| F4 证据随输出 | P0 | | ● | ●(透传) | | |
| F5 纠错样本 | P1 | | ●(钩子) | | ●(全链) | |
| F6 模板库 | P1 | | | | | ● |
| F7 智能体页升级 | P1 | | | | | ● |
| F8 误报反馈闭环 | P1 | | | | | ● |
| F9 chat 沉淀 | P2 | | | | ●(计数) | ●(提示) |
| F10 价值回顾 | P2 | | | | | ● |
| F11 成本护栏 | P1 | | ● | | | |
| F12 处置闭环 | — | 移出范围（业务化，属上层业务系统） | | | | |
| F13 看护项即工具 | P1 | | | | ● | |
| F14 槽位化创建 | P1 | | | | ● | |
| F15 语义资源目录 | P1 | | | | ● | |

## 4. 测试与验证策略

- **每 PR**：workspace `cargo test`；clippy 1.92.0 `-D warnings`（注意 PATH 陷阱，CLAUDE.md 有正确命令）；前端 `tsc --noEmit` + `npm run build`；新端点 OpenAPI schema 注册（ADR-0013 漂移守卫）。
- **M0 回归**：三处发布路径集成测试；既有 agent eval 套件（comprehensive_agent_eval / mock_llm_integration / cancellation）全量跑。
- **M1 新增**：OperatorEngine 单测（mock backend：schema 通过/失败重试/熔断迁移/预算暂停）；dry-run 不落库断言；`ai:` 数据源经 fromDashboardDTO 的往返测试（gotcha #1）。
- **M2 新增**：EventFilter 双态反序列化（旧字符串回归）；时间窗聚合单测（开窗/全中/超时清窗）；证据透传端到端。
- **场景验收**：每期以 S1-S4 对应场景的端到端演示作为里程碑完成的定义（见各期验证栏）。
- **桌面端**：M1/M4 各跑一次 `npm run tauri:dev` 冒烟（gotcha #2：workspace 构建不覆盖它）。

## 5. 依赖、并行与风险

**依赖链**：M0-2→M1-2（InferenceClient 用统一工厂）；M0-4→M1-3/M2-2（发布）；M0-5→M1-3（采集）；M1-3 钩子→M3-4（样本注入）；M2-5→M4-3（反馈闭环关联判定链）。**可并行**：M1 前端（M1-8~11）与后端（M1-1~7）按 OpenAPI 契约先行分头开发；M3-3 语义目录可与 M2 并行。

**主要风险与缓解**：

| 风险 | 缓解 |
|------|------|
| 算子高频写 telemetry 放大 | M1 压测 1Hz 输入防抖 30s 场景；write_batch 批量；预算默认保守（001 D5） |
| `ai:` 语义收编破坏旧仪表盘 | M1-10 先写旧数据迁移映射的回归测试再动类型（types/dashboard.ts:341 现状） |
| EventFilter/output_contract 改 agents.redb | 只尾部追加（enable_tool_chaining 先例注释在同文件）；写迁移单测 |
| 前端 agent 页族体量大（编辑器 2189 行） | 智能体 tab **原地升级**（卡片增强）、无新页面新菜单；编辑器只做增量（多源 UI、降权） |
| chat 工具动态目录与会话建立时机 | 目录随 DataChanged 事件失效重建（useDataVersion 模式已有） |
| M2-7 交叉点漏改 | CI 加资源 schema 类型检查不现实，列入 M2 验证清单人工双查 |

## 6. 立即可启动的第一批任务（M0-1/M0-4 细化）

1. **M0-1**（半天）：删 `session.rs:1558 process_message_stream`、`agent/process.rs:607 process_stream`、`streaming/stream_core.rs:1610 events_to_string_stream` 及 mod.rs:62 re-export → `cargo build --tests` 收敛 unresolved import → 跑 eval 套件。
2. **M0-4**（一周）：在 neomind-api 定义 `publish_virtual_metric()`，签名吸收 event_services.rs:335-411 的双命名空间+双写+value_provider 语义 → 三处调用点替换 → 每处一条集成测试证明行为不变。

评审焦点：分期边界是否认可（尤其 M3/M4 的先后——若 chat 交互是近期卖点，M3 可与 M2 对调或并行）。
