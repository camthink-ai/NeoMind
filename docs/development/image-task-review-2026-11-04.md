# 图片任务调查与首批修复（2026-11-04）

确认平台内部图片引用 /api/images/... 可直接传入 vision；无需拼域名或 curl 下载。
任意本地路径受另一套权限规则约束，shell 下载成功不能证明 vision 可访问。

本轮仅改 image_utils.rs 和 toolkit/vision.rs 的失败路径恢复提示，保持成功路径、目录限制和 SSRF 策略；新增测试先红后绿。agent lib（test-utils,llamacpp）925/0，git diff --check 通过。
日志：/tmp/neomind-image-contract-red.log、/tmp/neomind-image-contract-agent-gate.log。
补丁：/tmp/neomind-image-contract-review/input-recovery.patch。未提交、未部署。

T4 实测：运行中的 VLM 有 --mmproj；已知有效红圆 PNG 直连返回 HTTP200 且识别正确。用户给定 URL 返回 HTTP200、73737字节、JPEG头正确；同机 VLM 读取此图片返回 HTTP200，描述为白色金属天花/屋顶结构、灯具与线缆。仅为模型输出，未做人工图像真实性验收。SSH ubuntu 直接读取服务数据文件被权限拒绝，不能由此推断服务身份不可读；随后使用用户给定公开URL验证，无提权、无配置修改。

原日志的400不能归因为“未加载mmproj”或“所有VLM不可用”；实际失败调用的完整参数与后端需进一步逐条核对。

代码调查：图片扩展的内部引用失败会原样传给扩展，且数据目录来源与vision不同。重复轮保护按调用/结果指纹识别，换工具或改路径可能绕过；不能把新参数直接等同有效进展。此两项尚未修复。本轮输入错误提示不等于端到端根治，未重跑Spark任务或声称模型成绩提高。

## 实际聊天任务复现补充

2026-11-04：新 debug 二进制在隔离目录重建5台NE302设备与用户给定原图；调用T4 Spark和MiniCPM-V，未改生产数据或配置。
有效记录在 /tmp/neomind-image-task-repro/valid-startup/。101548ms，16调用（14 shell + 2 vision），正常end。第一次把99623字符的历史JSON当作image，触发本轮新错误指引；随后device get取得/api/images/020202/image_data/a533680f642074a5.jpg，第二次vision成功。最终中文描述结构、灯具、线缆，与视觉工具输出对应；未做独立人工视觉真实性验收。源码/二进制校验一致、进程已清理。

先前隔离尝试保留于父目录，约512.515秒/33调用，因启动后新增VLM未刷新工具而缺少vision，不作为输入补丁验收。零模型探针确认创建VLM返回200但工具清单仍无vision。有效运行先持久化后端再重启自有API，并在发任务前确认vision已注册。T4进程未重启。父目录failure.json属于结果保存后的日志导出TypeError，原样保留；有效运行已修正日志导出。

SOP：device get返回的内部图片引用原样传vision，无需curl/手工base64；确认VLM和工具实际可用。动态刷新仍待修。本轮是单例恢复成功，不是配对涨分或无回归结论，未提交/未部署。
