# Platform Defect Log（平台缺陷记录）

按实测验证过的缺陷条目。每条含：现象、复现、根因定位、影响、建议修复。
状态：`open` / `fixed` / `wontfix(原因)`。

---

## DEF-001 — MQTT 上行遥测的 `timestamp` 字段被静默忽略【fixed 2026-09-05】

- **现象**：通过 MQTT 上行（`device/{type}/{id}/uplink`）携带 `timestamp` 字段
  回灌历史数据，落库时间戳一律为**服务器当前时间**，客户端时间被丢弃，
  **无任何报错或日志提示**。
- **复现**（2026-09-05，本机 release 0.9.23）：
  - MQTT 发布 `{timestamp: <2小时前>, temperature: 88.8}` → 落库 **0.0h 前**（服务器时间）；
  - 对照：同一设备 webhook 路径 POST `{timestamp: <2小时前>, data:{temperature:99.9}}`
    → 落库 **2.0h 前**（正确尊重客户端时间）。
- **根因定位**：webhook 适配器尊重载荷时间（`crates/neomind-devices/src/adapters/webhook.rs:389`
  `payload.timestamp.unwrap_or_else(now)`）；MQTT 适配器的上行处理
  （`crates/neomind-devices/src/adapters/mqtt.rs` 上行→指标构造路径）未读取载荷
  时间字段，直接取服务器时间。两条写入路径行为分叉。
- **影响**：历史数据回灌（断网补传、离线设备补录）在 MQTT 路径完全不可用，
  且静默失败——数据"成功"入库但时间错位，图表/聚合/规则全部错排。
- **修复**（2026-09-05，已部署验证）：MQTT 适配器两条 JSON 上行分支新增
  `extract_client_timestamp()`（识别 `timestamp/ts/ts_ms/ts_ns/time`，秒/毫秒/
  纳秒量级自适应，拒未来 >5min）；写库 DataPoint 与 DeviceEvent 时间戳同源对齐
  （否则 service.rs 事件层二次写库会以不同时间戳再写一条，产生双条目——
  该覆盖去重机制依赖两次写同时间戳）。裸值分支（非 JSON 载荷）无时间字段可读，
  维持服务器时间。终验：回灌 22.2 → 恰 1 条 @2.00h；实时 11.1 → 恰 1 条 @0.00h；
  webhook 路径回归正常；152+5 单测全过。

## DEF-002 — `/api/telemetry` 全局查询必填参数与文档/直觉不符【fixed 2026-09-05】

- **现象**：`GET /api/telemetry` 实际 **`source` 与 `metric` 均为硬必填**
  （缺一即 422：`missing field source/metric`）。但公开资料（前端 api.ts 的
  注释、swagger 缺失、wiki 未覆盖）未清晰声明这一点，调用方极易以为可全局
  查询或按 source 聚合。
- **复现**：无参数 → `missing field source`；仅 `source` → `missing field metric`；
  `source+metric` → 200 正常。
- **影响**：主要坑外部集成方与 AI 辅助编码（按文档猜参数）。属可用性缺陷非
  功能缺陷。
- **修复**（2026-09-05，已部署验证）：参数改 Option + 处理器显式校验，缺参时
  422 错误体自带字段名、格式示例（`device:sensor1`/`extension:weather`）与可
  选项清单；正常查询路径回归通过。

---

## 验证环境与工具

- 实测平台：NeoMind 0.9.23（release, macOS），数据目录 /tmp/neomind-smoke
- 复现脚本：`bench/mqtt-devices.mjs`（MQTT 路径）、curl（webhook/查询路径）
- 相关既有记录：图片 I/O 与查询争抢为**硬件基座问题**（eMMC/SD 饱和），
  服务器 NVMe 下不复现（8MB/s 真实写盘仅致查询 p50 5→12ms）——归入部署
  规范而非缺陷。
