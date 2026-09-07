// MQTT 设备模拟器 — NeoMind 核心业务负载基准：设备接入/遥测/命令往返
//
// 契约: 内置 broker :1883, 上行 device/{type}/{id}/uplink, 下行 .../downlink
// 用法: node mqtt-devices.mjs --n 100 --interval 15 --duration 180 \
//         [--type bench_sensor] [--approve] [--cmd-rate 0.05] [--out samples.jsonl]
// 输出: 每操作一行 JSON {op, ms, ok, t} → report.mjs 汇总
import mqtt from "mqtt";
import fs from "fs";

const args = Object.fromEntries(process.argv.slice(2).map((s, i, a) =>
  s.startsWith("--") ? [s.slice(2), a[i + 1]?.startsWith("--") ? true : (a[i + 1] ?? true)] : []).filter(([k]) => k));
const N = parseInt(args.n ?? "50");
const INTERVAL = parseFloat(args.interval ?? "15");
const DURATION = parseFloat(args.duration ?? "120");
const TYPE = args.type ?? "bench_sensor";
const APPROVE = !!args.approve;
const CMD_RATE = parseFloat(args["cmd-rate"] ?? "0");
const API = args.api ?? "http://127.0.0.1:9375";
const OUT = fs.createWriteStream(args.out ?? "samples.jsonl");
const log = (op, ms, ok = true) => OUT.write(JSON.stringify({ op, ms: Math.round(ms * 10) / 10, ok, t: Date.now() }) + "\n");

const login = async () => {
  const r = await fetch(`${API}/api/auth/login`, { method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username: args.user ?? "admin", password: args.pass ?? "smoke-test-123" }) });
  const j = await r.json();
  return j.token ?? j.data?.token;
};
const api = (path, token, opts = {}) => fetch(`${API}${path}`, { ...opts,
  signal: AbortSignal.timeout(5000),
  headers: { "Content-Type": "application/json", Authorization: "Bearer " + token, ...opts.headers } }).then(r => r.json());

const token = await login(); console.error("[trace] login ok");
const t0 = Date.now();

await api("/api/device-types", token, { method: "POST", body: JSON.stringify({
  device_type: TYPE, name: "Bench Sensor",
  metrics: [
    { name: "temperature", display_name: "温度", data_type: "float", unit: "°C" },
    { name: "humidity", display_name: "湿度", data_type: "float", unit: "%" },
  ]})}).catch(() => {});
const registerOne = async (id) => {
  const t = Date.now();
  try {
    const r = await api("/api/devices", token, { method: "POST", body: JSON.stringify({
      device_type: TYPE, device_id: id, name: id, adapter_type: "mqtt", connection_config: {} })});
    log("register", Date.now() - t, !!(r?.success ?? r?.data?.added ?? true));
  } catch { log("register", Date.now() - t, false); }
};
for (let i = 0; i < N; i++) await registerOne(`${TYPE}_${String(i).padStart(4, "0")}`);
console.error("[trace] registered " + N);
let pubs = 0, errs = 0, cmdRTTs = [];

// ---- 自动审批（同时覆盖自动注册业务流）----
const approveAll = async () => {
  try {
    const list = await api(`/api/devices?status=pending`, token).catch(() => api(`/api/devices`, token));
    const items = (Array.isArray(list) ? list : list?.data ?? [])?.filter?.(d => (d.status ?? d.state ?? "") === "pending" || (d.id ?? "").startsWith(TYPE)) ?? [];
    for (const d of items) {
      if (!(d.id ?? "").includes("bench_")) continue;
      await api(`/api/devices/${d.id}/approve`, token, { method: "POST" }).catch(() => {});
    }
  } catch {}
};

// ---- 设备实例 ----
const devices = [];
for (let i = 0; i < N; i++) {
  const id = `${TYPE}_${String(i).padStart(4, "0")}`;
  const dev = { id, down: null, tPub: 0 };
  const tc = Date.now();
  const c = mqtt.connect("mqtt://127.0.0.1:1883", { clientId: "bench-" + id, clean: true, reconnectPeriod: 2000 });
  c.on("connect", () => {
    log("connect", Date.now() - tc);
    c.subscribe(`device/${TYPE}/${id}/downlink`);
  });
  c.on("message", (_t, msg) => {           // 命令到达（平台→设备）
    try { const m = JSON.parse(msg.toString());
      if (m.__bench_ts && dev.tCmd) { cmdRTTs.push(Date.now() - dev.tCmd); dev.tCmd = 0;
        c.publish(`device/${TYPE}/${id}/uplink`, JSON.stringify({ device_id: id, __cmd_ack: m.__bench_ts, temperature: 21, humidity: 55 })); }
    } catch {}
  });
  c.on("error", () => { errs++; });
  dev.client = c;
  devices.push(dev);
}

await new Promise(r => setTimeout(r, 3000));          // 连接建立
console.error("[trace] pre-approve"); if (APPROVE) { await approveAll(); console.error("[trace] approved"); await new Promise(r => setTimeout(r, 1500)); }
console.error("[trace] main loops starting");

// ---- 遥测主循环 ----
const tick = setInterval(() => {
  const now = Date.now();
  for (const d of devices) {
    if (now - (d.lastPub ?? 0) < INTERVAL * 1000 - 50) continue;
    if (INTERVAL * 1000 - (now - (d.lastPub ?? t0)) < 0 || d.lastPub === undefined) {
      d.lastPub = now; d.tPub = now;
      const p = JSON.stringify({ device_id: d.id, temperature: 18 + Math.random() * 10, humidity: 40 + Math.random() * 30, ts: now });
      d.client.publish(`device/${TYPE}/${d.id}/uplink`, p, { qos: 0 });
      pubs++;
    }
  }
}, Math.max(200, INTERVAL * 250));

// ---- 命令往返（REST → 平台 → 设备下行）----
let cmdTimer = null;
if (CMD_RATE > 0) cmdTimer = setInterval(async () => {
  const d = devices[Math.floor(Math.random() * devices.length)];
  d.tCmd = Date.now();
  const tc = Date.now();
  try {
    await api(`/api/devices/${d.id}/commands`, token, { method: "POST",
      body: JSON.stringify({ command: "reboot", params: { __bench_ts: d.tCmd } }) });
    log("cmd-rest", Date.now() - tc, true);
  } catch { log("cmd-rest", Date.now() - tc, false); }
}, 1000 / CMD_RATE);

// ---- 入库可见延迟探测：发布→100ms 轮询直至可查（真 KPI：MQTT→可查询）----
const probeVisible = async () => {
  const d = devices[Math.floor(Math.random() * devices.length)];
  if (!d.client?.connected) return;
  const tp = Date.now();
  d.client.publish(`device/${TYPE}/${d.id}/uplink`,
    JSON.stringify({ device_id: d.id, temperature: 25 + Math.random(), humidity: 50, ts: tp }), { qos: 0 });
  for (let i = 0; i < 50; i++) {
    await new Promise(r => setTimeout(r, 100));
    try {
      const r = await api(`/api/devices/${d.id}/telemetry?limit=1&hours=1`, token);
      const rows = r?.data?.data?.temperature ?? [];
      if (rows.some(x => x.timestamp * 1000 >= tp - 1500)) { log("visible", Date.now() - tp, true); return; }
    } catch {}
  }
  log("visible", Date.now() - tp, false);
};
const verify = setInterval(probeVisible, 3000);

// ---- 收尾 ----
setTimeout(async () => {
  clearInterval(tick); clearInterval(verify); cmdTimer && clearInterval(cmdTimer);
  if (APPROVE) await approveAll();
  await new Promise(r => setTimeout(r, 2000));
  for (const d of devices) d.client.end(true);
  OUT.end();
  const dur = (Date.now() - t0) / 1000;
  console.error(`[mqtt-devices] ${N}设备 ${dur.toFixed(0)}s: 发布 ${pubs} (${(pubs/dur).toFixed(1)}/s) 错误 ${errs} 命令RTT样本 ${cmdRTTs.length}`);
  process.exit(0);
}, DURATION * 1000);
