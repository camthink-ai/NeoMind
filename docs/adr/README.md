# 架构决策记录(ADR)索引

记录 NeoMind 的关键架构决策:**做了什么选择、为什么、付出了什么代价**。
CLAUDE.md/CHANGELOG 记录"是什么/改了什么",这里补"当时的权衡"。
新决策请追加编号;推翻旧决策时保留原文并将状态改为"已取代于 ADR-xxx"。

| # | 决策 | 状态 |
|---|------|------|
| 0001 | EventBus 发布/订阅作为子系统脊柱 | 生效 |
| 0002 | redb 按领域分库,不用 SQLite/Postgres | 生效 |
| 0003 | 边缘单机部署假设(内置 MQTT broker、无外部依赖) | 生效 |
| 0004 | 扩展双模隔离:原生进程 + WASM 沙箱 | 生效 |
| 0005 | `.nep` ABI 版本锁(当前 3)与跨仓库发布协调 | 生效 |
| 0006 | CLI 进程内分发:shell 工具拦截 `neomind` 命令 | 生效 |
| 0007 | 双记忆系统刻意不统一(调度 AgentMemory vs 聊天 MemorySnapshot) | 生效 |
| 0008 | 仪表盘 DTO 转换层(snake_case ↔ camelCase 必经 fromDashboardDTO) | 生效 |
| 0009 | 规则引擎 v2:纯 JSON 条件树,弃用 DSL 解析器 | 生效 |
| 0010 | 设备四态连接模型(transport 与数据活性独立跟踪) | 生效 |
| 0011 | 设备类型 = JSON 模板(DeviceTypes 仓库),不是代码 | 生效 |
| 0012 | 前端设计令牌体系(OKLCH CSS 变量,禁原生 Tailwind 色) | 生效 |
| 0013 | OpenAPI 全覆盖 + 路由漂移守卫进 CI | 生效 |
| 0014 | LLM 能力解析链与用户覆盖优先级 | 生效 |
| 0015 | 聊天共享流状态机 useChatStream + 会话事件过滤(2026-09) | 生效 |
| 0016 | 会话历史分页:raw-index 游标 + 碎片守卫(2026-09) | 生效 |
| 0017 | BuildCard/build_meta 移除(2026-09,投机功能减法) | 生效 |
| 0018 | WASM 三层资源限制:fuel + 模块大小 + ResourceLimiter(2026-09) | 生效 |
