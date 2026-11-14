# G2/G3：跨轮任务状态与循环止损诚实性（离线切片）

Date: 2026-11-14. Branch `1.3.0`. Tip at commit of this slice（基于 `1f7b4ba3`）. **不 push**. 138 未触碰。

## 范围（§12）

- **G2**：目标 / 对象 / 内容 / 已证实步骤 / 下一步及来源；纠正覆盖旧目标；取消停止；继续不重放已成功写入；完成只认证据 id。
- **G3**：无进展重复 → 强制止循环；`counts_as_task_success = false`（止循环 ≠ 任务成功）。

## 交付

| 项 | 路径 |
|----|------|
| 模块 | `crates/neomind-agent/src/agent/task_continuity.rs` |
| 会话挂载 | `AgentInternalState.task_state`；`push_message` 解析 继续/取消/纠正 |
| 循环挂钩 | `stream_core` 在 identical-rounds 止损时 `task_state.halt(...)` |
| Manifest | `eval/manifests/1.3.0-g2-g3-task-continuity.json` |
| 预注册 | `eval/acceptance/g2-g3-continuity-scenarios.json`（live 仍 blocked） |

## 测试

```bash
export PATH="$HOME/.rustup/toolchains/1.92.0-aarch64-apple-darwin/bin:$PATH"
cargo test -p neomind-agent --lib task_continuity --locked
```

期望 **9/9**。

## Live 阻塞

中英配对 / Spark 实跑仍被 PID 89912 `:29431` 占用；本片仅离线确定性 + 预注册，无分数宣称。

## Next（§12）

1. 138 结束后：天气→EK410 配对 live + 本 G2/G3 场景探针  
2. 将 `may_execute_write` 接到具体 mutating tool 执行前（本片 API 已备）  
3. C / D / E / F 清单项  
