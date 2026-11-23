---
cursor:
  subagentId: "bc-aa2cd044-96ca-5d35-bedf-4c0c8c760942"
---

# Vision / Agent 修复计划（东坪山告警路径）

**受众：** mingming shen  
**依据：** [`t4-three-agent-review.md`](t4-three-agent-review.md)、[`t4-dongpingshan-agents-audit.md`](t4-dongpingshan-agents-audit.md)、[`1-3-2-vision-false-success.md`](1-3-2-vision-false-success.md)、[`t4-1-3-2-fix-audit.md`](t4-1-3-2-fix-audit.md)  
**关联现场对话：** [`t4-latest-chat-issues.md`](t4-latest-chat-issues.md)（Chat 上下文压垮 → 后续轮无工具，与本计划独立但同属 T4 体验）  
**范围：** 计划 only（本票无代码改动 / 无 push / 无 restart）

---

## 一句话目标

把「事件图 → 诚实看图 → **仅真赌博才告警** → Failed/Completed 口径可信」收成可验证产品行为；1.3.2 的 vision 假成功门禁只覆盖「承认视觉失败」，**不覆盖「无赌博却 message send」**。

---

## 工作流总序（P0 → P2）

| 优先级 | 工作流 | 现场伤害 |
|--------|--------|----------|
| **P0** | 告警语义门禁（结论否定却发消息） | 假告警打爆 IM |
| **P0** | Prompt 编辑同步 / 失效 knowledge | 用户改了 prompt，旧「用 vision 工具」仍注入 |
| **P0** | `resources=[]` / `NO DATA COLLECTED` 误导 | 模型以为无图 / 乱找路径 |
| **P1** | 告警类 agent 模式与模型引导 | MiniCPM-1B free 环：duplicate / 错参 / 臆造工具 |
| **P2** | Journal ↔ stats 成功口径对齐 | 运维读绿、journal 红 |

---

## P0-1 — 告警语义门禁

### Goal
结论（或同等证据）写明 **无人 / 无赌博 / 无证据** 时，**禁止**本轮把 `neomind message send`（或等价通知）算作成功 Completed；应 Failed（或至少不投递）。

### Approach
1. 在 agent 执行收尾（`executor` / `tool_result` / vision conclusion 路径）增加 **semantic contradiction gate**：  
   - 扫描本轮 conclusion + 成功的 `message send` body/title；  
   - 命中否定赌博/无人类 marker（中英，对齐现有 `vision_path` marker 表风格）且存在成功告警 send → `Err` / business failure。  
2. 可选加固：`notify.on=judgment` 时，仅当 conclusion 显式「应告警」才允许 send；否则 send 成功也降级为 Failed。  
3. 不要用「stats 事后扣」代替：先挡投递或挡 Completed。

### Files / areas
- `crates/neomind-agent/src/ai_agent/executor/vision_path.rs`（marker 表可复用/扩展）
- `crates/neomind-agent/src/ai_agent/executor/mod.rs` / `tool_result.rs`（收尾判定）
- `crates/neomind-agent/src/ai_agent/executor/behavior_tests.rs`（东坪山形用例）
- 参考：`notify_on_judgment_*` 既有测试契约

### Tests
- 表驱动：conclusion「无赌博行为证据」+ shell `message send` 成功 → **Failed**  
- 表驱动：conclusion「有人且疑似赌博」+ send 成功 → **Completed**  
- 诚实暗光 / 「视觉不可用」仍走既有 vision false-success 路径（不回归）

### T4 verify
- 东坪山三台：窗内 **0** 条 inbox「疑似赌博」正文含「无赌博/无人」  
- 200366 改后日志：假告警 Completed 归零；允许 Failed↑

### Risk
- Marker 过宽误杀「描述了无赌博后仍写建议文案」的诚实回合 → 收紧为 **send 已成功** 才触发  
- 中英混写 / 模型改写同义句 → 维护 marker + 抽 1–2 个现场真实 conclusion 做回归

---

## P0-2 — Prompt 编辑同步 / 失效 knowledge

### Goal
改 `user_prompt`（API/CLI/UI）后，运行时 **不再**注入仍写「用 vision 工具」的旧 `task-understanding.md` / 过期 `parsed_intent`。

### Approach
任选其一（建议组合短路径 + 硬失效）：
1. **写路径：** `update_agent` / `save_agent` 在 prompt 变更时 regenerate 或 delete `memory/agents/<id>/custom/task-understanding.md`，并 clear `parsed_intent`。  
2. **读路径：** prefetch knowledge 时若 `task-understanding` 与当前 `user_prompt` hash 不一致 → 跳过或覆盖注入。  
3. 全局 `PROCEDURES.md` / `KNOWLEDGE.md` 中「用 vision 工具」与 InlineMultimodal 矛盾句：产品文案修订（运维可先手工，代码侧 standing prompt 已有 rewrite 则避免双重教法）。

### Files / areas
- Agent update handlers：`neomind-api` agent routes + `neomind-cli-ops` `update_agent`
- `crates/neomind-agent/src/ai_agent/executor/mod.rs`（`prefetch_knowledge_files` / `parsed_intent`）
- `crates/neomind-agent/src/ai_agent/executor/memory.rs`
- On-disk：`/var/lib/neomind/data/memory/agents/<id>/custom/task-understanding.md`

### Tests
- Update prompt → knowledge file 内容或 mtime/hash 同步；下一轮工具 prompt **不含**旧「vision 工具」句  
- 仅改 name/notify 不误删 knowledge

### T4 verify
- 三台 `task-understanding.md` 与当前 `user_prompt` 一致（或文件已删且运行时无旧句）  
- 日志仍可有 `Excluded vision`（Inline 正常），但模型不再「未找到图像分析工具」为主病

### Risk
- 用户自定义 knowledge 被误删 → 只失效 `task-understanding` / hash 标记文件，不动其它 custom  
- 多实例/只读副本：以 store 写路径为准

---

## P0-3 — `resources=[]` / 事件图路径不要 `NO DATA COLLECTED`

### Goal
事件触发且 `extract_image_data` 已成功时，collector **不得**用「bound sources 为空 → NO DATA COLLECTED」误导模型；`resources` 可为空但事件图应算「已有输入」。

### Approach
1. **代码：** `data_collector`：若本轮已有 event 内联图 / extracted images，跳过或改写 `NO DATA COLLECTED` placeholder（改为「image embedded from event」类诚实句）。  
2. **配置（可并行运维）：** 恢复三台 `resources` 绑定（device `image_data`），与创建时一致——降低对 collector 特例的依赖。  
3. `tool_prompt` 中 `resources.is_empty()` 分支：事件 multimodal 路径不要暗示「无绑定数据」。

### Files / areas
- `crates/neomind-agent/src/ai_agent/executor/data_collector.rs`（`NO DATA COLLECTED`）
- `crates/neomind-agent/src/ai_agent/executor/tool_prompt.rs`
- `crates/neomind-agent/src/ai_agent/executor/event_trigger.rs`
- 运维：`neomind agent update …` resources / UI 绑定

### Tests
- Event + inline image + `resources=[]` → prompt **无** `NO DATA COLLECTED`；含图像已嵌入说明  
- 真无图无 resource → 仍明确无数据（structured 拒绝发布契约保持）

### T4 verify
- 改后窗：`NO DATA COLLECTED` 在「有 extract_image_data 成功」轮次为 0  
- 「无 image_data / 未找到图像路径」叙事显著下降

### Risk
- 与 structured「无输入则 error」混淆 → 仅放宽 **free + event image** 路径文案，不放宽 structured 拒跑

---

## P1 — 告警类 agent：模式 / 模型引导

### Goal
赌博监测类任务不再默认 **MiniCPM-V 1B + free 多工具环**；产品给出可执行默认或硬引导。

### Approach
1. **配置模板 / 向导：** 告警+看图 → 推荐 `focused`（或 structured）+ 白名单 `allowed_tools=["shell"]`；VLM 仅看图，通知参数由模板填充。  
2. **或双模型：** 文本主脑（工具）+ 显式 vision companion；避免 1B 同时做工具环与判定。  
3. UI/CLI 在 `execution_mode=free` + builtin 1B VLM + `message send` 类 prompt 时 warning。  
4. 收紧 clap 错参重试：缺 `--title/--body` 的 send 快速 Failed，避免熔断噪声淹没真信号。

### Files / areas
- Agent create/update defaults：storage + API + web agent editor  
- `tool_config.allowed_tools`  
- Docs / skill：`agent-management`  
- 可选：executor 对 1B+free+notify 的 runtime warn

### Tests
- 模板创建：mode/tools 断言  
- Free+1B warning 单测（若做代码 warning）

### T4 verify
- 三台改为推荐配置后：duplicate loop / 臆造 `message_send` 工具 / timeout=0 显著下降  
- 真赌博帧仍能告警（需标注样本）

### Risk
- 改 mode 可能改变既有自动化行为 → 需备份 `agents.redb` + 灰度单台  
- 用户坚持 free：允许但必须过 P0 门禁

---

## P2 — Journal vs stats 对齐（可选）

### Goal
`stats.successful_executions` 与 journal `success` / 业务 Failed 一致，避免「门禁打 Failed 但历史累计仍偏绿」的运维错觉。

### Approach
1. 审计 `update_agent_stats`：仅 `business_ok` 增成功；确认 Ok→Completed 与 Err→Failed 均已接线（1.3.2 部分已做）。  
2. 对「语义门禁 Failed」确保计入 failure。  
3. 可选：CLI `agent get` 展示 journal 近窗 success rate，不只 lifetime stats。

### Files / areas
- `executor/mod.rs` `update_agent_stats`  
- Journal write 成功/失败分支（含 outer `Err`）  
- CLI agent 展示

### Tests
- 假告警门禁触发 → stats failure +1、journal `success=false`  
- 既有 vision marker Failed 同步

### T4 verify
- 采样窗 journal 与 stats **增量**一致（不要求改写历史累计，除非做迁移）

### Risk
- 回填历史 stats 会改报表 → 默认只修增量

---

## 建议落地顺序（工程）

1. **P0-1 语义门禁**（最大现场伤害，可独立合入）  
2. **P0-3 collector 文案**（小改、立刻减幻觉）  
3. **P0-2 prompt↔knowledge**（配置/写路径）  
4. **P1 模板/引导**（产品+运维）  
5. **P2 stats**（收尾）

每步：`cargo test -p neomind-agent --features test-utils --lib` 相关过滤 + T4 只读对照窗（授权后再改配置）。

---

## 非本计划（刻意外置）

- Chat 会话上下文压垮 / `no_feedback_degrade_no_tools` → 见 [`t4-latest-chat-issues.md`](t4-latest-chat-issues.md)  
- Skill create ASCII id（1.3.2 已宣称到位）  
- Rule `--name`+`--body`（已到位）

---

## Rails

只读计划；未改 T4、未 restart、未触碰 Mac `:29375`/`:29385`/`:18788`、未 push。
