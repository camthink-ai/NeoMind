# ADR-0012:前端设计令牌体系

## 状态
生效

## 背景
多页面多人长期演进,颜色/字号不收敛会视觉失控;深浅主题需要系统性方案。

## 决策
全部颜色经 OKLCH CSS 变量定义(`:root`/`.dark`),映射为 Tailwind 语义令牌(text-success/bg-error-light…);禁止原生 Tailwind 色板、禁止 CSS 变量色上 `/opacity` 修饰;图标仅 lucide 经统一映射;全部文案经 i18n。34 节 DESIGN_SPEC.md 为准。

## 后果
+ 主题一致性与可审计性;暗色模式零散点成本。
- 表达力受限:临时色需新令牌而不是随手写值 —— 这是特性不是缺陷。
