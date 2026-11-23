---
cursor:
  subagentId: "bc-aa2cd044-96ca-5d35-bedf-4c0c8c760942"
---

# T4 最近一次对话暴露的问题

**采样：** 2026-11-23 ~13:00–13:07 Asia/Shanghai  
**主机：** `ubuntu@43.132.189.162`（BatchMode SSH，只读；未 restart / 未改配置 / 未触碰 Mac `:29375`/`:29385`/`:18788`）  
**版本：** NeoMind **1.3.2**；Chat 模型本会话为 **SparkX2.6-1.7B** `@:29375`（`n_ctx=65536`）  
**摘录：** [`internal/t4-latest-chat-ebc333d4-excerpt.json`](../internal/t4-latest-chat-ebc333d4-excerpt.json)  
**关联计划：** [`vision-agent-fix-plan.md`](vision-agent-fix-plan.md)

---

## 对用户问题的直接回答

> 「好像上下文满了？后续对话都有问题？」

**部分正确，但不是经典 ZeroBudget。**

| 信号 | 本会话 |
|------|--------|
| `available_tokens=0` / `ZeroBudget` | **未出现**（今日日志计数 0） |
| Adaptive `skip_tools=true` / `skip_history=true` / `use_compact=true` | **未出现**（一直 `skip_*=false`，`prompt_budget=45875`） |
| `hard_budget_dropped_latest_tool_feedback` | **4 次**（同一会话 follow-up） |
| `no_feedback_degrade_no_tools` → `has_tools=false` | **4 次**（后续轮被剥工具） |
| 摘要压缩 | **有**（`force=true`，LLM 摘要失败 → digest fallback；`summary_up_to_index=6`） |

**结论：** 后续轮「坏掉」的主因是 **硬上下文预算丢掉了最新 tool feedback → 产品主动 strip 全部工具**；小模型再在无工具模式下把 `<tool_call>…` 当纯文本喷出。这与「上下文满了」体感一致，机制名不是 ZeroBudget，而是 **no-feedback degrade**。

---

## 1. 认定的「最近一次对话」

`sessions.redb` `list_sessions`（按 recency stamp）第一名：

| 项 | 值 |
|----|-----|
| Session | `ebc333d4-f75c-4a7e-997b-bddede441075` |
| Owner | `user:CamThink` |
| Preview | `【device 200366 …】最近一张照片发送给【device 20545d67 …】显示` |
| 消息数 | 15（采样时末条仍是用户「继续」；摘要后曾见 `has_tools=true` 的 in-flight 回合，未见新的 End） |

**并列参照（次新 Chat）：** `1d22ea62-…`（20035E→EK410 同类意图；LFM；长回合 dump 原始 JSON；client detach；**无**本票 hard_budget 四连）。  
**Agent 轨：** 东坪山三台仍在跑，`NO DATA COLLECTED` + `Excluded vision` + 假告警/错参 Failed——见复审文档；**用户问的「最近一次对话」以 Chat `ebc333d4` 为准。**

---

## 2. 回合时间线（`ebc333d4`）

UTC 时间来自 `neomind.log.2026-11-23`；消息索引来自 offline `SessionStore::load_history`。

| 阶段 | UTC | 发生了什么 |
|------|-----|------------|
| Turn1 工具环 | 04:55–04:56 | `has_tools=true`：`device list` → `get 20545d67` → `history 200366`（**273.9KB** 进 `$cached:`）→ `get 200366` |
| Turn1 收尾 | 04:56:52 | **`hard_budget_dropped_latest_tool_feedback`** → **`no_feedback_degrade_no_tools`** → 同轮最终 LLM 调用 `message_count=3 has_tools=false` |
| End | 04:59:51 | End（5198 events）。助手声称「照片已发送」；`system_notes`：**`write-metric` 并未成功执行**，并提示用户发 continue |
| Follow-ups | 05:00–05:01 | 用户：「没有发送 重试一下」→「继续」→「咋回事？重新尝试」→「继续」。**每轮**再触发 hard_budget + degrade；`has_tools=false`；助手输出假 XML：`<tool_call>neomind extension …</arg_value></tool_call>`（**未真实调用工具**） |
| Summarize | 05:02:23 | `force=true`，`usage_ratio=0.0%`（`prompt_tokens=0 usage_known=false`）；摘要模型未过 evidence gate → **digest fallback**；`summarized_messages=7` |
| 之后 | 05:02:41+ | 日志再现 `has_tools=true`（工具恢复），但采样时 history 仍停在用户「继续」——后续完成态未落库或仍卡住 |

---

## 3. 暴露的问题（按优先级）

### 3.1 上下文硬预算 → 剥工具（解释「后续都有问题」）

- **机制：** `streaming/context.rs` 在硬上限内 eviction 时丢掉「最新完整 tool 交互」→ 打 `hard_budget_dropped_latest_tool_feedback`；`llm.rs` 的 `no_feedback_degrade_no_tools` **拒绝带工具组装**，防止模型在看不见 tool 结果时继续乱调。  
- **触发物：** Turn1 塞进超大 `device list` / `device get` JSON + `history` 大结果（cached 指针仍占位）；follow-up 仍带着这份历史，**每一轮**再次 drop latest feedback。  
- **用户可见：** 重试/继续时模型只会「演戏」写假 tool_call 文本，**照片始终发不出去**。  
- **不是：** `ZeroBudget` / adaptive `skip_tools=true`（今日 0 次）。

### 3.2 Turn1 假成功叙事（与门禁无关的 honesty bug）

- `system_notes` 明确：`neomind device write-metric` **未执行**。  
- 助手正文仍写「The most recent photo … was sent」。  
- 同类「说做成了但副作用没有」——Chat 侧，不是 vision Completed 门禁。

### 3.3 任务未配对（能力/路由）

- 用户意图：PIR 最近一张图 → EK410 墨水屏 **显示**。  
- 实际路径：只 `list/get/history`，未走到可用的 `device control` / 写屏命令；history 大结果未 `file_read` 回读缓存。  
- 无工具轮次里还去「搜 extension photo send」——幻觉入口。

### 3.4 摘要质量差

- 强制摘要时 `usage_known=false`；digest 里助手要点为空、只记到 `shell`。  
- 压缩后工具虽可恢复，但 **任务状态（未发送）** 在摘要里几乎丢失。

### 3.5 次新会话 `1d22ea62`（对照）

- 同类「图发到 EK410」；LFM；错 `device get`/`control` 缺参；最终把 **4 万字符** shell 输出塞进助手回复；client detach。  
- **无** hard_budget 四连 → 说明「后续坏」不是凡长对话皆预算，而是 **ebc333d4 的大 tool 结果 + degrade 路径** 更致命。

### 3.6 同期 Agent 轨（非本 Chat，但同日现场）

- 200366 等：`NO DATA COLLECTED`、`Excluded vision`、无赌博却拼 `message send`、缺 `--title/--body` → Failed；与 [`t4-three-agent-review.md`](t4-three-agent-review.md) 一致。  
- Chat 剥工具与 Agent 假告警是 **两条线**；用户「对话有问题」以 Chat 为准。

---

## 4. 是否「上下文满了解释了一切」？

| 说法 | 判定 |
|------|------|
| 上下文压力导致后续轮异常 | **是**（hard_budget → strip tools → 假 tool_call） |
| 就是旧的 ZeroBudget / available_tokens=0 | **否** |
| 只有上下文问题，Turn1 其实发成功了 | **否**（Turn1 已 honesty 失败；notes 已写明未写度量） |
| 摘要后应自动好 | **未证实**（工具标志恢复，但采样时未见成功 End / 未见真正发图） |

---

## 5. 建议产品跟进（Chat，非 vision 计划主体）

1. **大 tool 结果：** `device list/get/history` 默认摘要/分页；已 `$cached:` 的勿再把巨型 sibling tool 结果留在硬窗口。  
2. **degrade UX：** `no_feedback_degrade_no_tools` 时对用户可见 system_note（「上下文过满，已暂时禁用工具，请开新会话或压缩」），禁止静默假 `<tool_call>`。  
3. **写操作 honesty：** `system_notes` 已否写时，禁止正文宣称已发送（可与 agent 告警语义门禁同族）。  
4. **EK410 显示图：** 明确 CLI/skill 路径（control 命令 + 图片 URL），减少 list→history 绕路。

---

## 6. 方法 / Rails

- Offline：`cp sessions.redb` → 本地 `SessionStore::open_isolated` + `list_sessions` / `load_history`（API key 无法读 JWT 会话 history）。  
- 日志：`/var/lib/neomind/data/logs/neomind.log.2026-11-23`。  
- 未 `neomind serve` 重启；未改 agent/prompt；未 push。曾尝试 `neomind login` 列 agent 失败（未取得可用凭据）；agent 证据改走日志。
