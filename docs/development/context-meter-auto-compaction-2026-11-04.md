# 上下文圆环计量与自动压缩修复（2026-11-04）

用户报告两个现象：前端 chat 页上下文圆环数字经常涨到容量数倍（首轮对话最明显）；上下文超出后没有自动压缩。排查结论是两个独立缺陷，均已修复（Mac 主仓库工作区，未提交、未部署）。

## 缺陷一：圆环口径错误（UI 侧为主）

旧实现把「执行过程累计量」当「上下文占用」显示：流式期间每轮的 thinking、工具参数、工具结果全部加进圆环，这是 ReAct 多轮执行的总量，不等于任何一次模型请求的实际占用；首次对话没有后端实测值时用整段聊天记录做字符估算，误差最大，可显示为容量的数倍。

修复：
1. 新增 `web/src/lib/contextUsage.ts`：圆环只显示后端最近一次请求的实测值；无实测值时显示等待态（"—"），不再用聊天记录估算占用。
2. 后端在每次模型请求结束后上报 `ContextUsage` 快照（promptTokens、分项估算、maxContext），前端 `useChatStream` 订阅该事件；会话切换/压缩后旧测量值作废，等待下一次实测。
3. 分项（系统提示、工具定义）是模板估算值而非 provider 实测：真实重放中首轮快照出现 system+tools 估算合计大于实测总量的情况。分项与总量不一致时分项显示为未知（"—"），保留可信的总量；不展示不可能的分解。
4. 悬浮卡片文案区分两种口径：「最近一次模型请求的上下文；不累加整个执行过程」（实测）与「等待模型请求的上下文实测值」（等待态）。

## 缺陷二：自动压缩的三个断点（后端）

1. 摘要范围保护最近 3 条用户消息（`RECENT_PROTECTED_USER_MSGS`），首轮只有 1 条用户消息时即使跟随大量工具历史也没有可摘要范围。修复：`summary_protection_floor` 允许短会话（用户消息 ≤3）摘要其工具历史；上下文构建器（`build_context_window_with_summary_tracked`）把最近 3 条用户消息原文保留在请求上下文中（即使落在已摘要前缀内），仍计入窗口预算；纯用户前缀（无 assistant/tool）不产生假摘要——`planned_summary_target` 保证摘要必须覆盖至少一条可释放的执行消息。
2. 裁剪信号断点：请求进入 LLM 前上下文构建器已按预算裁掉历史，但这一步从未上报；摘要器看到的是裁剪后的请求，用量比例可能一直低于 60% 阈值而永远跳过。修复：`build_for_llm` 在窗口压力时调用 `mark_history_truncated`，End 事件带出 `historyTruncated`，`trigger_summarization` 已有的 truncated 旁路（绕过 60% 阈值）得以生效。一次裁剪最多触发一次摘要（take 语义）。
3. 无完成反馈：后台摘要落库后前端无从知晓。修复：摘要成功后通过 WS 推送 `ContextCompacted`（含 summarizedMessages/fallbackUsed）；前端 toast 提示并作废旧测量值。

行为区分：长任务执行中按预算缩减工具上下文（请求侧）；任务结束后后台把历史整理为可复用摘要（存储侧）。不会等圆环「超限」才处理，存储的聊天历史不被压缩删除（服务级测试断言 13 条历史完整保留、首条用户原话不变）。

## 验证

- 前端：vitest 340/340（45 个文件，含 contextUsage 5 项：不一致分项隐藏、实测占用、首轮等待、压缩后新低值生效、无效容量回退）；`tsc --noEmit` 无错误。
- 后端：`cargo test --workspace` 3743 通过 / 0 失败（/tmp/neomind-context-fix/workspace-final.log）；本轮复验相关子集：neomind-api 6 项（含 `first_turn_pressure_writes_automatic_summary_below_usage_threshold`：首轮工具压力下摘要自动落库）、neomind-agent 3 项（含 `first_turn_window_pressure_is_forwarded_to_summary_trigger`、`compacted_prefix_keeps_recent_user_words_verbatim`）全过。
- 真实任务重放（/tmp/neomind-context-fix/replay/）：多步骤中文任务（查找有上传图片的 302 设备并分析图片），198.463s，18 次工具调用（16 shell + 2 vision）。12 个请求快照最高实测 24,188 / 32,768（73.8%），server 日志确认 `Triggering conversation summarization prompt_tokens=24188 max_context=32768 usage_ratio=73.8%`，此前出现 `Replaced marker section marker=chat-summary`，但时间早于本次摘要触发，不能证明新摘要被后续请求消费。重放自有 API 与 SSH 隧道均已退出（cleanup.json: api_stopped/tunnel_stopped = true）。

## 边界与非声明

- 本轮是缺陷修复与单次链路验收，不是配对评分或回归结论，不证明综合涨分。
- 圆环分项 system/tools 是模板估算；与实测总量矛盾时隐藏，不代表 provider 级精确分解。
- 会话内工具执行中的预算缩减不发 WS 事件，圆环在下一请求快照到达前可能显示上一轮值。
- 模型/服务器内容误分类（可见回复中的 </think> 泄漏）不在本轮范围，仍未修复。
- 工作区内既有无关修改（llm.rs 思考参数一致性、图片链路修复等）全部保留，未混入、未回退。
- 未提交、未推送、未部署；遵循 artifacts 目录 /tmp/neomind-context-fix/ 的 manifest。
