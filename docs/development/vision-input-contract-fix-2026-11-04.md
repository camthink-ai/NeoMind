# vision 输入契约修复方案（2026-11-04）

## 背景

T4 上同一条 NE302 图片分析任务（"找一台最近有上传图片的 302 设备，分析图片内容"），
Spark-1.7B（21 轮 34 工具调用）与 DeepSeek（27 轮 31 工具调用）先后失败。
两个模型能力差异明显但都没走上平台设计好的正路，判定为设计问题，不是单模型能力问题。

诊断结论（详细证据见对话记录，代码锚点如下）：

- 平台正路一步可达：`device get` 物化图片为 `/api/images/<device>/<metric>/<file>.jpg`
  相对 URL（`crates/neomind-cli-ops/src/device.rs:302` materialize_data_url），
  vision 的 `resolve_image` 分支 3（`crates/neomind-agent/src/image_utils.rs:241`）
  直接从本地 data 目录读取，不需要域名/DNS/HTTP。
  系统提示 capability_index.rs:101 也明确教了 "Pass the URL directly to vision(...)"。
- 设计断层：
  - D1 `read_local_image` 封锁 `/var/` `/tmp/` 等前缀（image_utils.rs:394-405），
    T4 部署 data_dir=/var/lib/neomind/data，平台自己的图片目录以绝对路径形态
    被平台自己的 vision 工具拒绝；shell 可读写这些路径，vision 不能，
    shell↔vision 无共享交换目录。
  - D2 相对 URL 契约脆弱：唯一可用形态是精确 `/api/images/` 前缀。
    少前缀（`images/...`）掉进 base64 兜底（image_utils.rs:339-359）报
    "Invalid base64 data: Invalid symbol 95"；完整正确的
    `http://127.0.0.1:9375/api/images/...` 被 SSRF 拦私有地址。
    三类报错都不指路。
  - D3 大图 base64 直传结构性不可行（71KB→~9.8 万字符，模型无法在单次
    工具参数中输出；DeepSeek 实际只送达 68 字符截断串，两个 VLM 均按
    损坏图片拒收）。
  - D4 三个图片工具三套参数方言（vision `image`=路径/URL/base64；
    paddle-ocr-v6 `image`=`image_base64` 别名；locate-anything 只认
    `image_base64`）。
  - D5 提示词教学没拦住偏航（两个模型都违反了 Do NOT pipe to python）。
  - D6 T4 运维：builtin-vlm-minicpm-v-4.7-1b 的 llama-server 400
    "Failed to load image or audio file"（mmproj 未加载）；
    deepseek_c82e0710 假 supports_multimodal 标记（API 拒收图片仍进候选）。
  - D7 无 SOP 沉淀通道：本次教训无法传给下次会话。

## 总原则

- 遵循迭代协议：每轮只改一个机制；先单测/集成测试，再配对筛选
  （受影响失败案例 + 正常运行对照，中英交错，双臂），通过后才考虑全量回归；
  评分规则、case 集、模型/运行时/采样全程冻结。
- 本方案目标文件（image_utils.rs、toolkit/vision.rs）已核实不在当前
  未提交的 17 文件 diff 内，改动与现有在途工作隔离。
- 不 commit/push，除非用户明示。

## Round A — 机制 1：vision 输入错误自愈（纯失败路径文案，成功路径行为零变化）

对应 D2/D5。DeepSeek 27 轮中三个关键转折点（R7 系统路径、R9 base64 兜底、
R25 私有地址）全部是"报错不说人话"直接导致偏航。

改动点：

- A1 `read_local_image` 系统路径封锁（image_utils.rs:394-405）：
  文案追加指路 —— 平台图片用 `device get` 返回的相对 URL 原样传入
  （例 `image="/api/images/<device>/<metric>/<file>.jpg"`，无需 host）；
  本地文件仅 home 目录可读（/tmp、/var、/opt 被封锁）。
  canonicalize 失败（文件不存在，Spark R19 幻觉路径）同样追加
  accepted-sources 摘要。
- A2 base64 兜底分支（image_utils.rs:339-359）：解码失败时先检查
  输入是否含 base64 字母表外字符且形如路径（含 `/` 或 `.`），是则改报
  "looks like a relative path, not base64. Relative image URLs must start
  with /api/images/ ..."，不再输出 "Invalid symbol 95, offset 19"。
- A3 私有地址拦截（image_utils.rs:492-497 与 toolkit/vision.rs:561-566）：
  文案追加 "Private/loopback addresses are blocked. This platform's own
  images don't need HTTP — pass the relative /api/images/... URL (no host)."

测试：io_tests 新增断言三类错误包含 `/api/images/` 指引；合法输入
（data URL、raw base64、公网 URL、home 文件）行为与文案完全不变；
跑全量 neomind-agent 测试，若有断言精确匹配旧错误串则更新断言
（属断言更新，非行为变化）。

筛选：新建 6-8 案例配对屏（新增 NE302 图片分析复现案例中英文版 +
正常对照），基线 = full-v2 基线线（含 7ccbffa7），候选 = +Round A。
判据：复现案例中相对 URL 直传达成率、无效重试轮数下降；对照不回归。

## Round B — 机制 2：允许平台数据目录的绝对路径形态（关闭 D1 自相矛盾）

对应 D1。分支 3 已在读 `data_dir()/images`，绝对路径被拒纯属自相矛盾。

改动点：

- `resolve_image` 分支 6（image_utils.rs:330-337）：读取前 canonicalize，
  若落在 `data_dir()/images/` 之下，复用
  `neomind_devices::image_storage::read_internal_image_url` 的全部校验
  （遍历防护、symlink 逃逸防护、大小上限、magic bytes）读取；
  否则照旧走 `read_local_image`。
- 其余系统路径封锁原样保留。

安全论证：读的仍是同一棵树、同一套校验，只是多接受"绝对路径"这一种
书写形式，不放宽任何其它目录。

测试：temp NEOMIND_DATA_DIR 下绝对路径读取成功；`/etc/x.png`、
`/tmp/x.jpg` 仍拒；绝对路径带 `..` 仍拒；symlink 逃逸仍拒。

Round B 必须在 Round A 之后单独一轮。A+B 都过后再评估是否全量回归
（按预算指令，本轮不做 552 重跑）。

## Round C — SOP 沉淀（不改代码与常驻提示行为）

- 新增内置技能 `crates/neomind-agent/src/skills/builtins/image-analysis.md`：
  1. NE302/NE101 图片分析标准流：`device get` → `/api/images/...` 相对 URL
     → 原样传 vision，一步完成；禁止 python 抽取/落盘/拼域名。
  2. vision 本地路径规则：/tmp、/var 等封锁；需 shell 中转时文件放 $HOME。
  3. VLM 后端选择：`model` 参数钉住；本地伴侣 mmproj 未加载的识别特征。
  4. 降级通道：设备端 YOLO 推理结果（本次 DeepSeek 靠它挽救部分答案）。
- 可选后续（单独一轮，因为改常驻提示）：强化 capability_index.rs:101
  措辞并前置；vision 工具 description 补一句封锁目录说明。本轮不动。

## T4 运维修复（独立于代码轮；不做则 A/B 无法端到端验证）

1. 修 builtin-vlm-minicpm-v-4.7-1b：29385 端口 llama-server 400
   "Failed to load image or audio file" → 检查实例启动参数/模型目录，
   补 mmproj 加载。
2. 修正 deepseek_c82e0710 的假 supports_multimodal 标记
   （DeepSeek API 无视觉模型），避免其进入 vision 候选烧轮次。
3. 修复后手工验收一条：
   `vision(image="/api/images/1A0A35/image_data/1793792098_bf480b7db8af5bfd82255fe17659f723.jpg")`
   单次调用返回描述。

## 明确不做（本轮）

- 不统一 paddle-ocr / locate-anything 参数方言（D4）——影响面大，
  待 A/B 筛选数据出来再评估。
- 不改评分规则、超时、case 集。
- 不 commit / push / deploy / publish。
