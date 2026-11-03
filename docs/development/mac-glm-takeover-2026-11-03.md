# Mac 本地 GLM 接手记录 — 2026-11-03

云环境断线后,Mac 本地 GLM 按 `HANDOFF_EVAL_2026-11-03.md`(仓库根,交接原文)接手。本文件记录核实结果、补齐的工作与当前活跃状态。**所有状态均为本地实测,不是转述。**

## 一、核实结果(接手时,2026-11-03 14:10 前后)

### Mac
- HEAD = `cead180138fb099a5050565ab40fb6f3888e2e11`,与交接一致;17 文件未提交改动完好(含 `cli-ops/llm.rs`、`cli-ops/api_client.rs` 两处新增修复)。
- 828 控制器 PID 38647 与审计 PID 45773 **均不在运行**。研究目录 `/tmp/neomind-comprehensive-20261103-085034` 中 `screen/stop-request.json` + `run-failed.json`(exit=-15)证实为**主动停止**,与交接描述一致;AGENTS.md 中"controller 38647 remains active"为过期记录(已由本文件更正,见下)。
- 无本轮遗留推理进程;`llama-server`/`neomind serve` 残留进程均为用户自有服务(早于本轮),未触碰。

### T4(ubuntu@43.132.189.162,/tmp/neomind-t4-eval-20261103-67vlq1cb)
- `neomind.service`:inactive(11:59:52 停止,`release-result.json` 记录了 stop 前状态与恢复命令)。
- GPU:仅两个无关 Docker Python 进程(2725541、3791136,约 8.3G),按授权不动;自建测速 provider 已于 13:37 停止。
- **v2 两组构建完成且校验通过**:`build-complete-v2.json` complete=true、shared_cli_identity_fix=true、source_verified=true;`binaries/cli_only-v2`、`binaries/full-v2` 存在,SHA256 与 `builds-v2.json` 一致;两组的 5 个 CLI 文件哈希完全相同(删除修复确实同时进入两组)。
- Python 依赖 returncode=0;`run_eval.py --help` 正常(venv Python 3.10.12)。
- **552 次正式评测未启动**:`acceptance-launch.json`、`restoration-result.json` 均缺失,无评测进程,`acceptance/` 目录不存在(bootstrap 确实未写完)。

### Jetson(box@192.168.93.20,/tmp/neomind-linux-ci-20261103-msim9c25)
- 194 项通过记录完好(complete.json,15 文件范围,source_verified=true)。
- `cli-identity-v2.patch` 原先**不在** Jetson;Mac 与 T4 两份 SHA256 相同(`2d11a5a4…54ab`),已推送至 Jetson 并复核一致。

## 二、控制器审查(8 项检查清单)

`neomind-t4-acceptance.py`(sha256 `b68b8fac…b412`,**未修改**)逐条审查:

1. v2 二进制及校验:控制器启动时对 `acceptance/builds.json` 每个二进制断言 SHA256 ✓
2. Python 依赖 / run_eval --help:实测通过 ✓
3. 138 案例哈希:`plan['source_sha256']` 覆盖评测器源码**及全部案例文件**,控制器全量断言;bootstrap 又独立预校验了一遍 ✓
4. 机器/编译器信息:**发现 study-plan.json 中 rustc/cargo 还是 Mac Homebrew 1.98.0 值** → bootstrap 用 T4 构建时记录的 `build-environment.json`(rustc/cargo 1.92.0 x86_64)替换,并写入 `acceptance_host` 溯源块;采样/判据/案例哈希未动 ✓
5. 端口:llama-server 固定 36365,控制器启动前先探测绑定;实测空闲;代理/MQTT 用临时端口 ✓
6. 清理:仅 terminate/killpg 本轮自建进程组,不触碰他进程 ✓
7. 恢复:`finally: restore_original_service()` 覆盖正常/异常/SIGTERM 退出,按 `release-result.json` 的 before 状态执行 `sudo -n systemctl start neomind.service` ✓(SIGKILL/断电不覆盖,属残余风险,已在启动记录注明 verify_before_signaling)
8. 防重复启动:bootstrap 断言 `acceptance/`、`acceptance-launch.json`、`restoration-result.json` 不存在;控制器 `trials.mkdir()` 对已存在目录会失败 ✓

## 三、本轮新增产物(全部在研究目录/仓库,未提交)

| 位置 | 文件 | 说明 |
|---|---|---|
| T4 ROOT | `neomind-t4-acceptance-bootstrap.py` | 补写的 bootstrap:全前置校验 + 生成 `acceptance/{builds,build-complete,study-plan}.json` |
| T4 ROOT | `acceptance-bootstrap.json` | 校验报告(全部通过;GPU 前置快照、采样参数记录在内) |
| T4 ROOT | `acceptance-launch.json` | 启动记录:PID 439563、控制器/脚本 SHA256、UTC 时间 |
| T4 ROOT | `acceptance-controller.log` | 控制器 stdout(DONE 每例一行) |
| Jetson | `cli-identity-v2.patch`、`revision-v2-input.json` | 从 Mac 推送,哈希一致 |
| Jetson | `jetson-v2-verify.py`、`cli-identity-v2-launch.json` | 补丁前后 15→17 文件哈希校验(均通过)与测试启动记录 |
| 仓库根 | `HANDOFF_EVAL_2026-11-03.md` | 交接原文 |
| docs/development/ | 本文件 | 接手记录 |

## 四、当前活跃状态(截至 2026-11-03 ~14:20)

- **T4 552 次正式评测运行中**:PID 439563(启动 06:16:55Z),cohort r0-g00(cli_only 先行,批 6 交替、第二轮反序),前 5 例 87 秒完成(5 pass),模型 ~118 tok/s,GPU 显存 1894 MiB。单例含超时上限 600s;全程预计十小时级。
- **Jetson CLI 库测试运行中**:PID 44024(`cargo test --locked -p neomind-cli-ops --lib -- --test-threads=2`)。注意:默认工具链解析为 1.92.0,与 194 项当时记录的 1.99.1 不同,构建缓存未复用、需全量编译(不影响源码哈希定义的正确性);结果看 `cli-identity-v2-test.log`。

## 五、监控与后续

```bash
# 进度
ssh ubuntu@43.132.189.162 'cat /tmp/neomind-t4-eval-20261103-67vlq1cb/acceptance/progress.json'
ssh ubuntu@43.132.189.162 'ps -p 439563 -o pid,etime,command'   # 控制器存活
ssh ubuntu@43.132.189.162 'tail -3 /tmp/neomind-t4-eval-20261103-67vlq1cb/acceptance-controller.log'
# Jetson 测试
ssh box@192.168.93.20 'tail -5 /tmp/neomind-linux-ci-20261103-msim9c25/cli-identity-v2-test.log'
```

- 正常终点:`acceptance/run-complete.json`(552/552)+ `restoration-result.json`(服务恢复)。异常:`run-failed.json` + 服务仍会在 finally 恢复。
- 需要中止时:`kill -TERM 439563`(优雅停止,单例等待最长 ~12 分钟;**先 `ps -p` 核实 PID 仍是该 python 再发信号**)。
- 完成后待办:独立复核(两组通过率/差值、按语言与领域、配对进退、超时率、覆盖率、按任务聚类置信区间、缺失记录);`/tmp/comprehensive_analysis.py` 为三组逻辑,需适配两组并通过测试后再出正式报告;结论遵守交接边界(不做外部语义判分宣称、不提交不发布)。

边界遵守情况:未 reset/覆盖仓库改动;未提交/推送/发布;未向外部模型发送评测答案;无凭据写入;未触碰无关 Docker GPU 进程;`neomind.service` 保持暂停至评测结束由控制器自动恢复。

## 八、评分契约核实 + rule update 假成功根因(2026-11-03 深夜,用户复核指示)

### 评分器修正(两臂统一,原始结果保留)
- **widget 复合命令缺陷确认**:`hard_signal._actual_from_record` 只按 `;` 切分,`&&` 链中的 neomind 命令不可见(`normalize_command` 取整条命令前 3 个有效 token)。
- **统一重算**:对全部 552 条,把 shell 命令按 `&&`/换行预切分后用原版 compute 重评(两臂同规则)。**仅 2 条判决变化(full 臂 widget en r0/r1,False→True),cli 臂零变化**。修正版:full 208/276(75.4%),差值 +5.80pp,增 33/退 17,任务聚类 [1.09,10.87]、执行组 [0.36,11.23]。**原始 69.6%/74.6% 与原始 results.jsonl 保留为正式结果;修正版仅作参考(graded-revised.json),两轮不对称(r0 +2/r1 +14)仍在,不作为"已确认提升"的新依据。**
- **file-edit 契约核实**(2026-11-04 更正):案例冻结的 state_query 要求最终回复含文件名;full 臂文件操作最终正确,仅败于交付措辞——按冻结契约合法失败,未翻转。**原始轨迹中确实发生过请求路径被沙箱拒绝、模型随后改用允许目录成功**(前文"未出现路径不匹配"为误报,以此更正);路径被拒不是最终失败原因。文件名判据属冻结契约,该失败保留。

### rule update 契约修复已实现(2026-11-04 凌晨,用户细化规格)
- **规格**(用户 2026-11-04):未知字段(含合法+未知混合)→ 结构化 400 列出未知与支持字段,整单不写入;{} 或无受支持字段 → 400;字段类型错误 → 明确拒绝;幂等同值更新 → 允许成功,返回 changed=false。
- **实现**:crates/neomind-api/src/handlers/rules.rs update_rule_handler——入口处未知字段拒绝(允许键 = 7 个更新字段 + CompiledRule 透传键 id/tags/cooldown/for_duration/state/dsl_preview/created_at/updated_at,前端完整 PUT 不受影响);类型校验;部分更新路径先比对现值,无变化直接返回 changed=false 且**不写引擎、不动 updated_at**;变化路径返回 changed=true。
- **测试**:tests/handlers/rules.rs 15 项全过——红色测试转正(原始轨迹 body 被拒 + 错误列出 trigger_device + **规则与 updated_at 均未变**)、空 body 拒绝、类型错误拒绝、幂等同值成功 changed=false 且 updated_at 不动、元数据键放行、改名控制(changed=true,condition 不受扰)。neomind-api 全量 603 通过/0 失败。
- **下一步(未做)**:workspace 全量门禁 → full+fix 候选构建 → 配对筛选(multi-turn-correction 中英 + rule 类案例 + 对照,双臂交错)——按用户排序"先证明错误被准确拒绝(✓),再看 Spark 能否依据错误恢复";接口修复有效不预设 eval 涨分。

### 评分器解析器 v2 与修正版重算(2026-11-04 凌晨)
- grader_split_v2.py:引号感知切分;纯 `&&` 链 exit 0 全部 ok、exit≠0 保守全败;`;`/换行按"末段 exit"归因,前段记为不可证(不计 ok 也不计 failed);`||` 不可证。8/8 单测。
- 统一重算 v2(两臂同规则,原始不动):**3 条变化**——widget en r0/r1 False→True(tar && install 全链 exit 0,后续 list/uninstall 输出佐证);**config-export-validate en r1 True→False:原评分器被 `validate…; echo "EXIT_CODE=$?"` 的末段 exit 0 骗过,轨迹里 validate 实际 4 次全败(3×exit1 + 422),stdout 明确打印 EXIT_CODE=1**——用户预警的"失败命令;成功命令"语义缺陷在真实数据中坐实。修正版 v2:full 207/276(75.0%),+5.43pp,增32/退17,任务聚类 [0.72,10.51]、执行组 [0.00,10.87]。
- 并列报告:T4 `grading-addendum.md`(冻结为正式,修正版如实披露为事后修正,非新独立实验)。产物 graded-revised-v2.json。


### rule update 假成功(根因 + 回归测试已钉死)

- **轨迹证据**(full--multi-turn-correction--en--r0):flag 形式 `--trigger-device` 直接 clap exit 2;模型改用 `--body` 提交 create 风格字段名(trigger_device/metric/operator/threshold/notify)。
- **根因**:`update_rule_handler`(crates/neomind-api/src/handlers/rules.rs:434)只识别 name/description/enabled/source/condition/trigger/actions,未知字段静默丢弃;部分更新路径把同名 name 重写一遍,finalize() 只改 updated_at,返回 success + 原规则。CLI `--body` 原样透传。次要问题:CLI 输出双层 success/data 包裹。
- **回归测试**:crates/neomind-api/tests/handlers/rules.rs 新增 `test_update_rule_rejects_unknown_field_noop_body`(#[ignore] 红色测试,`-- --ignored` 运行今日按预期失败,精确复现轨迹行为)+ 正向控制 `test_update_rule_partial_rename_preserves_condition`(通过:改名生效且 condition 不受扰)。
- **修复定位为迭代第一轮机制**:更新端点拒绝未知/空操作字段(结构化 400 列出可识别字段),走配对筛选→完整回归→保留/撤回;不做命令核对类评测导向逻辑。第 3 项(agent 对"成功但状态未变"的恢复)按用户排序在其后单独验证。



## 七、552 最终结果与退化归因(2026-11-03 深夜)

### 结果(最终审计 552/552 通过,产物在 T4 acceptance/)
cli_only 192/276(69.6%)vs full 206/276(74.6%),配对 +5.07pp,增 33/退 19,中文 99→107、英文 93→99。任务聚类 95% [0.00,10.51],执行组聚类 [-0.36,10.87]——实测提升、统计确认不足。重复调用 734→326、总调用 -19%。

### 19 次配对退化归因(证据:regression-forensics.json,guard 量化)
- **技能过滤(skill_recovery)危害,1 例确凿**:guided-mqtt-onboarding zh r1——预算耗尽触发 skill_recovery(技能被排除)后,模型失去接入指南,乱创 device type 并循环重试 device create 直至失败;cli 臂同案例通过。skill_recovery 在 full 臂共触发 10 次、通过率仅 40%(未触发时 78.7%)。
- **过早收尾/欠执行,2-3 例**:multi-turn-correction zh(failed_cmds 为空=期望命令根本没执行就总结成功)、agent-conversation en(该行动时反问用户,incomplete_action_note 触发)、metric-discovery-rule en。
- **报错后恢复路径发散,2-3 例**:widget-full-lifecycle zh(full 臂失败后手工 tar/python-zipfile 乱试)。
- **噪声级内容差异,多数**:tools-file-edit en(两臂工具序列完全相同、无失败命令,纯内容差异)、micro-rule-create、push-update 等——不宜归因机制。
- **参数漂移假设排除**:主请求两臂 thinking 等效(cli 不发送 override=模板默认 true;full 显式 true);full 臂的 thinking=False 请求均为设计内的辅助请求(摘要等)。

### 第一轮迭代候选机制(待按协议做配对筛选)
优先候选:**skill_recovery 的技能排除路径**——证据最强(唯一有直接因果轨迹的机制),改善方向:预算耗尽后不剥夺指南访问,而是改变重复加载的代价(如最后请求附带指南摘要/保留一次带技能机会)。次候选:错误后恢复引导(widget 型)。明确缓办:完成前命令核对(用户否决)。

### 基线冻结
下一轮基线 = full-v2(二进制 sha 30c3cccf…,源码=Mac 17 文件未提交 diff,T4 builds-v2.json)。552 完整结果与 552 个 trial 保留于 T4 acceptance/,不得覆盖。

## 六、552 结束后的迭代协议(用户指令,2026-11-03)

全量对照出结果后**不就此结束**:

1. **若无可靠提升**(按任务聚类的置信区间跨零即视为未确认):从完整轨迹归因主要失败类型(原始请求、工具调用、原始结果、期望状态、最终回复;分数本身不解释失败)。
2. **每轮只改一个明确机制**,先做配对筛选(受影响失败案例+正常运作对照,中英双语,双臂交错),再完整回归;保留有效改动,撤回有害改动,持续迭代。
3. **硬约束:不得修改评分规则、不得放宽超时、不得删除或弱化难例来制造涨分。** 评分器、案例集、模型/运行时/采样在各臂间保持冻结。
4. 全程遵循 [agent-eval-method.md](agent-eval-method.md)(冻结基线、独立复核、撤回无用扩展、恢复备份后刷新 mtime 并验证哈希等)。

**T4 服务临时调整(2026-11-03 16:1x,用户指令)**:应用户要求提前启动 `neomind.service`(平台可用),但通过临时 drop-in `/etc/systemd/system/neomind.service.d/zz-eval-builtin-llm-off.conf` 设置 `NEOMIND_BUILTIN_LLM=off`,**内置 LLM 不拉起**,GPU 仍专属于 552 评测(启动后核实:无新增 GPU 进程,评测正常推进)。记录在 T4 `service-manual-start.json`。**评测结束的恢复清单除原步骤外,还需:删除该 drop-in → `daemon-reload` → 重启 `neomind.service`,让内置 LLM 回到原始启用状态。**控制器收尾的 `systemctl start` 幂等,不受影响。

### 待接入:第三台 Jetson(192.168.93.4,16GB,账号 zbing)

用户于 2026-11-03 提供第三台 16GB Jetson(IP 192.168.93.4,账号 zbing,密码仅口头提供,未写入任何文件)。**接入受阻**:Mac(192.168.31.0/24)经现有通路可达 .20;从 .20 局域网内部探测 192.168.93.4 时 ARP FAILED,93.x 邻居表与 Tailscale 网内均无此设备——该地址当前不在线(未开机/未连网/IP 不符,93.x 邻居中 192.168.93.220 的 MAC 04:04:04:20:20:20 形似手工配置设备,或为线索)。设备上线后:一次性注入 Mac 公钥(密钥认证,弃用密码),只读盘点(型号/内存/JetPack/rust/neomind 部署/在跑进程),再按协议分配角色(第二硬件验证主机或迭代期筛选/构建容量);在其角色明确前不得混入 T4 归因数据。

### 已就绪的分析工具(2026-11-03 14:3x 补充)

- `t4_paired_analysis.py`(T4 ROOT,Mac 副本 /tmp/t4-takeover-20261103/):两组严格配对比较,复用原三组版全部统计逻辑(Wilson、按任务聚类 bootstrap、执行组块 bootstrap、缺失/重复/串组拒绝)。
- `t4_acceptance_audit.py`:两组审计,逐条独立复核(原生判据一致性、`hard_signal.compute` 独立重评分、案例哈希、轨迹/wire 指标、覆盖率口径),final 门禁(552 完整 + 二进制/评测器哈希)通过后才产出 `acceptance/native-audit-records.json` 与 `acceptance-report.md`。
- 测试:`test_t4_paired_analysis.py`(8 项)+ `test_t4_acceptance_audit.py`(9 项)在 Mac 与 T4 双端全部通过;审计测试默认 T4 根,Mac 运行需 `NEOMIND_T4_EVAL_ROOT=/tmp/neomind-comprehensive-20261103-085034`。
- T4 实时审计 watcher 运行中(PID 见 `acceptance-audit-launch.json`,日志 `acceptance-audit-watch.log`),每 30s 增量复核并写 `acceptance/local-audit.json`;控制器失败则审计保持部分状态并报错。
- Mac 侧挂有后台监视器(caffeinate + ssh 阻塞轮询控制器 PID),控制器退出即接续做全量对照。
