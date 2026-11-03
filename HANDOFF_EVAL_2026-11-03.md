> 交接原文,由断线前的云环境于 2026-11-03 发出,Mac 本地 GLM 接手时原样保存。
> 文中状态为断线前最后确认记录;接手后必须逐一核实,不得直接当作现状。

# NeoMind 综合评测与 harness 改良交接

## 目标与边界

继续提高 Spark-1.7B 在 NeoMind 上的综合任务完成率,验证 agent harness 改进的净收益。

- 保留仓库已有修改,不 reset、不覆盖、不清理未知任务。
- 不提交、不推送、不打 tag、不发布。
- 不把初筛结果当作综合成绩。
- 不向外部模型发送原始评测答案或业务证据。
- 密码、API key 不写入交接文件、代码或日志。
- 优先完成已有改进的验收,再决定下一轮修改。

## 一、接手位置

Mac 主仓库:

```text
/Users/shenmingming/CamThink Project/NeoMind
```

Mac 本轮研究目录:

```text
/tmp/neomind-comprehensive-20261103-085034
```

也可读取:

```bash
cat /tmp/neomind-comprehensive-dir
```

T4:

```text
ssh ubuntu@43.132.189.162
/tmp/neomind-t4-eval-20261103-67vlq1cb
```

Jetson:

```text
ssh box@192.168.93.20
/tmp/neomind-linux-ci-20261103-msim9c25
```

Mac 到两台机器此前已有可用的 SSH 密钥认证。优先使用现有配置,不修改主机密钥检查。

## 二、已完成的工作

Mac 最后确认的 HEAD:

```text
cead180138fb099a5050565ab40fb6f3888e2e11
```

已有 17 个文件的改进,主要包括:

- 摘要和上下文保留。
- 技能探索限制与后续继续执行。
- 重复工具结果处理、循环终止和自然语言收尾。
- shell 执行信息与工具证据。
- llama.cpp thinking 参数传递。
- CLI 配置保留和资源 ID 提示。
- LLM 后端删除前验证真实 ID,避免使用名称时误报删除成功。

最后两处新增修改:

```text
crates/neomind-cli-ops/src/llm.rs
crates/neomind-cli-ops/src/api_client.rs
```

删除修复的行为:

1. 先 GET 验证后端 ID。
2. 验证失败,不执行 DELETE。
3. 提示运行 `neomind llm list` 获取真实 ID。
4. DELETE 本身失败时保留错误,不报告成功。

验证记录:

| 检查 | 最后确认结果 |
|---|---|
| Mac workspace 全量测试 | 3703 通过,0 失败,69 忽略 |
| 新增删除修复 HTTP 测试 | 4/4 通过 |
| Jetson 原有改动验证 | 194 项通过 |
| Jetson 新增两文件修复 | 尚未补测 |

Mac 全量日志:

```text
/tmp/neomind-final-gate-v2.log
```

## 三、评测结果与正确解释

已完成一轮小样本筛选,每组 24 个任务语言样本:

| 版本 | 通过数 |
|---|---:|
| 原基线 | 17/24 |
| 仅 CLI 改进 | 18/24 |
| 完整 harness | 20/24 |

完整 harness 相对 CLI 组在这批样本中增加两项通过,没有新增失败。

**这只是初筛,不能据此声称全量综合分已经提升。**

原先的 828 次实验已主动停止。保留其记录,不恢复旧队列、不把容量试验或未完成结果混入正式验收。

## 四、第一优先级:检查 T4 资源和已有构建

用户已授权暂停 T4 上 NeoMind 的资源来进行评测。

断线前:

- `neomind.service` 已停止。
- 其他两个 Docker Python GPU 进程属于无关任务,禁止停止。
- 自建测速模型服务已经停止。
- v1 两组构建完成。
- v2 构建已启动,最终完成状态未知。
- **552 次正式评测没有确认启动,不能假定正在运行。**

首先检查:

```bash
ssh ubuntu@43.132.189.162 \
  'systemctl is-active neomind.service; nvidia-smi'
```

在 T4 研究目录核实:

```text
build-v2-progress.json
build-v2-failed.json
builds-v2.json
build-complete-v2.json
build-v2-controller.log
python-deps-result.json
acceptance-launch.json
restoration-result.json
```

预期 v2 二进制:

```text
binaries/cli_only-v2
binaries/full-v2
```

不能只看旧 PID 判断任务状态;结合进程命令、日志时间及结果文件。

若暂时无法继续验收,先确认没有自建评测模型占用 GPU,再恢复原服务:

```bash
sudo -n systemctl start neomind.service
```

## 五、正式验收:552 次同机配对实验

只比较两个版本:

- **CLI 基线**:包含全部共享 CLI 修复。
- **完整 harness**:相同 CLI 修复,加上 harness 改进。

新增删除修复必须同时进入两组,避免把 CLI 收益算成 harness 收益。

实验设计:

```text
69 个任务 × 2 种语言 × 2 轮 × 2 个版本 = 552 次
```

要求:

- 两组都在 T4 上运行。
- 使用同一模型文件、运行时与参数。
- 每批 6 个样本,交替版本顺序,第二轮反转顺序。
- 每个案例使用独立数据库和 API 实例。
- 超时计失败,不挑选性重跑。
- 保留实际模型请求参数、工具过程、原始结果和时间数据。
- 不把 Mac 与 Jetson 各跑一半的结果合并为同机归因结论。

模型:

```text
/var/lib/neomind/data/models/sparkx2.6-1.7b/sparkx2.6-1.7b-q4_k_m.gguf
```

模型 SHA256:

```text
7abd9859c10924be3c583945456fa9b25d0569484876105a7b7376f7938fffb7
```

运行时:

```text
/var/lib/neomind/data/llama-server/b12024-cuda/llama-server
```

已有控制器:

```text
/tmp/neomind-t4-eval-20261103-67vlq1cb/neomind-t4-acceptance.py
```

已有材料:

```text
snapshot/eval/
venv/
study-plan.json
revision-v2-input.json
```

**启动前必须审查控制器,不能直接盲跑。**

重点检查:

1. 使用 v2 二进制及其校验值。
2. Python 依赖和 `run_eval.py --help` 正常。
3. 138 个语言案例文件与冻结哈希一致。
4. 计划中的编译器、机器信息更新为 T4 实际值。
5. 端口没有冲突。
6. 清理路径只终止本轮创建的进程。
7. 正常结束和异常退出均恢复原 `neomind.service`。
8. 启动文件和结果目录不存在冲突,避免重复启动。

控制器预期读取:

```text
acceptance/builds.json
acceptance/build-complete.json
acceptance/study-plan.json
```

应分别对应 v2 构建元数据和正式计划。Bootstrap 脚本在云环境断线时尚未确认写入完成,需要检查是否存在,缺失则补齐。

## 六、补完 Jetson 验证

Jetson 的 194 项通过结果覆盖此前 15 文件改动,不包含新增删除修复。

在原私有 CI 源码目录中:

1. 验证已有源码版本。
2. 应用新增两文件补丁 `cli-identity-v2.patch`。
3. 验证 17 文件源码哈希。
4. 运行 CLI 库测试:

```bash
cargo test --locked -p neomind-cli-ops --lib
```

复用原私有构建缓存,控制并发,保留之前的测试结果。无需重新执行全部已通过的 194 项。

## 七、结果审计与后续改进

正式结果必须报告:

- 两组总通过率和差值。
- 按中文、英文和任务领域划分的表现。
- 配对新增通过、退步案例。
- 超时率、循环失败、错误成功报告。
- 延迟和 token 数据的覆盖率。
- 按任务聚类的置信区间。
- 缺失记录和实验异常。

注意:评测器超时时可能缺少部分工具轨迹,不能把缺失轨迹解释为“零工具调用”。

已有分析脚本可参考:

```text
/tmp/comprehensive_analysis.py
/tmp/neomind-local-audit.py
```

Mac 版本可能仍是三组比较逻辑;两组支持修改可能只存在于失联的云环境。先检查,再适配,并通过相应测试。

后续代码优化根据完整失败案例决定,优先关注:

1. 错误资源标识和工具错误后的恢复能力。
2. 无效重复探索与长时间循环。
3. 压缩后任务目标和证据丢失。
4. thinking 策略与真实请求参数是否一致。
5. 最终回答是否得到实际执行证据支持。

不要为了某几个固定案例硬编码行为,也不要用机械模板代替所有自然语言收尾。

## 八、完成标准

只有满足以下条件才报告本轮完成:

- v2 两组构建和必要测试通过。
- 552 次结果完整,实验配置和源码可追溯。
- 独立复核通过数、退步案例和缺失记录。
- 明确说明综合收益是否成立,以及不确定性。
- 自建评测进程清理完毕。
- 原 NeoMind 服务恢复并检查健康状态。
- 保存最终报告与复现命令。
- 不提交、不推送、不发布。

**接手后先汇报实际进程、v2 构建状态和正式评测是否启动,再推进未完成步骤,不重复已确认完成的工作。**
