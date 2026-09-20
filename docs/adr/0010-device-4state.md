# ADR-0010:设备四态连接模型

## 状态
生效

## 背景
"在线/离线"二义:MQTT 连着但半天没数据(connectedIdle)、连接断了但数据还在缓冲 —— 运维需要区分"断连"与"数据停更"。

## 决策
transport 连接(rmqtt 钩子)与数据活性(last telemetry vs `effective_offline_timeout`:设备>模板>全局)独立跟踪,组合出 online/connectedIdle/offline/disconnected 四态。前端 `getDeviceState()` 兼容旧后端三态回退。

## 后果
+ 状态语义诚实;每设备超时可独立配置。
- 实现必须处处用 `effective_offline_timeout`,直接用全局值是已知 gotcha(#4)。
