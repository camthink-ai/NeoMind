// 引擎基准：规则引擎（评估吞吐/触发时延） + 转换引擎（JS 执行延迟）
// 用法: node engines-bench.mjs --rules 50 --msgs 500 --transform 200
import fs from "fs";
const args = Object.fromEntries(process.argv.slice(2).map((s, i, a) =>
  s.startsWith("--") ? [s.slice(2), a[i + 1]?.startsWith("--") ? true : (a[i + 1] ?? true)] : []).filter(([k]) => k));
const API = "http://127.0.0.1:9375";
const N_RULES = parseInt(args.rules ?? "50");
const N_MSGS = parseInt(args.msgs ?? "500");
const N_TF = parseInt(args.transform ?? "200");
const OUT = fs.createWriteStream(args.out ?? "engine-samples.jsonl");
const log = (op, ms, ok = true, extra = {}) => OUT.write(JSON.stringify({ op, ms: Math.round(ms * 10) / 10, ok, ...extra, t: Date.now() }) + "\n");
const login = async () => (await (await fetch(`${API}/api/auth/login`, { method: "POST",
  headers: { "Content-Type": "application/json" }, body: JSON.stringify({ username: "admin", password: "smoke-test-123" }) })).json()).token;
const token = await login();
const api = (p, o = {}) => fetch(`${API}${p}`, { ...o, signal: AbortSignal.timeout(10000),
  headers: { "Content-Type": "application/json", Authorization: "Bearer " + token, ...o.headers } }).then(r => r.json());

// ================= 转换引擎：JS 每次调用的编译+执行延迟 =================
console.log(`[transform] ${N_TF} 次 test-code 调用...`);
const code = `function transform(data) { const out = {};
  for (const [k, v] of Object.entries(data)) { if (typeof v === 'number') out[k] = v * 1.8 + 32; else out[k] = v; }
  out.__computed = true; return out; } transform(input)`;
{
  const input = { temperature: 22.5, humidity: 61, pressure: 1013, extra: "text" };
  for (let i = 0; i < N_TF; i++) {
    const t = Date.now();
    try { const r = await api("/api/automations/transforms/test-code", { method: "POST",
      body: JSON.stringify({ code, input_data: input, output_prefix: "tf" }) });
      log("transform", Date.now() - t, !!r?.success);
    } catch { log("transform", Date.now() - t, false); }
  }
}

// ================= 规则引擎：建规则 → 遥测风暴触发 → 清理 =================
console.log(`[rules] 建 ${N_RULES} 条规则...`);
const DEV = "bench_sensor_0000";
const ruleIds = [];
for (let i = 0; i < N_RULES; i++) {
  const t = Date.now();
  try {
    const r = await api("/api/rules", { method: "POST", body: JSON.stringify({
      name: `bench_rule_${i}`, enabled: true,
      trigger: { trigger_type: "data_change", device_id: DEV, metric: "temperature" },
      condition: { condition_type: "comparison", source: `device:${DEV}:temperature`, operator: ">", value: 20 },
      actions: [{ type: "notify", message: "bench hit {value}" }],
    })});
    const rid = r?.data?.rule?.id ?? r?.data?.id;
    log("rule-create", Date.now() - t, !!r?.success);
    ruleIds.push(rid);
  } catch { log("rule-create", Date.now() - t, false); }
}

// 遥测风暴：向 DEV 快速发 N_MSGS 条，测 (a) 触发评估是否拖慢入库 (b) 规则效果可观察
import mqtt from "mqtt";
console.log(`[rules] ${N_MSGS} 条遥测风暴...`);
const c = mqtt.connect("mqtt://127.0.0.1:1883", { clientId: "bench-rules-1" });
await new Promise(r => c.on("connect", r));
{
  let sent = 0;
  for (let i = 0; i < N_MSGS; i++) {
    c.publish(`device/bench_sensor/${DEV}/uplink`,
      JSON.stringify({ device_id: DEV, temperature: 25 + Math.random(), humidity: 50 }), { qos: 0 });
    sent++;
  }
  // 风暴后测入库可见（应不受规则评估拖累）
  await new Promise(r => setTimeout(r, 3000));
  const t = Date.now();
  const r = await api(`/api/devices/${DEV}/telemetry?limit=1&hours=1`);
  log("post-storm-query", Date.now() - t, !!r?.success);
}
c.end(true);

// 规则触发时延：单条发布 → 轮询规则最近触发时间（如果有 stats）
// 规则触发验证：等评估完成后查 trigger_count
await new Promise(r => setTimeout(r, 4000));
let hits = 0, hitQ = [];
for (const rid of ruleIds.filter(Boolean).slice(0, 5)) {
  const t = Date.now();
  const r = await api(`/api/rules/${rid}`).catch(() => null);
  hitQ.push(Date.now() - t);
  hits = Math.max(hits, r?.data?.rule?.trigger_count ?? r?.data?.trigger_count ?? 0);
}
console.log(`规则触发数(前5条中最大): ${hits} | 规则查询延迟: ${Math.min(...hitQ)}-${Math.max(...hitQ)}ms`);

// 清理
console.log("[rules] 清理...");
for (const id of ruleIds) if (id) await api(`/api/rules/${id}`, { method: "DELETE" }).catch(() => {});
OUT.end();
const sum = (f) => { const lines = fs.readFileSync(args.out ?? "engine-samples.jsonl", "utf8").trim().split("\n")
  .map(l => { try { return JSON.parse(l) } catch { return null } }).filter(Boolean).filter(f);
  const v = lines.map(x => x.ms).sort((a, b) => a - b);
  return { n: v.length, ok: lines.filter(x => x.ok).length, p50: v[v.length >> 1], p95: v[Math.floor(v.length * .95)] ?? v[v.length - 1], max: v[v.length - 1] }; };
console.log("== 汇总 ==");
console.log("transform:", JSON.stringify(sum(x => x.op === "transform")));
console.log("rule-create:", JSON.stringify(sum(x => x.op === "rule-create")));
process.exit(0);
