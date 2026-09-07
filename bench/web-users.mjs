// 前端用户模拟器 — 登录 / events WS / 轮询 / 遥测查询 的完整行为画像
// 用法: node web-users.mjs --n 5 --duration 120 [--out samples.jsonl]
import WebSocket from "ws";
import fs from "fs";

const args = Object.fromEntries(process.argv.slice(2).map((s, i, a) =>
  s.startsWith("--") ? [s.slice(2), a[i + 1]?.startsWith("--") ? true : (a[i + 1] ?? true)] : []).filter(([k]) => k));
const N = parseInt(args.n ?? "5");
const DURATION = parseFloat(args.duration ?? "120");
const API = args.api ?? "http://127.0.0.1:9375";
const OUT = fs.createWriteStream(args.out ?? "web-samples.jsonl");
const log = (op, ms, ok = true) => OUT.write(JSON.stringify({ op, ms: Math.round(ms * 10) / 10, ok, t: Date.now() }) + "\n");

const t0 = Date.now();
let pollCount = 0, wsMsgs = 0, wsErrs = 0;
const latSamples = { "login": [], "device-list": [], "telemetry-query": [], "events-latency": [] };
const P = (arr) => arr.length ? arr.sort((a, b) => a - b) : [];

for (let u = 0; u < N; u++) {
  (async () => {
    // 登录
    let tl = Date.now();
    const lr = await fetch(`${API}/api/auth/login`, { method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username: args.user ?? "admin", password: args.pass ?? "smoke-test-123" }) });
    const lj = await lr.json();
    const token = lj.token ?? lj.data?.token;
    latSamples.login.push(Date.now() - tl);
    if (!token) { wsErrs++; return; }
    const api = (p, o = {}) => fetch(`${API}${p}`, { ...o, headers: { "Content-Type": "application/json", Authorization: "Bearer " + token } }).then(r => r.json());

    // events WS（真实前端行为：Auth 握手 + 订阅）
    try {
      const ws = new WebSocket(`ws://127.0.0.1:9375/api/events/ws?token=${token}`);
      ws.on("open", () => ws.send(JSON.stringify({ type: "Auth", token })));
      ws.on("message", (d) => {
        wsMsgs++;
        try { const m = JSON.parse(d.toString());
          // 事件时延：事件内 timestamp vs 本地到达
          const evs = m.batch ? m.events : [m];
          for (const e of evs) if (e?.timestamp) {
            const ts = e.timestamp < 1e12 ? e.timestamp * 1000 : e.timestamp;  // 秒/毫秒自适应
            const d = Date.now() - ts;
            if (d >= 0 && d < 600000) latSamples["events-latency"].push(d);
          }
        } catch {}
      });
      ws.on("error", () => wsErrs++);
    } catch { wsErrs++; }

    // 行为循环：设备列表(2.5s) + 遥测查询(5s) —— 仪表盘典型轮询
    let stop = false;
    setTimeout(() => stop = true, DURATION * 1000);
    while (!stop) {
      let t = Date.now();
      await api("/api/devices?limit=50").catch(() => {});
      latSamples["device-list"].push(Date.now() - t); pollCount++;
      await new Promise(r => setTimeout(r, 2500));
      t = Date.now();
      await api("/api/telemetry?device_id=&hours=1&limit=100").catch(() => {});
      latSamples["telemetry-query"].push(Date.now() - t); pollCount++;
      await new Promise(r => setTimeout(r, 2500));
    }
  })();
}

setTimeout(() => {
  OUT.end();
  const dur = (Date.now() - t0) / 1000;
  const pct = (a, p) => { const s = P(a.slice()); return s.length ? s[Math.floor(s.length * p)] : null; };
  console.error(`[web-users] ${N}用户 ${dur.toFixed(0)}s: 轮询 ${pollCount} WS消息 ${wsMsgs} WS错误 ${wsErrs}`);
  for (const [k, v] of Object.entries(latSamples)) {
    if (!v.length) continue;
    console.error(`  ${k}: n=${v.length} p50=${pct(v,.5)}ms p95=${pct(v,.95)}ms`);
  }
  process.exit(0);
}, DURATION * 1000 + 2000);
