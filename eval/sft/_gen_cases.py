"""One-off: generate new eval cases to close coverage gaps (run once).

Produces eval/cases/{zh,en}/<category>/<file>.json for every uncovered
subcommand + image_edit + a few multi-step flows. Mutating verbs on unseeded
domains are written as create-then-operate (the agent creates the entity via
CLI then reads/updates/deletes it) so traces are clean successes without
needing fixture schemas. Validates each with eval.lib.validate.
"""
from __future__ import annotations
import json, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from eval.lib import validate

ROOT = Path(__file__).resolve().parents[2] / "eval" / "cases"

# Each spec: id, cat, file, workflow, desc, user_en, user_zh, state_queries(list|None)
SPECS = [
 # --- device ---
 ("device-create-explicit-id","device","device-create.json","device-management",
  "Create a device with an EXPLICIT id (fixes the create-camera case bug; covers `neomind device create --id`)",
  "Add a new device. Set its id to cam-lobby, name it 'Lobby Camera', device type ne101_camera, connect via MQTT.",
  "新增一个设备:id 设为 cam-lobby,名称「大厅摄像头」,类型 ne101_camera,用 MQTT 连接。",
  [{"type":"device_exists","params":{"id":"cam-lobby"},"expected":True}]),
 ("device-webhook-url","device","webhook-url.json","device-management",
  "Read a device's webhook ingest URL (covers `neomind device webhook-url`)",
  "What is the webhook telemetry ingest URL for device sensor-001?",
  "设备 sensor-001 的 webhook 上行采集地址是什么?",
  None),
 # --- agent ---
 ("agent-memory-read","agent","memory-read.json","agent-management",
  "Create an agent then read its memory journal (covers `neomind agent memory`)",
  "Create an agent named probe-one that checks sensor-001 every 5 minutes, then read its memory journal.",
  "创建一个名为 probe-one 的 agent,每 5 分钟检查一次 sensor-001,然后读取它的记忆日志。",
  [{"type":"agent_exists","params":{"id":"probe-one"},"expected":True}]),
 ("agent-latest-execution","agent","latest-execution.json","agent-management",
  "Create an agent, execute it, then show latest execution (covers `neomind agent latest-execution`)",
  "Create an agent named probe-two that watches sensor-001, execute it once now, then show its latest execution.",
  "创建一个名为 probe-two 的 agent 监视 sensor-001,立即执行一次,然后查看它的最近一次执行记录。",
  [{"type":"agent_exists","params":{"id":"probe-two"},"expected":True}]),
 # --- transform (create-then-operate; transform_count verifies the create) ---
 ("transform-get-detail","transform","get-detail.json","transform-management",
  "Create a transform then read its detail (covers `neomind transform get`)",
  "Create a transform named tf-avg that averages sensor-001 temperature, then show its full details.",
  "创建一个名为 tf-avg 的 transform,对 sensor-001 的温度求平均,然后查看它的完整详情。",
  [{"type":"transform_count","params":{},"expected_min":1}]),
 ("transform-enable","transform","enable.json","transform-management",
  "Create a transform then enable it (covers `neomind transform enable`)",
  "Create a transform named tf-rolling on sensor-001 humidity, then enable it.",
  "创建一个名为 tf-rolling 的 transform 处理 sensor-001 的湿度,然后启用它。",
  [{"type":"transform_count","params":{},"expected_min":1}]),
 ("transform-disable","transform","disable.json","transform-management",
  "Create a transform then disable it (covers `neomind transform disable`)",
  "Create a transform named tf-fast on sensor-001 temperature, then disable it.",
  "创建一个名为 tf-fast 的 transform 处理 sensor-001 的温度,然后禁用它。",
  [{"type":"transform_count","params":{},"expected_min":1}]),
 ("transform-metrics","transform","metrics.json","transform-management",
  "Create a transform then list its output metrics (covers `neomind transform metrics`)",
  "Create a transform named tf-out that converts sensor-001 temperature to fahrenheit, then list its output metrics.",
  "创建一个名为 tf-out 的 transform,把 sensor-001 的温度转成华氏度,然后列出它的输出指标。",
  [{"type":"transform_count","params":{},"expected_min":1}]),
 # --- message ---
 ("message-get-detail","message","get-detail.json","message-management",
  "Send a message then read it back (covers `neomind message get`)",
  "Send a message 'hello from eval' then show the details of that message.",
  "发送一条消息「hello from eval」,然后显示这条消息的详情。",
  None),
 ("message-channel-get","message","channel-get.json","message-management",
  "Create a webhook channel then read its detail (covers `neomind message channel-get`)",
  "Create a webhook channel named wh-1, then show its configuration details.",
  "创建一个名为 wh-1 的 webhook 通道,然后查看它的配置详情。",
  [{"type":"channel_exists","params":{"id":"wh-1"},"expected":True}]),
 ("message-channel-types","message","channel-types.json","message-management",
  "List supported channel types (covers `neomind message channel-types`)",
  "What notification channel types does NeoMind support?",
  "NeoMind 支持哪些通知通道类型?",
  None),
 ("message-channel-type-schema","message","channel-type-schema.json","message-management",
  "Show a channel type's config schema (covers `neomind message channel-type-schema`)",
  "Show the configuration schema for the webhook channel type.",
  "展示 webhook 通道类型的配置 schema。",
  None),
 # --- push ---
 ("push-get-detail","push","get-detail.json","push-management",
  "Create a push config then read it (covers `neomind push get`)",
  "Create a data-push config named push-1, then show its details.",
  "创建一个名为 push-1 的数据推送配置,然后查看它的详情。",
  None),
 ("push-update","push","update.json","push-management",
  "Create a push config then update it (covers `neomind push update`)",
  "Create a data-push config named push-2, then update its target endpoint to https://example.com/hook.",
  "创建一个名为 push-2 的数据推送配置,然后把它的目标端点改成 https://example.com/hook。",
  None),
 # --- widget ---
 ("widget-get-detail","widget","get-detail.json","widget-management",
  "Create a widget then read it (covers `neomind widget get`)",
  "Create a custom widget named wg-1, then show its details.",
  "创建一个名为 wg-1 的自定义 widget,然后查看它的详情。",
  None),
 ("widget-bundle","widget","bundle.json","widget-management",
  "Bundle a widget for distribution (covers `neomind widget bundle`)",
  "Create a widget named wg-bundle, then bundle it for distribution.",
  "创建一个名为 wg-bundle 的 widget,然后把它打包以便分发。",
  None),
 ("widget-market-install","widget","market-install.json","widget-management",
  "Install a widget from the market (covers `neomind widget market-install`)",
  "Install the gauge widget from the community market.",
  "从社区市场安装 gauge 这个 widget。",
  None),
 # --- connector ---
 ("connector-enable","connector","enable.json","connector-management",
  "Create a connector then enable it (covers `neomind connector enable`)",
  "Create an MQTT connector named conn-1, then enable it.",
  "创建一个名为 conn-1 的 MQTT connector,然后启用它。",
  None),
 # --- extension ---
 ("extension-validate","extension","validate.json","extension-management",
  "Validate an extension package (covers `neomind extension validate`)",
  "Validate the extension package weather.nep before installing.",
  "安装前先校验扩展包 weather.nep。",
  None),
 ("extension-install","extension","install.json","extension-management",
  "Install an extension (covers `neomind extension install`)",
  "Install the weather extension from the local package weather.nep.",
  "从本地包 weather.nep 安装 weather 扩展。",
  None),
 ("extension-uninstall","extension","uninstall.json","extension-management",
  "Uninstall an extension (covers `neomind extension uninstall`)",
  "Uninstall the weather extension.",
  "卸载 weather 扩展。",
  None),
 ("extension-create-scaffold","extension","create-scaffold.json","extension-management",
  "Scaffold a new extension (covers `neomind extension create`)",
  "Create a new extension scaffold named my-ext.",
  "创建一个名为 my-ext 的扩展脚手架。",
  None),
 ("extension-build","extension","build.json","extension-management",
  "Build an extension (covers `neomind extension build`)",
  "Build the my-ext extension into a .nep package.",
  "把 my-ext 扩展构建成 .nep 包。",
  None),
 ("extension-market-install","extension","market-install.json","extension-management",
  "Install an extension from the market (covers `neomind extension market-install`)",
  "Install the ocr extension from the official market.",
  "从官方市场安装 ocr 扩展。",
  None),
 ("extension-reload","extension","reload.json","extension-management",
  "Reload an extension (covers `neomind extension reload`)",
  "Reload the weather extension after updating its config.",
  "更新配置后重新加载 weather 扩展。",
  None),
 ("extension-config","extension","config.json","extension-management",
  "Show/set extension config (covers `neomind extension config`)",
  "Show the current configuration of the weather extension.",
  "查看 weather 扩展的当前配置。",
  None),
 # --- llm ---
 ("llm-update","llm","update.json","llm-management",
  "Update an LLM backend (covers `neomind llm update`)",
  "Update the active LLM backend's display name to 'primary'.",
  "把当前激活的 LLM 后端显示名称改成「primary」。",
  None),
 ("llm-delete","llm","delete.json","llm-management",
  "Delete an LLM backend (covers `neomind llm delete`)",
  "Delete the LLM backend named 'old-backend'.",
  "删除名为「old-backend」的 LLM 后端。",
  None),
 # --- image_edit tool (the missing 7th tool) ---
 ("tools-image-edit","tools","image-edit.json","tool-use",
  "Exercise the image_edit tool (covers the only agent tool with zero golden demos)",
  "Take the bound camera image and crop it to the center square, then describe the result.",
  "把绑定的摄像头图像裁剪成中心正方形,然后描述结果。",
  None),
 # --- multi-step cross-domain onboarding (high pedagogical value) ---
 ("onboarding-temp-monitor","system","onboarding-temp-monitor.json","onboarding",
  "End-to-end: create device + rule + dashboard for a temperature monitor (multi-step cross-domain)",
  "Set up temperature monitoring for a new camera: add device temp-cam (ne101_camera, MQTT), create a rule 'temp-high' that notifies when temp-cam values.temperature > 35, and create a dashboard 'Temp Overview' showing temp-cam temperature.",
  "为一个新摄像头搭建温度监控:新增设备 temp-cam(ne101_camera,MQTT),创建规则「temp-high」在 temp-cam 的 values.temperature > 35 时通知,再创建看板「温度总览」展示 temp-cam 的温度。",
  [{"type":"device_exists","params":{"id":"temp-cam"},"expected":True},
   {"type":"rule_exists","params":{"id":"temp-high"},"expected":True},
   {"type":"dashboard_exists","params":{"id":"Temp Overview"},"expected":True}]),
 ("agent-full-lifecycle","agent","full-lifecycle.json","agent-management",
  "Create an agent, execute it, then delete it (multi-step lifecycle)",
  "Create an agent named lifecycle-probe that checks sensor-001 every 10 minutes, execute it once, then delete it.",
  "创建一个名为 lifecycle-probe 的 agent 每 10 分钟检查 sensor-001,执行一次,然后删除它。",
  None),
]

def build(spec, lang):
    id_,cat,file_,wf,desc,uen,uzh,sq = spec
    user = uen if lang=="en" else uzh
    c = {
      "id": id_, "lang": lang, "category": cat, "workflow": wf,
      "scenario_type": "single_turn",
      "description": desc,
      "setup": {"fixture": "seed-default"},
      "turns": [{"user": user}],
      "applies": ["tool_accuracy","task_completion","language_adherence"],
      "expectations": {"per_turn":[f"Turn 1: {desc}"], "overall": desc},
    }
    if sq: c["state_queries"] = sq
    return c

written=0; errors=0
for spec in SPECS:
    for lang in ("en","zh"):
        c = build(spec, lang)
        errs = validate.validate_case(c)
        if errs:
            print(f"INVALID {lang}/{spec[2]}: {errs}"); errors+=1; continue
        out = ROOT/lang/spec[1]/spec[2]
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(c, ensure_ascii=False, indent=2)+"\n")
        written+=1
print(f"\nwrote {written} case files ({len(SPECS)} specs x 2 langs), {errors} invalid")
