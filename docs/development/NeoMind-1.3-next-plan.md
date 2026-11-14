# NeoMind 1.3 后续迭代与验收计划

日期：2026-11-14（Asia/Shanghai）
参考基线：1.3.0@9af1f46f；执行前核对实际 HEAD。
状态：执行中。迭代 A 首片（tip/作业核对、主目标 fixture+state_query 预检查、冻结 manifest）已落地；其余条目仍待执行。本文门槛不代表已全部通过。

## 1. 执行约束与交付目标

目标：约 2B 本地模型通过 Chat 完成设备接入、Transform、Rule、Agent、通知及标准 Dashboard 配置，并能验证、修改和排障。
保留现有架构与评测框架，不新增独立编排架构。暂缓新协议、自定义组件和扩展自动编码。
主仓库：/Users/shenmingming/CamThink Project/NeoMind；在 1.3.0 开发，不在旧 8ef0 detached worktree 继续开发。
先完整阅读仓库 AGENTS.md、CLAUDE.md（如存在）、eval/README.md、docs/development/agent-eval-method.md 及受影响模块约束。核对真实实现，不把历史记录当作当前测试结果。
保留用户改动；本计划不授权推送、合并、发布、生产设备控制或真实外发通知。测试使用隔离数据目录、端口、模拟设备和本地接收器。
检查现有推理/构建/评测进程，禁止与运行中的模型评测争用资源。不要恢复历史作业。
每轮只保留可解释的小机制改动，确定性修复与模型效果分别判定。

## 2. 总体执行顺序

A 候选与评测基础 → B @交互原型 → C 引用协议/草稿/历史闭环 → D 状态展示 → E 完整Chat方案 → F 扩展业务与Appliance验收。
B 原型经负责人评审后再确定编辑器实现；可提交可运行原型与取舍报告，不需为了常规实现细节停工询问。
各轮交付代码、测试、复现步骤、证据路径、未验证项，不以文档“完成”代替结果。

## 3. 迭代 A：候选冻结与评测基础

记录 HEAD、工作区、基线/候选二进制 SHA256、模型文件与量化、runtime及哈希、硬件、上下文、采样、思考开关、并发、超时。
修复已有主目标用例中 fixture 不存在、state_query参数错误等问题。对基线候选统一使用修正版本，保留旧结果及撤回说明。
每次运行建立唯一目录和manifest，绑定代码、二进制、用例、fixture、评分器与配置哈希，记录起止时间、退出状态、原始轨迹。
检查138运行二进制是否对应最终提交。旧二进制结果仅证明旧候选，不能当作当前tip验收。
重要工件移入项目认可的持久位置；不只放/tmp。日志不得包含凭证。
验收：fixture/断言预检查通过；分数能从原始记录重算；超时中断完整保留；最终代码通过实际CI要求。
Rust门禁参考 cargo test --workspace --locked --features neomind-agent/test-utils，以真实CI为准。不要误用neomind-api/test-utils。

## 4. 迭代 B：Chat @前端产品专项（核心）

### 4.1 目标与首版边界

优先准确选中、自然编辑、可靠发送、历史可读、移动端可用。
采用行内引用，保留句内角色和位置。例如“把 [设备·A] 的规则复制给 [设备·B]”。
当前附件式Chip只作兼容过渡，不能移除触发文字后丢失来源/目标语义。
先做独立编辑器原型：评估现有组件、成熟编辑器依赖、包体和维护成本；不直接手写复杂contentEditable。
交付桌面/移动原型、录屏或截图、依赖取舍及原型测试。

### 4.2 内容模型与后端契约

消息是有序 text/reference 节点；reference包含entityType、稳定id、labelSnapshot。字段以实际协议设计统一。
后端按节点原位置生成模型消息，并附类型与权威ID；不能只在前导集中列对象。
label只作展示，发送时重解析对象和现有权限。引用不是授权。
同一对象可以在句中重复出现；解析查库可去重，句内节点不能去重。
保留旧纯文本和旧客户端兼容。协议增加版本或明确回退路径，覆盖HTTP、文本WS和图片WS。
限制请求字节数、节点数、唯一引用数和字段长度；上限明确展示，禁止静默丢弃。
名称作为不可信数据处理；清理控制字符并编码边界，不能声称清理字符已彻底解决prompt injection。

### 4.3 选择器

输入@与工具栏@按钮打开同一选择器。
默认分组显示当前页面对象、最近引用；标明来源。筛选全部/设备/规则/转换/Agent。
支持名称、ID、中文子串搜索。重名项显示类型、短ID、已有位置/分组等辨识信息。
搜索覆盖全部可访问对象，优先服务端搜索和分页；禁止仅第一页本地过滤。
区分加载、空结果、请求失败、部分类型失败。防止旧请求覆盖新搜索。
选中后原位插入标签，恢复光标；取消保留输入。搜索请求不得暴露凭证或不可访问对象。

### 4.4 编辑行为

左右键越过原子标签；Backspace/Delete整块删除；选区删除可同时包含文本和标签。
标签可查看简短详情、移除或重选。详情只读，不触发控制。
复制至少获得可读@名称；粘贴普通文本不得自动绑定对象。富文本粘贴必须校验来源与ID。
撤销/重做恢复文本、引用、顺序和光标。
中文IME确认不得误发送或误选，composition结束后正确触发查询。
支持光标中间插入、多引用、重复对象出现、长名称、空消息只含引用的明确发送策略。
输入@邮件、普通文字、粘贴和光标移动均测试，触发边界不能破坏中文。

### 4.5 移动端与无障碍

桌面输入框附近浮层；移动端面板适配软键盘，结果和操作可见。
结果触摸不丢插入位置；返回先关闭面板；横竖屏和窄屏可滚动。
上下键选择、Enter确认、Escape关闭；Tab遵循明确焦点规则。
读屏可获知类型、名称、搜索状态和选中项；不只靠颜色表达状态。

### 4.6 草稿、发送与历史

引用与消息结构一起持久化，发送后用户消息仍显示原位标签。
每会话独立草稿；刷新、会话切换、发送失败和重连不丢引用或串会话。
重试不得重复发送。草稿清理与服务端接受状态有明确边界。
保存名称快照；改名后历史保留发送时名称，详情可显示当前名称。
删除后保留历史标签并标不可用，不匹配替代对象。
发送时无效/无权引用：恢复可编辑草稿，逐项标错，保留其他节点；用户修正后重发。
发送过程中对象消失仍须由实际工具执行进行校验，前端预检查不替代权限与存在性检查。

### 4.7 @专项验收

确定性/浏览器测试：四类搜索分页、重名、原位顺序、同对象重复位置、删除与撤销、复制粘贴、IME、HTTP/WS/图片一致、会话切换刷新、失败重连、改名删除权限、超长恶意输入、键盘触屏无障碍。
组件测试之外必须真实浏览器验证；移动端至少一个实际触屏环境，缺失设备则注明未验证。
5名目标用户×4任务：同名选择、双引用来源目标、替换句中引用、移动端失败恢复重发。
门槛：18/20任务无需协助；错误对象发送0；文本/引用/顺序/草稿丢失0；关键输入流程无阻塞。
记录时间与误操作原因，不用平均时间掩盖失败。
另选6个对象歧义业务场景配对有/无@：定位、误修改、查询/澄清次数、终态耗时；不预设模型收益。

## 5. 迭代 D：结果状态前端闭环

Transform测试页展示失败、无输出、部分成功、未验证；unverified非空不得显示普通绿色成功。纯计算结果与未验证依赖同时可见。
Rule增加“为什么未触发”只读入口，展示条件值、值龄、持续时间、冷却、最近执行及未知原因。
命令展示适配器接收、扩展接收、设备ACK和物理效果验证；无证据不得提升层级。
Agent展示已知输入时间、输出、动作结果与失败节点，复用既有存储；缺证据明确未知。
验收：API到UI不丢状态，桌面移动代表流程通过，错误/警告可展开，不能只靠颜色。

## 6. 迭代 E：一个完整Chat方案

首选：摄像头检测→人数统计Transform→超阈值Rule→本地通知。
覆盖发现、创建、测试、启用、查看证据、改阈值/通知目标、故障诊断、取消、重复执行、中断恢复。
模型应检查真实已完成节点，不能盲目重建。基于失败轨迹选最小改动，不预先要求新编排层。
行为刺激包含正常、越限、恢复和冷却期重复输入。断言指标、触发、通知次数、关联和无关资源差异。
验收：中英文各5次，冻结输入与采样，共10次至少9次完整成功；无禁止副作用、错误对象修改、证据外成功声明。
该门槛仅证明该场景，不代表整体成功率。

## 7. 评测体系：138之外新增三套

历史138保持既定用例、超时和cmd_ok AND sq_all_pass，不放宽、不覆盖。
新增业务能力集、交互与接口契约集、Appliance运行集，分别报告，不合成一个掩盖风险的总分。

### 7.1 业务能力集规模与分组

48独立场景，中英文镜像96输入变体；多轮仍按一个原始场景聚类。
开发24场景、日常回归12、独立验收12；按场景与故障家族分组分层划分，镜像改写不能跨组。
独立集由评测维护者保管，开发模型不读取；用于分析后转已知集，补新的留出。
数量不足以支持总体提升结论时明确限制，不把语言和重复试验当独立客户场景。

设备8：
DEV01已知类型接入；02未知Payload草稿确认；03单位缩放歧义澄清；04注册无数据；05凭证连接错；06重复接入；07同名设备；08断连恢复。
Transform8：
TR01标量；02嵌套数组；03JS错误；04null/undefined空输出；05副作用扩展隔离；06catch后仍unverified；07MultiOutput部分失败；08定向修改且测试不污染生产。
Rule8：
RULE01阈值；02AND/OR；03持续时间；04冷却；05缺失/过期；06双规则历史隔离；07改字段不动其他配置；08历史无证据不臆断。
Agent6：
AG01单目标配置；02规则触发；03缺失陈旧输入；04工具失败超时；05无ACK交付层级；06非幂等重复请求按真实策略处理。
配套6：
AUX01本地Webhook实收；02接收端失败；03修改渠道不重复；04标准卡片正确绑定；05同名指标来源；06数据源删除。
跨域12：
E2E01检测通知链；02传感器Agent模拟命令；03新设备建规则；04中途改需求；05启用前取消；06重复方案；07转换故障诊断；08通知故障定位；09重启恢复查询；10引用改名删除；11多轮澄清保留参数；12中断续建。
按上述ID建立具体fixture、输入、允许动作、终态、行为刺激、禁止副作用、清理、超时和判定器。
预检查用例资源存在与断言参数契约，避免已知fixture缺陷污染评测。

### 7.2 评分维度

主成功=终态正确 AND 行为正确 AND 无禁止副作用 AND 回复有证据。
主分母包括全部已启动任务，超时计失败；此前10成功/20任务=50%，10/17仅作排除超时诊断。
基础设施故障标注保留，修复后统一重跑，禁止静默删样本。
分项报告：对象定位、字段/关联/启停、行为触发、真实性、必要/不必要澄清、恢复、修改、重复资源/动作、耗时/调用、故障节点定位、人工介入次数。
澄清正确不等于业务最终完成；分别记录。
真实性证据对应：注册记录、新遥测、规则历史、本地通知实收、适配器接收、设备ACK、物理状态反馈。
优先确定性断言；语言按预注册规则人工复核，有争议待判定。LLM judge如采用须固定校准且不能覆盖硬失败，不擅自外发数据。
时序边界用确定性可控时钟，避免让模型长时间等待。

### 7.3 每case概念契约

id / scenario_group / language / split；
fixture与清理；conversation多轮；
allowed_operations与required_confirmation；
expected_state与relationships；
behavior_probe输入和预期输出；
forbidden_effects；
required_facts与forbidden_claims；
timeout和判定来源。
映射到现有schema，不能直接假定这些字段可被runner识别。

## 8. Appliance运行评测

选一个目标配置，冻结CPU/内存/系统/存储、模型量化哈希、runtime、上下文/并发/采样和候选二进制。
6代表任务：查询、单对象修改、Transform创建测试、Rule排障、方案搭建、多轮恢复。
测冷/热启动、首个有效反馈、首个有用工具调用、正确终态时间、峰值内存CPU/OOM、连续稳定性、断连重启恢复、遥测并行负载。
禁止与构建/其他评测争用；重叠记录时间窗口，重跑受影响案例。
先测基线再冻结硬件性能目标；样本不足不把p95当稳定结论。无OOM/崩溃，性能不能以正确率退化交换。

## 9. 配对、门禁与发布

两臂相同模型/runtime/采样/超时/fixture/评分器/硬件，交错执行；重复次数事先冻结。
报告both pass/gained/lost/both fail、绝对数、按原始场景聚类区间、每个lost轨迹。
统计不足写“证据不足”，不能把净增益或相关重复样本的p值当确定提升。
发布要求：实际CI必需检查通过；契约关键用例全过；高风险错误出现即停止验收复现；重点场景9/10；138遵守既有规则并分析lost；新增集独立出分；硬件稳定；版本与产物一致。
正式发布前同步Cargo、Web、Tauri、lockfile、CHANGELOG及升级元数据，开发分支名不等于产品版本。
最终tip修改后重新验证相关路径；既往3908/383或138结果不自动覆盖新提交。

## 10. 执行模型每轮报告模板

目标/真实失败证据：
基线与候选提交、二进制、模型和配置哈希：
唯一主要机制与改动范围：
用例/fixture/判定器版本：
测试命令、退出状态、工件：
业务终态/行为/真实性/副作用/效率：
配对矩阵与gained/lost：
未验证项及限制：
保留/回修/撤回建议：
下一轮最小任务：

## 11. 首批任务清单

- [x] A：核对tip与运行作业、冻结manifest、修复用例基础、确认当前138二进制。
  （2026-11-14 切片：tip `9af1f46f`；运行中 138 属 8ef0@`830b54c1` 旧候选 bin `a7c55395…`，≠ tip，不作 tip 验收；`seed-main-objective` + `device_command_sent` name 回退 + `case_precheck`；证据 `eval/manifests/1.3.0-iter-a-foundation.json`。未跑新 138 / 未宣称模型增益。）
- [x] B：提交行内@桌面/移动原型及编辑器取舍。
  （2026-11-14 切片：`/dev/inline-at` 分段编辑器原型 + `InlineDoc` 模型；取舍推荐 C 片用 TipTap；vitest 12/12；生产 ChatComposer 未改。触屏真机未验证。）
- [x] C：有序节点协议、后端原位解析、草稿历史、失败恢复和兼容。
  （2026-11-14 切片：`contentNodes` v1 + HTTP/WS/图片路径原位渲染；旧 `references` preamble 兼容；超限硬 400/Error；会话草稿 localStorage；生产 Chip 未换行内编辑器。SessionMessage 持久化 nodes 未做。）
- [x] C：全量可访问对象搜索/分页、重名辨识、无效引用处理。
  （2026-11-14 切片：`GET /api/mentions/search` 全量服务端搜索+分页+`duplicateName`/`shortId`/`hint`+`typeErrors`；`POST /api/mentions/resolve` 发送前预检；ChatComposer 类型筛选/加载更多/无效芯片标错并保留草稿。证据 `eval/manifests/1.3.0-iter-c-mentions-search.json`。未换 TipTap；浏览器/触屏/5人未做。）
- [x] C：浏览器/触屏/IME/无障碍测试与5人可用性验证。
  （2026-11-14 切片：compositionend 重扫 + IME Enter 防误选/误发；listbox/option/tablist ARIA；vitest 38（Composer+InlineAt+IME guard+model）。**未做**：真浏览器 headed、真触屏、VoiceOver 实机、5 人×4 任务——缺口与协议见 `docs/development/mention-a11y-usability-checklist-2026-11-14.md`。证据 `eval/manifests/1.3.0-iter-c-mention-a11y.json`。）
- [x] D：Transform/Rule/命令/Agent状态界面闭环。
  （2026-11-14 完成：① Transform 测试诚实判定。② Rule「为什么未触发」。③ 命令 ACK 阶梯。④ Agent 执行证据：`stop_reason`/`tokens_used` DTO 透出 + `deriveAgentExecutionEvidence` + `AgentExecutionEvidencePanel`（输入时间/输出/动作结果/失败节点；缺证据标未知）；证据 `eval/manifests/1.3.0-iter-d-agent-execution-evidence.json`。验收桌面/移动代表流程与 5 人未做。）
- [x] E：首个可运行方案10次预注册验收。
  （2026-11-14 切片：**预注册完成** — `eval/acceptance/iter-e-detection-chain.json` 冻结 5×zh+5×en、采样 .3/.75/20、≥9/10；硬化 `main-objective-e2e-detection-chain`（transform≥1、rule≥2、保留 `rule-battery-low`）；平台 `scenario_a_detections…` PASS。**未做**：Spark 10 次 live（PID 89912 占 `:29431`，争用风险已记）；多轮测试/改阈值/故障/取消/中断与行为探针 deferred。证据 `eval/manifests/1.3.0-iter-e-detection-chain-preregister.json`。）
- [ ] F：48场景分组冻结、基线候选配对、最终138与Appliance验收。
  （进行中 — 2026-11-14 离线片：48 ID 目录 + 分层 24/12/12 + holdout 仅 ID；配对门禁文档；`test_business_capability_48` 7/7。证据 `eval/manifests/1.3.0-iter-f-48-freeze-pairing.json`。**未做**：tip 上 live 138、Appliance 六任务、E live 9/10、stub 用例全量编写。）
- [x] 发布前：版本统一、证据归档、未解决问题、人工复现步骤。
  （2026-11-14：产品版本统一到 **1.3.0**（Cargo/Web/Tauri/locks + CHANGELOG）；证据索引 + 未决（E live 10、tip 138、Appliance、5 人可用性、触屏 Playwright 等）+ 人工复现见 `docs/development/1.3.0-release-prep-2026-11-14.md` / `eval/manifests/1.3.0-release-prep.json`。**不 push、不打 tag**；138 未触碰。）


## 12. 2026-11-14 审查补充：上下文可靠性（优先于 E/F live）
基线：Mac 1.3.0 @66574262，审查开始工作区干净。本节覆盖§11笼统勾选：实现、预注册不等于实际验收。
### 发现与优先级
- P0 llm.rs:2330–2415：内层按单消息逆序裁剪，超预算立即break，短历史无条件插回首个user，即使预算为0；无法保证最近完整工具交互。外层context.rs保护后仍可能被二次裁剪。
- P1 llm.rs:1032：不同Skill全文累积无总量上限；可能挤占历史，尚未证明是生产耗尽预算的主因。
- P1 stream_core.rs:157：新用户轮清除Skill；需验证“继续”、目标纠正和任务状态衔接。不能直接永久保留全部Skill。
- P1 生产行内@、SessionMessage nodes持久化、真人UX、E live、tip138尚未完成，不能因计划打勾宣称发布验收通过。
生产证据：T4 1.2.11会话6621a013-0d12-43c7-ad39-6dbf6e9cb156，北京时间19:46:18/:29/:38，两项list成功返回7925/449字节；历史9→1、9→1、10→1，available_tokens=0；实际Spark请求message_count=3后重复查询。证明裁剪与循环同时发生，完整请求正文未留存，唯一根因仍需复现。DeepSeek关联成功会话仅确认render_text回执queued，非屏幕物理刷新，非受控A/B。
### 执行计划
1. G0 检查实际作业，冻结源码/二进制/模型/运行时/采样/fixture哈希，不恢复旧队列、不争用推理。隔离实例记录脱敏实际序列化请求及消息ID、裁剪原因、system/Skill/tools/history/current-user/template预算、摘要fallback、实际usage和循环保护开关；禁止保存认证头/密钥。
2. G1 单轮只修一个机制：统一预算契约，保护当前目标与最近完整工具调用/结果组，按协议匹配调用ID。大结果压缩保留设备ID、错误、状态、ACK。固定提示已耗尽时压缩或明确失败，禁止静默丢反馈继续循环。Skill总量策略如为独立机制，另轮验证。
3. G2 有证据再补轻量跨轮任务状态：目标、对象、内容、已证实步骤、下一步及来源；纠正覆盖旧目标，取消停止，继续不重放成功写入。不靠模型自述判完成。
4. G3 循环兜底另轮验证：区分合法状态轮询与无进展重复；有限重试并明确卡点。停止循环不算任务成功。
### 测试及验收
- 修改前固定回放并保存完整请求/工具调用和原始结果/最终回复/业务终态；修改后使用相同冻结fixture和评分，不改超时、不删失败。
- 确定性测试：零预算、超大最新结果、多工具ID配对、中文混合内容、多Skill、摘要fallback、“继续”、纠正、取消、成功写入后未知回执、旧协议兼容。断言无孤儿工具结果、关键证据保留、预算不足明确处置、不泄漏凭据。
- Mock天气→EK410链路8场景：短会话、无关长历史、多Skill、屏幕纠正为eink、继续、工具失败恢复、写入成功回执未知、取消。每场景中英×2重复=32次/arm，两臂64次交错。新会话与历史回放分开报告。
- Spark1.7B主验收，冻结运行时及采样；DeepSeek独立参考。同一工具返回，mock写入隔离硬件副作用。
- 调参前预注册另8个独立变体，同样32次/arm。门槛：候选>=28/32且不低于配对基线；正常控制无新增失败；错误设备写入、重复副作用、虚假成功均0；连续3轮同参同结果且无进展循环trial为0。未完成的止循环仍算失败。该门槛是工程门槛，不宣称统计显著。
- 每trial归档请求、裁剪、工具原始结果、状态、回复、时间、结束原因；保留超时/失败，报告gained/lost及回归原因、重复调用、P50/P95耗时和token。
- queued只算排队，物理成功要求对应设备证据；不按预设命令路径评分。
### 待完成清单（覆盖§11相关勾选）
- [x] G0 实际请求复现与根因定位。
  （2026-11-14 离线：`prompt_capture` 脱敏归档 schema + T4 形态 tip 装配捕获；根因对照 legacy `message_count≈3` vs tip `ZeroBudget`→2；fixture `eval/fixtures/history-budget-p0/g0-desensitized-prompt-archive.json`；单测 3/3 + test-utils 4/4。证据 `eval/manifests/1.3.0-g0-desensitized-prompt-capture.json`。**生产原文请求体仍不可得**；Spark 实捕被 138 阻塞。）
- [x] G1 修复、协议/证据保留测试及独立配对验收。
  （代码/单测已落地；配对矩阵已离线预注册 `eval/acceptance/context-reliability-weather-ek410.json`（8×zh/en×2=32/arm）。**Live 64 trial 被 138 阻塞，未跑、无分数宣称。**）
- [x] G2/G3 按证据逐轮验证衔接和循环兜底。
  （2026-11-14 离线：`task_continuity` — 纠正覆盖目标、取消停写、继续不重放成功写入、完成只认证据；G3 identical-rounds 止损 `halt` 且 `counts_as_task_success=false`；挂到 `AgentInternalState` + stream 止损路径。单测 9/9；预注册 `eval/acceptance/g2-g3-continuity-scenarios.json`。证据 `eval/manifests/1.3.0-g2-g3-task-continuity.json`。**Live 探针仍被 138 阻塞。**）
- [x] G2 `may_execute_write` 接到 mutating tool 分发。
  （2026-11-14：`mutating_write_fingerprint` + stream_core/multimodal 分发前闸门；成功写入 `record_successful_write`。单测 fingerprint + task_continuity 10/10。证据并入 `eval/manifests/1.3.0-c-nodes-persist-write-gate.json`。Live 仍被 138 阻塞。）
- [x] C 生产 chip→contentNodes 发送 + SessionMessage nodes 持久化（历史可回读）。
  （2026-11-14：`SessionMessage`/`AgentMessage` `contentNodes`+`contentVersion` 往返；HTTP/WS 在 process 前 `attach_pending_content_nodes`；生产 Chat/Panel 用 `chipsAndTextToContentNodes` 发送。legacy chips 亦可合成 nodes。单测 storage/api/agent + vitest contentProtocol 4/4。**未做**：TipTap 真行内编辑器、真浏览器/触屏/5人、历史 chip UI 重绘。）
- [x] C 生产 TipTap 真行内 `@`（替换 trailing chips）。
  （2026-11-14：ChatComposer 在 `onReferencesChange` 时用 TipTap Mention；同实体可句内重复；toolbar `@`；contentNodes 原位发送；无 mention 宿主仍走 textarea。vitest TipTap+docBridge+Composer 35/35。证据 `eval/manifests/1.3.0-c-tiptap-inline-mention.json`。**未做**：真浏览器/触屏/VoiceOver/5人、历史消息 inline chip 重绘。）
- [x] C 历史消息按 `contentNodes` 重绘行内 chip + 重发/回显。
  （2026-11-14：`UserMessageContent` 按有序 nodes 画 `.nm-mention`；`mergeMessagesForDisplay` 归一化 camel/snake；Chat/Panel「填回输入框」恢复 nodes+草稿；无效发送也落盘草稿。vitest TipTap+history 54/54。证据 `eval/manifests/1.3.0-c-history-content-nodes.json`。**未做**：真浏览器/触屏/VoiceOver/5人。）
- [x] C 真浏览器 headed Playwright（触屏模拟 + axe）；5人仍人工缺口。
  （2026-11-14：`e2e/specs/mention-a11y.spec.ts` headed Chrome **6/6**；历史 chip/reuse、listbox ARIA、mobile `hasTouch` tap、axe serious/critical。证据 `eval/manifests/1.3.0-c-mention-headed-a11y.json`。**明确未通过**：物理触屏、VoiceOver/TalkBack 实听、5×4（**0/20**，禁止宣称 18/20）。138 PID 89912 / `:29431` 仍占用、未触碰、未开新长跑。）
- [ ] C 人工：真机触屏 + VoiceOver/TalkBack + 5人×4任务>=18/20（未执行）。
- [ ] D 桌面/移动代表流程。
- [ ] E Spark实际10次>=9/10，多轮/故障/取消探针。
- [ ] F 48场景可执行化、tip138与Appliance验收。
G0–G3 离线项、C TipTap/历史 chip/headed Playwright 已勾；**C 真人 UX/5人、中英配对 live、D/E/F** 仍为缺口。最终发布准备文档必须引用本节剩余项。
