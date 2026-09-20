# ADR-0015:聊天共享流状态机 useChatStream + 会话事件过滤

## 状态
生效(2026-09)

## 背景
聊天页与侧栏各自维护 WS 事件状态机,已语义分叉("pair fix"税:每个流式 bug 修两遍,经常只修一边);且两视图共用一条连接,事件不带过滤时互相串台(幽灵气泡)。

## 决策
单一 `useChatStream`(hooks/useChatStream.ts)承担全部事件解释,以聊天页语义为超集;视图差异走回调(持久化/对账/错误策略)。后端事件本就携带 sessionId,机器按 `options.sessionId` 过滤,`session_created/switched` 控制面事件豁免。高频 Content/Thinking 帧节流(rAF,epoch 防陈旧回刷)。

## 后果
+ 修一处即两视图生效;串台类问题在机器层根除;边缘设备流式渲染不再逐 token 重解析。
- 视图若需要新事件语义,先改机器再透出回调 —— 多一层间接。
