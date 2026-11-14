# G0：脱敏请求复现与根因定位（离线切片）

Date: 2026-11-14. Branch `1.3.0`. Tip at commit of this slice（基于 `77be53f8`）. **不 push**. 138（PID 89912 / `:29431`）未触碰。

## 范围

§12 未勾选第一项 **G0**。生产会话 `6621a013-…` 未留完整请求正文；本片在 tip 上用 **T4 形态 fixture + 真实 `chat_stream` 装配路径** 产出脱敏归档，并冻结源码哈希。

## 交付

| 项 | 路径 |
|----|------|
| 脱敏模块 | `crates/neomind-agent/src/agent/prompt_capture.rs` |
| 归档 fixture | `eval/fixtures/history-budget-p0/g0-desensitized-prompt-archive.json` |
| Manifest | `eval/manifests/1.3.0-g0-desensitized-prompt-capture.json` |
| 配对预注册（next live） | `eval/acceptance/context-reliability-weather-ek410.json` |

## 根因（相对生产观察）

- 观察：大 list 返回后 `available_tokens=0`，历史 9→1，`message_count≈3`，随后重复查询。
- 机制：旧内层裁剪在预算为 0 时仍插回最早 user → system + early user + current（≈3），工具组丢失。
- Tip（`77be53f8`+）：`ZeroBudget` 保留空历史 → system + current（`message_count=2`），并标记 `history_truncated`；最近工具组成组保留（G1）。

## 测试

```bash
export PATH="$HOME/.rustup/toolchains/1.92.0-aarch64-apple-darwin/bin:$PATH"
cargo test -p neomind-agent --lib prompt_capture --locked
cargo test -p neomind-agent --lib prompt_capture --features test-utils --locked
```

期望：无 feature 3/3；`test-utils` 4/4（含 LLM 装配路径捕获）。

## 阻塞（live）

| 项 | 状态 |
|----|------|
| 生产原文请求体 | 仍不可得（从未归档） |
| Spark 实跑捕获 / 中英配对 32×2 | **blocked** — 138 占用 `:29431` |
| G2/G3 | 未开始（等 G0/配对证据） |

## Next（§12）

1. 138 结束后：隔离 llama（如 `:29432`）跑 `context-reliability-weather-ek410` 两臂 64 trial  
2. G2 跨轮任务状态（有配对证据后）  
3. G3 循环兜底  
