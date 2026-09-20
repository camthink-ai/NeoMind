# ADR-0014:LLM 能力解析链与用户覆盖优先级

## 状态
生效

## 背景
多模态支持与否取决于模型;运行时探测可能出错,用户手动覆盖必须被尊重;曾被运行时探测覆盖用户设置的真实 bug。

## 决策
优先级固定:user_override > runtime_api(Ollama /api/show)> registry(LiteLLM 2988 项)> 启发式 > false。单一入口 `supports_multimodal()`;`ensure_instance_capabilities` 只对前两种来源跳过重探测。thinking 类模型的非对话调用显式 `thinking_enabled: Some(false)` 省 token。

## 后果
+ 行为可预测、覆盖不可被降级;gotcha #7 的 token 浪费有守卫。
- 解析链长,新人需读本 ADR 才能安全改动。
