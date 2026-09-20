# ADR-0009:规则引擎 v2 纯 JSON 条件树

## 状态
生效(取代 v1 DSL)

## 背景
v1 用 DSL 文本 + 解析器,解析边界 bug 多、UI 无法可靠双向编辑。

## 决策
v2 条件是递归 JSON(Comparison | Range | Logical),动作是 Notify | Execute | TriggerAgent;`dsl_preview` 只是自动生成的只读展示文本,不再是数据源。

## 后果
+ UI 与存储同构,可视化编辑器直接映射;无解析失败态。
- 文本党用户失去手写规则的入口(可用 CLI 补)。
