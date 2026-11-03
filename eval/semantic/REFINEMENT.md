# 自动对照与有界自检

这套工具用于离线筛选，不能把格式正确、评审弃权或继承的任务状态当作分数提升。生产路径不自动启用自改写，也不自动发布、提交或更新基线。

## 一次改写、独立评审、自动比较

先保存生产回归产生的原始 `cases.jsonl`。对完整且证据提取范围覆盖的记录，最多增加一次 Spark 调用；其他记录保留原答并明确记录跳过原因。

```bash
./.venv/bin/python eval/refine_answers.py \
  --records /path/to/cases.jsonl --run-dir /tmp/refinement-new \
  --endpoint http://127.0.0.1:18080 --model Spark-X2.6-1.7B-Q4_K_M.gguf \
  --thinking off --structured-output schema

./.venv/bin/python eval/evaluate_refinement.py \
  --attempts /tmp/refinement-new/attempts.jsonl \
  --calibration-dir /path/to/accepted-calibration \
  --run-dir /tmp/refinement-reviewed-new \
  --judge-url http://192.168.93.3:3000 --judge-model glm-5.3-flash --ask-key
```

第一步要求已启动显式指定的本机推理服务，不自行查找模型或占用端口。第二步要求评审模型、采样、规则及证据提取实现符合已通过的参考校准；请求时限可以增加并记录。参考标签由开发者预设，不是独立人工准确率验证。

模型先输出简短事实及证据编号，再输出自然语言回复。Schema 只限制 JSON 结构及引用编号；事实是否成立仍由独立评审检查。使用 token 和请求时限限制预算，不用字符串长度约束在半句话处强制关闭答案。thinking 模式和结构化输出模式必须记录，不把其变化隐藏为同参数对比。

改写发生异常、输出截断、JSON/引用无效、可见控制标记、明显语言不符或疑似半句话时，保留原始回答并记录失败。语言和结束标点仅是保守的交付检查，可能产生误报，也不证明回复完整或事实正确。

`attempts.jsonl` 同时保留原答、候选、模型调用和原因；`baseline.jsonl` 与 `candidate.jsonl` 用于对齐评审。`manifest.json` 和 implementation 快照固定输入、实现和配置。评审不会把分组标签、成绩或修改建议反馈给生成模型，不执行“重试直到通过”。

`evaluate_refinement.py` 以交错顺序派发原答/候选的盲评，保存每项原始结果，再自动比较。比较器验证答案与证据哈希、实体记录、规则版本、工具调用与状态没有被替换；独立命令行比较还检查两侧评审 manifest 一致。

退出码 0 仅表示小样本筛选有收益且没有已观察到的退化、弃权、生成失败或交付失败；退出码 2 表示没有通过筛选。任何结果的 `production_acceptance` 都是 false。应通过更多留出案例和完整生产链路回归，再考虑生产接入。

## 本轮已发现的边界

默认 thinking 的 JSON 自改写四次均触及 3072 token 上限；关闭 thinking 后四次都生成了无效 JSON。Schema 解码解决了格式问题，但仍观察到语言切换、工具标记与半句话，交付检查拒绝全部四个完整候选。这些候选不进入生产，也不能声称提高了事实准确率。

真实 b12024 Spark 探针同时证实 `chat_template_kwargs.enable_thinking=false` 有效：同一简短请求默认返回 reasoning 内容，关闭后 reasoning 字符数为零。NeoMind llama.cpp 适配器此前丢弃这个请求设置。本轮修复显式布尔/effort 的转发，未设置时保留服务器默认，不修改全局默认或宣称所有模型支持关闭思考。

适配器修复由真实 HTTP 捕获测试覆盖流式及非流式路径。后续使用同样显式关闭的配置，对修复前后二进制交错评测，并记录真实请求参数。对应结果应引用本轮实验报告；小模型关闭 thinking 是否有利需要按任务验证，不能预设。
