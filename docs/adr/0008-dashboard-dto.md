# ADR-0008:仪表盘 DTO 转换层

## 状态
生效

## 背景
后端 snake_case、前端 camelCase;早期多次因跳过转换层导致仪表盘静默损坏(加载"成功"但字段全 undefined)。

## 决策
所有仪表盘 API 响应必须经 `fromDashboardDTO()`(`web/src/store/persistence/types.ts`)。这是 CLAUDE.md 12 条 gotcha 之首,违者静默出 bug。

## 后果
+ 单点转换,字段对齐可测试(persistence-dto.test.ts)。
- 新增字段的仪式感成本:两端都要动,漏掉不报错 —— 靠测试与审查兜底。
