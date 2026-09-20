# ADR-0007:双记忆系统刻意不统一

## 状态
生效

## 背景
调度型 agent(定时/事件触发)和聊天会话对"记忆"的需求完全不同:前者要可审计的执行日志与知识文件,后者要轻量的用户画像与对话历史。

## 决策
维持两套:调度 = `AgentMemory`(journal/knowledge_files/user_messages,有截断纪律:outcome 300 字/action 150×5);聊天 = `MemorySnapshot`(user.md/knowledge.md)+ 会话历史。明确不合并。

## 后果
+ 各自语义清晰、截断策略独立调优;避免"统一抽象两头不讨好"。
- 概念成本:新人常问为什么不统一 —— 本 ADR 即答案。
