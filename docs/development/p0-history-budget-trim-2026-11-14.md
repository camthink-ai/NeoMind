# P0：上下文裁剪 — 工具成组保留 / 零预算不续跑早期用户（2026-11-14）

Branch `1.3.0`. Tip at commit of this slice. **不 push**. 138（PID 89912 / `:29431`）未触碰。

## 问题（§12 P0）

`llm.rs` 内层按单消息逆序裁剪；短历史无条件插回首个 user，即使 `available_tokens == 0`。外层 `streaming/context.rs` 保护后仍可能被二次裁剪。生产形态（T4 1.2.11 会话 `6621a013-…`）：大 list 返回后历史 9→1、`available_tokens=0`、实际请求 `message_count≈3`，随后重复查询。

## 本片修复（G1 机制一轮）

1. 新模块 `crates/neomind-agent/src/agent/history_select.rs`
   - 将 **assistant 工具调用 + 连续 tool 返回** 划为原子组
   - 从最近组向前填充预算
   - `available_tokens == 0` → 保留空历史（`ZeroBudget`），**禁止**只插早期 user
   - 最新组过大时压缩 tool 正文（保留 device id / error / status / ACK 针），再决定是否丢弃
2. `LlmInterface::select_history_for_prompt` 接到全部 4 条 chat 路径（text / multimodal / stream）
3. 外层 `build_context_window_with_config` 改为按组选择；`evict_until_fits` 按组驱逐，避免孤儿 tool

## 证据

| 项 | 路径 |
|----|------|
| 确定性单测 | `cargo test -p neomind-agent --lib history_select`（6） |
| 外层窗口回归 | `cargo test -p neomind-agent --lib agent::streaming::context`（4） |
| 修改前后回放 | `eval/fixtures/history-budget-p0/t4-shaped-zero-budget-replay.json` |
| Manifest | `eval/manifests/1.3.0-p0-history-budget-trim.json` |

回放断言：legacy 在预算 0 时仍保留早期 user（`message_count≈3`）；新逻辑保留 0 条历史（system + current user only），并标记截断。

## 未做 / next（§12）

- 中英配对 32×2（天气→EK410 八场景）— **next**，本片范围外
- G0 完整脱敏实请求归档（生产未留全文）— 仍缺，本片用重构 fixture
- G2 跨轮任务状态 / G3 循环兜底 — 另轮
- Skill 总量上限（P1）— 另轮

## 复现

```bash
cd "/Users/shenmingming/CamThink Project/NeoMind"
export PATH="$HOME/.rustup/toolchains/1.92.0-aarch64-apple-darwin/bin:$PATH"
cargo test -p neomind-agent --lib history_select --locked
cargo test -p neomind-agent --lib agent::streaming::context --locked
python3 -c "import json;print(json.load(open('eval/fixtures/history-budget-p0/t4-shaped-zero-budget-replay.json'))['after']['kept_roles'])"
```
