// 业务层并发基准：N 个并发在途请求 × 多端点 + K 个并发 WS 订阅者
import WebSocket from "ws";
const API = "http://127.0.0.1:9375";
const login = async () => (await (await fetch(`${API}/api/auth/login`, {method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({username:"admin",password:"smoke-test-123"})})).json()).token;
const token = await login();
const H = { "Content-Type": "application/json", Authorization: "Bearer " + token };

// ---- 并发 REST：三端点 × 递增并发 ----
const endpoints = [
  ["遥测查询", () => fetch(`${API}/api/devices/bench_sensor_0000/telemetry?limit=5&hours=1`, { headers: H, signal: AbortSignal.timeout(15000) })],
  ["设备列表", () => fetch(`${API}/api/devices?limit=50`, { headers: H, signal: AbortSignal.timeout(15000) })],
  ["转换执行", () => fetch(`${API}/api/automations/transforms/test-code`, { method: "POST", headers: H,
    body: JSON.stringify({ code: "function t(d){const o={};for(const[k,v]of Object.entries(d)){if(typeof v==='number')o[k]=v*2}return o}t(input)",
      input_data: { a: 1, b: 2.5, c: "x" }, output_prefix: "c" }), signal: AbortSignal.timeout(15000) })],
];
const pct = (a, p) => a[Math.min(a.length - 1, Math.floor(a.length * p))];
for (const [name, fn] of endpoints) {
  const line = [];
  for (const CONC of [10, 50, 100, 200]) {
    const lat = []; let errs = 0;
    const worker = async () => {
      for (let i = 0; i < Math.ceil(400 / CONC); i++) {
        const t = performance.now();
        try { const r = await fn(); if (!r.ok) errs++; } catch { errs++; }
        lat.push(performance.now() - t);
      }
    };
    const t0 = performance.now();
    await Promise.all(Array.from({ length: CONC }, worker));
    const dur = (performance.now() - t0) / 1000;
    lat.sort((a, b) => a - b);
    line.push(`C=${CONC}: p50=${pct(lat,.5).toFixed(0)}ms p95=${pct(lat,.95).toFixed(0)}ms ${Math.round(lat.length/dur)}req/s${errs ? ` ✗${errs}` : ""}`);
  }
  console.log(`${name}: ${line.join(" | ")}`);
}

// ---- 并发 WS：K 个 events 订阅者 ----
const wsLat = []; let wsMsgs = 0;
const CONC_WS = 20;
const wss = [];
for (let i = 0; i < CONC_WS; i++) {
  const ws = new WebSocket(`ws://127.0.0.1:9375/api/events/ws?token=${token}`);
  ws.on("open", () => ws.send(JSON.stringify({ type: "Auth", token })));
  ws.on("message", (d) => { try { const m = JSON.parse(d.toString()); if (m.batch || m.type === "ping") wsMsgs++; } catch {} });
  wss.push(ws);
}
await new Promise(r => setTimeout(r, 3000));
// 并发期间 REST 压力继续（100 并发查询）
{
  const lat = [];
  await Promise.all(Array.from({ length: 100 }, async () => {
    for (let i = 0; i < 4; i++) { const t = performance.now();
      await fetch(`${API}/api/devices/bench_sensor_0000/telemetry?limit=5&hours=1`, { headers: H, signal: AbortSignal.timeout(15000) }).catch(() => {});
      lat.push(performance.now() - t); }
  }));
  lat.sort((a, b) => a - b);
  console.log(`WS×20 并发期间 REST: p50=${pct(lat,.5).toFixed(0)}ms p95=${pct(lat,.95).toFixed(0)}ms | WS 消息总量 ${wsMsgs}`);
}
wss.forEach(w => w.close());
process.exit(0);
