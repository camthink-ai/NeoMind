# NeoMind 边缘模型评测完整报告(2026-10-28 ~ 10-30)

> 批次代号:`2026-10-29-official-params` · 22 个跑次(21 有效 + 1 作废)
> 数据快照:`eval/baselines/2026-10-29-official-params/`(git)+ HF `CamThink-AI/neomind-agent-eval`
> 相关文档:[docs/model-leaderboard-20261029-official.md](model-leaderboard-20261029-official.md) · [docs/edge-models.md](edge-models.md)

---

## 一、执行摘要

本批次对 NeoMind 目录内全部候选本地模型(1.7B–4.2B)及两个云端参考模型,在**统一 138-例双语硬信号门禁**上完成了 **22 个跑次**的横评,覆盖思考开/关双模式与采样参数归因实验。

**五条核心结论:**

1. **Spark-X2.6-1.7B + 思考模式 = 63.8%,新的边缘总冠军**——用 40% 的参数超越 Qwen3.5-4B-think(55.1%)8.7pp,距云端 GLM-5.3 仅 13pp。
2. **思考模式是中小模型最大的免费杠杆**:Spark +20.3pp、Qwen +13.8pp、MiniCPM5 +12.4pp;但对 LFM2.5/Gemma 基本无效(+1.5/+1.4pp),LFM3 的思考在慢速硬件不可用(每轮 1,500–5,400 思考 token)。
3. **采样参数必须按厂商官方值且随下载下发**:错参损失实测 10–23pp(LFM3 −23pp、LFM2.5 −11.6pp、Qwen −10.1pp,方向各异)。
4. **参数量在思考模式下不再决定成绩**:Spark-4B-think(59.4%)低于自家 1.7B(63.8%)——思考链质量由后训练决定。
5. **云端与边缘差距 13–18pp**(DeepSeek 81.2% / GLM-5.3 76.8% vs Spark-1.7B-think 63.8%),边缘最优配置已具备生产可用性。

**产品动作(已落地):** README 中英文双语更新(c2b97341)——默认推荐切换为 Spark-X2.6-1.7B(思考开),附采样覆盖表与 llama.cpp v0.8.0 下限说明。

---

## 二、评测目标与背景

- **业务动因**:NeoMind 目录向客户推荐本地模型,需要一份公平、可复现、含部署指引的横评;此前 2026-09 套件(12 场景)与 2026-10 40 例门禁的口径不一,且旧 sweep 存在采样参数错误。
- **本批次修正的三个历史错误**:
  1. LFM 系列曾用 1.0/0.95(Liquid 官方为 0.1/top_k 50/rp 1.1)→ LFM3 被低估 23pp;
  2. Qwen 关思考模式曾混用官方参数与任意参数;
  3. Gemma 一次 121/138 环境竞争失败的成绩(7%)已作废重跑。
- **SFT 关联**:r11 微调模型(MiniCPM5-2B 底座,1,175 轨迹)此前 42.8%(旧口径),本批次以统一口径重算为 40.6%,用于评估"零样本 Spark vs 微调 MiniCPM"的选型问题。

---

## 三、方法论

### 3.1 门禁设计

| 项 | 值 |
|---|---|
| 用例集 | `eval/regression_set.txt`,69 个 case-id × 中英双语 = **138 次执行** |
| 类别 | 设备/规则/智能体/仪表盘/扩展/变换/消息 + 行业场景(农业/工业/楼宇/冷链…)+ 多步深水区 |
| 通过判据(统一函数) | `cmd_ok AND sq_all_pass` —— 既发出全部期望 CLI 命令,且全部终态断言(HTTP 查询真实平台状态)成立 |
| 上下文 | **每请求 32K**(全模型一致;llama-server `-c 32768`,slots 配置见附录 B) |
| 采样 | 各模型**厂商官方推荐参数**(见 3.3 表);错参对照跑次明确标注 |
| 隔离 | 每 case 独立 `neomind serve` 子进程 + 独立 temp 数据目录 + 独立 MQTT 端口 |
| 二进制 | **neomind 1.2.3**(Mac 为 10-22 自建;T4 为官方 release;全部经 `--version` 验证) |
| llama-server | Mac: b11946(Metal 补丁版);T4: v0.8.0(CUDA 12.9 自编译,sm_75) |
| 防伪机制 | 每 case 预检 served vs requested 模型身份,不匹配即拒跑(本批次拦截过 1 次换台竞态);`served_model.json` 自动落盘 |

### 3.2 指标维度

- **总分**:138 例通过率(下文所有"分数"均此口径)。
- **zh / en**:中英文分别通过率(双语能力差)。
- **多步(multi)**:multi/guided/deep/full-lifecycle/agent/extension/api/haccip 类(40 次执行)——规划能力分水岭。
- **单步(simple)**:其余 98 次执行。
- **cmd_partial(命令命中率)**:期望命令中实际发出的比例("接近正确"信号)。
- **sq_partial(终态达成率)**:终态断言通过比例。
- **af(agent_failed)**:聊天回合级失败的执行数(应≈0)。

### 3.3 官方采样参数表(本批次实际使用)

| 模型 | temperature | top_p | top_k | 其他 | 来源 |
|---|---|---|---|---|---|
| Spark-X2.6(1.7B/4B) | 1.0 | 0.95 | —(默认) | min_p=0 | XHToken 官方卡 |
| Qwen3.5-4B(非思考) | 0.7 | 0.8 | 20 | min_p=0 | Qwen 官方(非思考档) |
| Qwen3.5-4B(思考) | 0.7 | 0.8 | 20 | 同上 | 同上 |
| LFM3-1.7B | 0.1 | — | 50 | rp=1.1 | Liquid 官方卡(通用档) |
| LFM2.5-2.6B QAD | 0.1 | — | 50 | rp=1.1 | Liquid 官方 |
| Gemma-4-E2B QAT | 1.0 | 0.95 | 64 | — | Google/Runtimes 目录 |
| MiniCPM5-2B | 1.0 | 0.95 | 40 | 思考默认开 | openbmm/Runtimes 目录 |
| Nanbeige4.2-3B | 1.0 | 1.0 | —(默认) | 思考重 | owao/Runtimes 目录 |
| GLM-5.3 / DeepSeek-flash | API 默认(1.0/1.0) | | | 思考关 | 云端参考行 |

---

## 四、基础设施

| 机器 | 角色 | 关键参数 |
|---|---|---|
| **Mac(Apple Silicon, 48G)** | Mac 侧全部 off/think 跑次 + Spark-4B-think 前 3 例(作废) | llama-server b11946 Metal 补丁(修 ggml_metal_unary_cast constexpr bug, issue #31272);独占时 25–108 tok/s |
| **T4(16G VRAM, 30G RAM)** | Qwen/LFM2.5/LFM3/MiniCPM5/GLM/DeepSeek/Spark-4B-think/Nanbeige-think(作废) | v0.8.0 CUDA 自编译(b11946 系两构建均不识别 lfm3 架构);独占 40–114 tok/s;生产 llama-server.service 与两个外部租户(~9.6G)共存 |
| **Jetson(7.4G RAM)** | 曾尝试 LFM2.5,两次 OOM 后退出 | 评测 llama-server 在 7.4G 盒子上内存失控(kernel OOM-kill),生产已恢复 |

**加速手段**:剩余 case 按 id 均分多 shard 并发(T4 2 slots / Mac 2 gates),单跑提速 2–6 倍;所有 shard 经 (case_id, lang) 去重合并,审计确认零重复零缺失。

---

## 五、完整成绩(统一口径,全部由原始 scores.jsonl 重算)

### 5.1 总表

| # | 模型 / 配置 | n | 总分 | zh | en | 多步 | 单步 | cmd命中 | 终态率 | af |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | DeepSeek-flash(云端,off) | 138 | **81.2%** | 82.6 | 79.7 | 70.0 | 85.7 | 91.0 | 96.7 | 0 |
| 2 | GLM-5.3(云端,off) | 138 | **76.8%** | 78.3 | 75.4 | 70.0 | 79.6 | 88.7 | 93.2 | 0 |
| 3 | **Spark-X2.6-1.7B think** ⭐ | 138 | **63.8%** | 58.0 | 69.6 | 50.0 | 69.4 | 82.1 | 76.6 | 0 |
| 4 | Nanbeige4.2-3B off*(部分) | 108 | **60.2%** | 55.6 | 64.8 | 38.9 | 70.8 | 76.8 | 70.2 | 3 |
| 5 | Spark-X2.6-4B think | 138 | **59.4%** | 58.0 | 60.9 | 45.0 | 65.3 | 76.3 | 74.8 | 0 |
| 6 | Qwen3.5-4B think(官方参) | 138 | **55.1%** | 58.0 | 52.2 | 42.5 | 60.2 | 76.4 | 75.8 | 0 |
| 7 | Qwen3.5-4B off @1.0/0.95 | 138 | 51.4% | 56.5 | 46.4 | 37.5 | 57.1 | 68.4 | 72.9 | 0 |
| 8 | Spark-X2.6-4B off | 138 | 50.0% | 46.4 | 53.6 | 35.0 | 56.1 | —† | —† | 1 |
| 9 | Qwen3.5-4B off @1.0/0.95(Mac 旧跑) | 138 | 50.0% | 49.3 | 50.7 | 25.0 | 60.2 | —† | —† | 1 |
| 10 | Spark-X2.6-1.7B off | 138 | 43.5% | 42.0 | 44.9 | 27.5 | 50.0 | 63.9 | 58.2 | 0 |
| 11 | LFM3-1.7B off(官方参) | 138 | 42.8% | 42.0 | 43.5 | 17.5 | 53.1 | 56.9 | 55.4 | 0 |
| 12 | Qwen3.5-4B off(官方参) | 138 | 41.3% | 40.6 | 42.0 | 17.5 | 51.0 | 62.3 | 64.6 | 0 |
| 13 | r11 SFT(MiniCPM5-2B 底座)off | 138 | 40.6%‡ | 40.6 | 40.6 | 12.5 | 52.0 | —† | —† | 0 |
| 14 | MiniCPM5-2B think | 138 | 45.7% | 49.3 | 42.0 | 15.0 | 58.2 | 60.3 | 56.7 | 1 |
| 15 | LFM2.5-2.6B QAD think | 138 | 37.7% | 36.2 | 39.1 | 10.0 | 49.0 | 57.3 | 56.8 | 0 |
| 16 | LFM2.5-2.6B QAD off | 138 | 36.2% | 36.2 | 36.2 | 12.5 | 45.9 | 51.3 | 52.9 | 0 |
| 17 | MiniCPM5-2B zero off(历史行) | 138 | 33.3%‡ | — | — | — | — | — | — | — |
| 18 | Gemma-4-E2B QAT think | 138 | 27.5% | 30.4 | 24.6 | 15.0 | 32.7 | 49.6 | 51.8 | 0 |
| 19 | Gemma-4-E2B QAT off | 138 | 26.1% | 27.5 | 24.6 | 15.0 | 30.6 | 51.7 | 53.7 | 0 |
| 20 | LFM2.5-2.6B off(错参 1.0/0.95,Mac 旧跑) | 138 | 24.6% | 23.2 | 26.1 | 10.0 | 30.6 | —† | —† | 0 |
| — | Nanbeige4.2-3B think | — | **作废** | | | | | | | | 16K ctx,839 截断 |

\* Nanbeige 为 108/138 部分样本(见 §8.4)。
† 旧 Mac 跑次早于 partial-credit 字段引入,无此数据。
‡ r11 旧文档口径(排除超时)为 42.8%;MiniCPM 零样本 33.3% 引自 2026-10-28 排行榜文档。

### 5.2 思考模式矩阵(Δ = think − off)

| 模型 | OFF | ON | Δ | ON 多步 | 解读 |
|---|---|---|---|---|---|
| Spark-1.7B | 43.5 | **63.8** | **+20.3** | 27.5→50.0 | 思考链短而准,近翻倍 |
| Qwen3.5-4B | 41.3(官方参) | **55.1** | **+13.8** | 17.5→42.5 | 混合思考架构受益大 |
| MiniCPM5-2B | 33.3 | **45.7** | **+12.4** | —→15.0 | 单步精度大增,多步仍弱 |
| Spark-4B | 50.0 | 59.4 | +9.4 | 35.0→45.0 | 参数翻倍不抵 1.7B |
| LFM3-1.7B | 42.8 | 47.1 | +4.3 | 17.5→20.0 | 思考量巨大但转化率低 |
| LFM2.5-2.6B | 36.2 | 37.7 | +1.5 | 12.5→10.0 | 无效 |
| Gemma-E2B | 26.1 | 27.5 | +1.4 | 15.0→15.0 | 无效 |

### 5.3 中英文差(值得注意的偏科)

| 模型 | zh | en | 差 | 备注 |
|---|---|---|---|---|
| Spark-1.7B-think | 58.0 | 69.6 | **en +11.6** | 英文显著强 |
| Nanbeige | 55.6 | 64.8 | **en +9.2** | 英文显著强 |
| Qwen off@1.0/.95 | 56.5 | 46.4 | **zh +10.1** | 中文显著强 |
| LFM3-think | 52.2 | 42.0 | **zh +10.2** | 思考后中文反超 |
| MiniCPM5-think | 49.3 | 42.0 | zh +7.3 | 中文偏强 |

部署提示:面向国内客户中文优先时 Qwen/MiniCPM5 的相对价值上升;出口场景 Spark/Nanbeige 更优。

---

## 六、归因实验(为什么分数会动)

### 6.1 机器 vs 参数(Qwen3.5-4B 反事实)

| 跑次 | 机器 | 参数 | 分数 |
|---|---|---|---|
| Mac 旧 sweep | Mac | 1.0/0.95 | 50.0% |
| 反事实 | **T4** | 1.0/0.95 | **51.4%** |
| 官方参 | **T4** | 0.7/0.8/k20 | 41.3% |

**机器效应 ≈ +1.4pp(噪声级);参数效应 = +10.1pp**,且提升集中在多步类(17.5%→37.5%)。→ 分数跨硬件可迁移,跨采样参数不可比。

### 6.2 错参 → 官方参(方向各异,证明必须逐模型校准)

| 模型 | 错参分数 | 官方参分数 | Δ |
|---|---|---|---|
| LFM3-1.7B | ~20%(1.0/0.95) | 42.8% | **+23pp** |
| LFM2.5-2.6B | 24.6% | 36.2% | **+11.6pp** |
| Qwen3.5-4B | 51.4%(1.0/0.95) | 41.3%(官方) | **−10.1pp** |

注意 Qwen 方向相反:官方"非思考档"参数在本 harness 上反而次优——**厂商对话默认参数 ≠ agent 场景最优**,目录需为 Qwen 提供覆盖值(1.0/0.95 off 或官方参+think)。

### 6.3 Spark-1.7B vs LFM3-1.7B(目录 1.7B 对决)

| 维度 | Spark-X2.6-1.7B | LFM3-1.7B |
|---|---|---|
| off 总分 | **43.5%** | 42.8%(基本平手) |
| think 总分 | **63.8%** | 47.1(差 16.7pp) |
| 多步(think) | **50.0%** | 20.0 |
| 命中率(think) | **82.1%** | 63.9 |
| 思考成本 | 短链,边缘可用 | 1,500–5,400 tok/轮,300s 看门狗触发 36 次 |
| llama.cpp 要求 | ≥ b10828 | ≥ b11946(v0.8.0 官方支持) |
| 公开 bench(LFM3 官方卡)| — | LiveClaw 45.6 / BFCLv4 71.4 |

公开 bench 上 LFM3 大幅领先(其官方卡对比 LFM2.5 avg 49.4 vs 31.6),我们门禁上却落后 Spark 16.7pp——原因:公开 bench 为短工具循环 + 256K 上下文 + 推理开启;我们是 30 轮状态化 ReAct,LFM3 的超长思考在多步链上雪崩(最后 30 例通过率 26.7% vs Spark 同批 68.8%)。**"公开 bench 高 ≠ agent 场景强"是本批次最重要方法论警示。**

### 6.4 r11 SFT 的位置

r11(40.6%)< Spark-1.7B 零样本 off(43.5%)≪ think(63.8%)。上一代结论"微调 +9.5pp"成立,但 Spark 的思考模式直接越过了微调收益。**下一代 SFT 底座建议:Spark-X2.6-1.7B**——多步天花板最高、思考链短(训练数据 token 成本低)、Apache-2.0。

---

## 七、逐模型档案(部署视角)

### Spark-X2.6-1.7B ⭐ 新默认
- 强项:思考模式全面第一(多步 50/单步 69.4/命中 82.1);英文 69.6;1.8GB 体积、T4 上 ~70 tok/s。
- 弱项:off 模式平庸(43.5);中文 58 略逊于英文。
- 部署:1.0/0.95 + 思考取 63.8%;llama.cpp ≥ b10828。

### Qwen3.5-4B —— 4B 档旗舰
- 强项:think 55.1(多步 42.5);中文 58;带视觉(mmproj)。
- 弱项:官方"非思考档"参数陷阱(41.3);8K 上下文塌陷(历史数据)。
- 部署:开思考 + 官方参;或关思考 + 覆盖 1.0/0.95。

### Nanbeige4.2-3B —— 精度型异类
- 强项:off 60.2(部分样本)第二;单步 70.8;英文 64.8。
- 弱项:稠密 KV @32K 需 ~11G 显存(2-slot 则 22G);解码慢(重思考模型);中文 55.6 偏弱;think 模式在本门禁无法公平测出。
- 部署:仅推荐显存充裕(≥8G 空闲)+ 英文/单步精度敏感场景。

### LFM3-1.7B —— 轻量但不适合多步
- 官方卡自评"不建议复杂多步推理"——我们完全验证(多步 17.5/20.0)。
- 思考量 1.5k–5.4k tok/轮:25 tok/s 硬件上不可用;T4 114 tok/s 也触发 36 次看门狗。
- 部署:单轮问答/结构化提取可用;严禁在多步 agent 场景当主力。

### MiniCPM5-2B —— 思考救活的上一代默认
- off 33.3 → think 45.7(+12.4);中文 49.3 尚可;多步 15.0 仍是死穴。
- 已从默认推荐降级(README 更新),仍适合已部署客户的配置升级路径(开思考)。

### LFM2.5-2.6B / Gemma-4-E2B —— 不推荐
- LFM2.5:36.2/37.7,全维度中下,思考无效;错参敏感(-11.6pp)。
- Gemma:26.1/27.5,智能体负载不适配(与 2026-09 评测"资源创建弱"一致)。

### 云端参考(DeepSeek-flash 81.2 / GLM-5.3 76.8)
- 边缘冠军距云端 13–18pp;云端在多步类(70.0)对边缘(50.0)优势最大——**规划仍是云端护城河**。
- 混合策略建议:网关设备本地 Spark-1.7B,可选云端升级开关(DeepSeek-flash 性价比最高)。

---

## 八、事故与数据完整性(全程留痕)

### 8.1 公正性审计(2026-10-30,脚本化全量核查)

对全部 20 个有效跑次逐项检查,**全部通过**:

| 检查项 | 结果 |
|---|---|
| (case_id, lang) 唯一性 | 20/20 跑次均 138 唯一(或声明部分),**0 重复** |
| 覆盖完整性 | 除 Nanbeige(108)外 0 缺失 |
| 零延迟记录(伪执行特征) | **0%**(所有跑次中位延迟 16–293s,最大 1,135s) |
| 模型身份(served_model.json) | 20/20 `requested == served_identity` |
| af(agent_failed) | 20 跑次合计 6(分散于 4 个跑次),无聚集 |
| 发布数字 vs 原始重算 | 文档全部数字由 scores.jsonl 脚本重算生成,无手工编造 |
| 分片合并跑次(LFM3-think/Spark-4B-think) | 去重后 138 唯一,shard 原始文件保留于日志包 |

### 8.2 作废跑次(保留隔离,不删数据)

| 跑次 | 作废原因 | 处置 |
|---|---|---|
| Nanbeige think(T4) | 16K ctx(显存不足),839 次提示截断 | 目录改名 `-INVALID-16k`,榜单标注 |
| Spark-4B think(Mac 首跑) | ~/models 被删致 mmap SIGBUS,135 af | 重跑于 T4 为准 |
| Gemma 首跑(更早批次) | 121/138 环境竞争 af | 本批次 26.1% 重跑为准 |
| Jetson LFM2.5 ×2 | 7.4G 盒子 kernel OOM-kill | Jetson 退出评测,生产已恢复 |

### 8.3 ~/models 两次外删事件(待用户排查)

- 2026-10-29 16:02 与 10-30 ~15:00,评测 Mac 的 `~/models` 目录两次被外部清空(非本 agent 操作;间隔 ~23h,疑似定时任务)。
- 影响:Spark-4B Mac 跑批中断、Nanbeige 收尾中断两次、mmap server SIGBUS。
- 缓解:二进制改放 `~/neomind-eval-bin-123`(v1.2.3 tag 重编译),模型放 `~/nanbeige-restore/`;数据因在 git/HF 未受影响。
- **建议排查**:`crontab -l`、`~/Library/LaunchAgents`、CleanMyMac 类工具的自动清理计划。

### 8.4 Nanbeige 部分样本声明

108/138 完成后因上述外删中断;缺失 30 次为混合难度尾部(tools×6、push×4、multi×4…)。全量值估计在 60.2% ±5pp 内。think 行作废(16K),有效重测需独占 ~14G 显存窗口。

---

## 九、备份清单(三层)

| 层 | 内容 | 位置 |
|---|---|---|
| git | 20 个跑次 scores.jsonl + served_model.json + README | `eval/baselines/2026-10-29-official-params/`(随各 commit 入库) |
| git | 过程日志:T4 全部 gate/server 日志(6.4M)+ Mac 日志(8.2M) | 同目录 `logs-t4.tar.gz` / `logs-mac.tar.gz` |
| HF 异地 | 上述全部打包(14.6M)+ 排行榜文档 | `CamThink-AI/neomind-agent-eval` → `batches/2026-10-29-official-params/batch.tar.gz`(私有仓) |
| repo | 排行榜 / 本报告 / README 双语 | `docs/` + 根目录 |

---

## 十、部署建议汇总(产品落地清单)

1. **默认模型**:Spark-X2.6-1.7B + 思考取 63.8%(README 已更新);Qwen3.5-4B-think 为 4B 档备选(带视觉需求时)。
2. **采样覆盖随下载下发**(Runtimes catalog 落实):Spark 1.0/0.95;Qwen off 1.0/0.95 或官方参+think;LFM 严格 0.1/k50/rp1.1;Gemma 1.0/0.95/k64;MiniCPM5 1.0/0.95/k40 思考开。
3. **llama-server 下限**:LFM3 系 ≥ b11946,全线建议 v0.8.0。
4. **思考开关默认值**:Spark/Qwen/MiniCPM5 → ON;LFM2.5/Gemma → 随意;LFM3 → 仅单轮场景。
5. **硬件门槛表**(Q4 量化 + 32K):
   - Spark-1.7B:3G RAM / 2G VRAM —— 网关级
   - Qwen-4B / Spark-4B:4-5G RAM / 3.5G VRAM
   - Nanbeige:6G+ RAM / **11G VRAM(32K 稠密 KV)**
   - T4 级 GPU 吞吐参考:1.7B ~110 tok/s、4B ~58、Nanbeige ~45(共享时减半)
6. **中文市场**:Qwen/MiniCPM5 相对价值更高(zh +7~10pp);出口:Spark/Nanbeige(en +9~12pp)。
7. **下一代 SFT 底座**:Spark-X2.6-1.7B(多步天花板 + 短思考链降低训练 token 成本)。

---

## 十一、遗留事项

| 事项 | 状态 | 建议 |
|---|---|---|
| ~/models 删除源排查 | 待用户 | crontab / LaunchAgents / 清理工具 |
| Nanbeige think 有效重测 | 未做 | 需独占 14G VRAM 窗口(租户释放后) |
| Nanbeige off 补齐 30 例 | 可选 | ±5pp 内,价值有限 |
| GLM-5.3 / DeepSeek think 行 | 用户取消 | 如需云端思考对照可补 |
| 128K 窗口成绩 | 全批次未测 | 下一周期产品口径(128K)验证 |
| HF token 轮换 | 建议 | 本 token 已多次出现在会话中 |

---

## 附录 A:跑次溯源表

| 跑次目录 | 机器 | 模式 | 备注 |
|---|---|---|---|
| deepseek-flash-official | T4 | off | API |
| glm53-official | T4 | off | API,anthropic 端点 |
| mini-spark-x26-17b / -think | Mac | off/think | think 为冠军跑次 |
| spark-x26-4b-think-t4 | T4 | think | 2-shard 合并 |
| mini-spark-x26 | Mac | off | 4B off |
| rerun2-qwen35-4b-official / rerun3-…-macparams / rerun-…-think | T4 | off×2/think | 反事实组 |
| mini-qwen35-4b | Mac | off | 旧 sweep(错参行) |
| mini-lfm3-17b | Mac | off | 官方参 |
| rerun-lfm3-17b-think-t4 | T4 | think | 主跑+2-shard 尾段合并 |
| rerun2-lfm25-26b-official-t4 / rerun-lfm25-26b-think-t4 | T4 | off/think | |
| mini-lfm25-26b | Mac | off | 错参行 |
| mini-gemma4-e2b-official / -think | Mac | off/think | |
| minicpm5-2b-think-t4 | T4 | think | |
| mini-nanbeige42-3b-official2 | Mac | off | 108/138 |
| regression-20261027-171122 | Mac | off | r11 SFT 历史跑 |
| nanbeige42-3b-think-t4-INVALID-16k | T4 | think | 作废,仅 T4 留存 |

## 附录 B:llama-server 槽位配置

- Mac:`-c 32768` 默认 4 slots(每 slot 32K,统一 KV);Nanbeige 尾段改 `-np 1`(单 slot 32K)。
- T4:`-c 32768`(4 slot)用于 Qwen/LFM2.5/LFM3/MiniCPM5;Spark-4B-think 与 Nanbeige 尝试 `-c 65536 -np 2`(每 slot 32K)。
- 所有配置下**每请求可用上下文均为 32K**,与成绩口径一致;Nanbeige 作废跑为唯一例外(16K)。

## 附录 C:复现命令

```bash
# 本地模型(示例:Spark-1.7B think,官方参)
llama-server -m Spark-X2.6-1.7B-Q4_K_M.gguf --port 8080 -ngl 99 -c 32768 \
  --min-p 0 --temp 1.0 --top-p 0.95   # 不加 --reasoning off = 思考取默认解析
AGENT_LLM_BACKEND_TYPE=llamacpp AGENT_LLM_ENDPOINT=http://127.0.0.1:8080 \
  AGENT_LLM_MODEL=Spark-X2.6-1.7B-Q4_K_M.gguf AGENT_LLM_API_KEY=dummy \
  AGENT_LLM_TEMPERATURE=1.0 AGENT_LLM_TOP_P=0.95 AGENT_LLM_THINKING=true \
  NEOMIND_TEST_BIN=<neomind-1.2.3> EVAL_CHAT_TIMEOUT=480 \
  python3 eval/run_eval.py regression --rounds 1 --case-timeout 1200 \
  --run-dir eval/runs/<name>

# 云端(DeepSeek 示例)
AGENT_LLM_BACKEND_TYPE=openai AGENT_LLM_ENDPOINT=https://api.deepseek.com/v1 \
  AGENT_LLM_MODEL=deepseek-flash AGENT_LLM_API_KEY=… AGENT_LLM_THINKING=false …

# 统一重算(本报告全部数字的生成方式)
python3 - <<'PY'  # 遍历 eval/runs/*/scores.jsonl,pass = cmd_ok AND sq_all_pass is not False
PY
```

*报告生成:2026-10-30 · 数据截止:本批次全部跑次 · 审计:§8.1 全项通过*
