# 评估数据存档 — 2026-11-01/02（Mac, Spark/LFM3, matched-params 系列）

## 数据目录状态
- eval/runs/mini-spark-x26-17b-think — 1.2.3 基线 88/138=63.8%（完好）
- eval/runs/mac-125rel-lfm3-official-138 — LFM3 官方参 41/138=29.7%（完好）
- eval/runs/mac-nudge3-full-138 — nudge1→3 全量 68/138=49.3%（完好）
- eval/runs/mac-125rel-nudge3-lost20 — 丢失20用例小集（完好）
- eval/runs/mac-125rel-123params-138 — **数据丢失**（脚本误启动截断账本与日志）；
  结论以本文件固化值为准：**70/138=50.7%（en 50.7/zh 50.7），vs 1.2.3 配对
  63/7/25/43，McNemar p=0.0021**；丢失 25 条清单=20 用例（/tmp/lost20-set.txt 副本
  在本文件末尾），形态：13 cmd+sq 双失 / 10 命令未完成 / 2 断言未过；7 条 <90s 收尾
- eval/runs/mac-125rel-neverrep-138 — NEVER-repeat 行恢复版 A/B（进行中）

## 根因取证结论
1. 回归非护栏限额（nudge3 A/B 49.3% vs 50.7%，p=0.83；账本 guards 仅 7/138 触发）
2. 失败命令 stdout 完整回灌（in-process 序列化+help 附加）
3. 行为为概率性退化：同二进制同案例两次一成一败（代理复跑自纠正成功）
4. 提示差异：1.2.5 后续轮指令块删除了 1.2.3 的"NEVER call the same tool with
   the same arguments"行（并新增 ask-user 分支——保留，产品功能）
5. 修复实验：仅加回 NEVER-repeat 行（强化措辞：失败时读建议改命令）

## lost20 集合（副本）
agent-event-trigger, agent-executions, agri-bulk-irrigate-workflow,
agriculture-irrigation-solution-deploy, api-key-lifecycle, building-co2-anomaly,
config-export-validate, dashboard-components, dashboard-update-component-tweak,
deep-memory-transform, device-control-stop-pump, edge-empty-system,
extension-usage-entry, guided-mqtt-onboarding, guided-webhook-onboarding,
micro-channel-create, micro-rule-create, push-create-webhook,
transform-lifecycle, widget-full-lifecycle
