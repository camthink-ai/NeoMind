# 设备诊断回复的语义验收（试验性）

原有命令、状态评分继续独立使用。本工具只读 `cases.jsonl`，另行检查最终回复是否超出工具证据，不修改原始记录或评分。目前仅支持直接的 `neomind device get/history/list` 证据；其他调用会弃权。

先校准评审模型，再评审实际记录。Spark 是被测模型，评审模型单独配置。API key 通过受控环境变量 `NEOMIND_SEMANTIC_JUDGE_API_KEY` 或安全终端的 `--ask-key` 输入，不写入产物。不要在命令参数中提供密钥。

```bash
./.venv/bin/python eval/semantic_review.py \
  --calibration eval/semantic/calibration/device_grounding.json \
  --run-dir /tmp/semantic-calibration-new \
  --judge-url http://192.168.93.3:3000 --judge-model glm-5.3-flash --ask-key

./.venv/bin/python eval/semantic_review.py \
  --records /path/to/new-run/cases.jsonl \
  --run-dir /tmp/semantic-review-new \
  --judge-url http://192.168.93.3:3000 --judge-model glm-5.3-flash --ask-key
```

输出目录必须不存在。`manifest.json` 固定输入、评审配置和实现哈希；`reviews.jsonl` 保存证据包、判断与引用；`summary.json` 汇总结果。校准要求全部预设参考标注匹配；未通过时不能把该评审器作为验收门槛。随附的20个样例及标签由本轮实现者编写，尚未经过独立人工校对；校准样本本身不构成独立的泛化验证，应另做盲审与留出案例。

模型从 `citation_catalog` 选择 `e0` 等证据编号；代码独立重建目录，将编号解析为实际 JSON 路径和值。未知编号、自由生成的路径和值都拒绝，原始模型输出和解析后的判断分别保留。引用正确仍不保证断言正确。

- `PASS`：本次评审认为所有事实断言均受证据支持。
- `FAIL`：发现超出证据或与证据矛盾的断言。
- `ABSTAIN`：输入、完成状态、证据、引用或评审输出不足以可靠判断。不能计作通过。

校准退出码为 0（全部匹配）或 2（未接受）；实际评审为 0（全部通过）、1（至少一个失败）、2（未完成或弃权）。未配置评审模型时只生成弃权结果，便于检查证据提取流程。

新采集记录通过 WebSocket 的真实结束事件记录 `generation_complete`。旧记录缺此字段会弃权，不能补写为真以绕过检查。显式截断同样弃权；框架结束事件不保证底层模型未触及输出上限，后端提供完成原因时仍须核实。

证据边界包括：当前连接状态不等于从未连接；`last_seen` 不等于断线时刻；某个指标、窗口或分页为空不等于设备从未生成数据；请求失败不等于结果为零。允许模型用自然语言提出明确标注的假设和下一步建议。

本地检查验证引用真实、类型匹配及逐行覆盖，不能证明自然语言蕴含关系，也不能保证评审模型正确识别了每个分句。20 个校准样例与引用检查均不能代替人工审查。当前工具不会自动改写回复、重试直至通过或改变生产行为。
