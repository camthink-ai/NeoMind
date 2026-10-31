# c7fb11b5 重构回归证据包（2026-10-31，Mac 同机全量）

## 结论摘要（截至打包时刻，评估仍在运行）
- c7fb11b5 重构在同片用例上通过率 ~34%，距 1.2.3 基线（63.8% 全量）差距 ~-20pp，接近 1.2.4 回归量级（39.9%）
- 两种失败形态（详见下方归因）

## 已核实的根因（代码位置）
1. 意图引导无调用点：get_intent_prompt_addon 仍在 prompts/builder.rs:86，但全仓已无生产调用——系统提示不再携带任务域指引
2. 续轮上下文弱化：stream_core.rs:350 续轮注入只剩一句 "Continue the original user request... Report actual outcomes and any remaining work."——"Report outcomes"在鼓励提前收尾；验收版(e95f23e4/abc3afda)含轮次计数+已执行清单+STOP AND THINK 决策框架

## 失败形态（与痕迹对应）
- 形态A 一条命令后快速文本收尾：agri-auto-irrigate-north[en] 28s/1cmd/cmd_partial=0（基线多步通过）
- 形态B --help 爬行+思考停滞撞 120s chat 超时：building-co2-anomaly[en] 10条全是--help、63.9s stall

## 文件说明
- gate-trace.log        完整执行痕迹（每条命令+时间戳+LLM请求+判定行）
- scores.jsonl          c7fb11b5 每用例结构化判定（进行中快照）
- baseline-123-scores.jsonl  1.2.3 基线（63.8%）
- mac-124-scores.jsonl  1.2.4（39.9%，已知回归参照）
- new-loss-cases.json   基线过而 c7fb 挂的用例清单（含分项分数）
- model-server.log      llama-server 时序（token 数/速度）
- served_model.json     模型身份存档

## 复现命令（Mac，Spark-1.7B 官方参数）
AGENT_LLM_ENDPOINT=http://127.0.0.1:8080 AGENT_LLM_MODEL=Spark-X2.6-1.7B-Q4_K_M.gguf \
AGENT_LLM_BACKEND_TYPE=llamacpp AGENT_LLM_THINKING=true AGENT_LLM_TEMPERATURE=1.0 AGENT_LLM_TOP_P=0.95 \
AGENT_LLM_API_KEY=dummy NEOMIND_TEST_BIN=<binary> EVAL_CHAT_TIMEOUT=120 \
.venv/bin/python eval/run_eval.py run-case --case eval/cases/en/<dir>/<case>.json
