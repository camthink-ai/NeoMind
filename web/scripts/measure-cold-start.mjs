/**
 * Cold-start timing for the two entry pages users actually land on.
 *
 *   signed-in  /          from navigation start to the composer being visible
 *                        AND accepting a typed character
 *   login      /login     from navigation start to the login form accepting a
 *                        submit
 *
 * Build size says how many bytes move; this says how long until the page is
 * usable. Compare two builds by running the same script against each:
 *
 *   npx vite preview --outDir dist-a --port 4201 --strictPort &
 *   npx vite preview --outDir dist-b --port 4202 --strictPort &
 *   node scripts/measure-cold-start.mjs http://127.0.0.1:4201 http://127.0.0.1:4202
 *
 * Every run uses a fresh browser context (cold cache). Each page × build is
 * measured RUNS times and the median is reported. Numbers are only comparable
 * across runs on the same machine and browser.
 *
 * Optional:
 *   COLD_START_RUNS=5        samples per page per build (default 5)
 *   COLD_START_VIEWPORT=…    "1280x800" (default) or e.g. "390x844"
 *   COLD_START_JSON=path     also write the raw samples as JSON
 */
import { chromium } from "@playwright/test"
import fs from "node:fs"

const RUNS = Number(process.env.COLD_START_RUNS || 5)
const [w, h] = (process.env.COLD_START_VIEWPORT || "1280x800").split("x").map(Number)
const CHAT_PATH = "/"

// Signed-in session on the mock backend; enough for the chat page to mount
// its composer. No live server, no LLM call.
function chatBody(pathname, url) {
  if (pathname === "/api/setup/status") return { setup_required: false }
  if (pathname === "/api/auth/me" || pathname === "/api/auth/current") {
    return { id: "u1", username: "cold-start", role: "admin" }
  }
  if (pathname === "/api/sessions/pending") return { sessionIds: [], count: 0 }
  if (pathname.startsWith("/api/sessions")) return []
  if (pathname.startsWith("/api/llm-backends")) {
    return {
      backends: [{ id: "mock-llm", name: "Mock", provider: "openai", model: "mock", is_active: true }],
      count: 1,
      active_id: "mock-llm",
    }
  }
  if (pathname.includes("/onboarding")) {
    return { dismissed: true, steps: { llm: { completed: true }, device: { completed: true } } }
  }
  if (pathname === "/api/devices") return { devices: [], count: 0 }
  if (pathname === "/api/extensions/pages") return { pages: [] }
  void url
  return {}
}

async function installSignedInMocks(context) {
  await context.addInitScript(() => {
    sessionStorage.setItem("neomind_token_session", "cold-start-token")
    sessionStorage.setItem(
      "neomind_user_session",
      JSON.stringify({ id: "u1", username: "cold-start", role: "admin" }),
    )
    localStorage.setItem("neomind_has_llm_backends", "1")
    localStorage.setItem("i18nextLng", "zh")
    localStorage.setItem("theme", "light")
  })
  await context.route(/\/api\//, async (route) => {
    const { pathname, search } = new URL(route.request().url())
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(chatBody(pathname, search)),
    })
  })
  if (typeof context.routeWebSocket === "function") {
    await context.routeWebSocket(/.*/, () => {})
  }
}

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b)
  const m = Math.floor(s.length / 2)
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2
}

/**
 * Measures one load. `ready` polls in the page (every frame) until it returns
 * a number: the performance.now() at which the page became usable. Returning
 * false means "not yet" — it may also perform the interaction that proves
 * usability, such as typing into the composer.
 */
async function measure(browser, base, { path, signedIn, ready }) {
  const context = await browser.newContext({ viewport: { width: w, height: h } })
  try {
    if (signedIn) await installSignedInMocks(context)
    const page = await context.newPage()
    // Recorded, not fatal: e.g. the API prefetch hints in index.html have no
    // backend behind a `vite preview` and fail by design.
    const failed = []
    page.on("requestfailed", (r) => failed.push(r.url()))
    await page.goto(base + path, { waitUntil: "commit" })
    const usableAt = await page.waitForFunction(ready, null, { timeout: 60_000, polling: "raf" })
    const ms = await usableAt.jsonValue()
    // A predicate that is not synchronous resolves on its first frame with a
    // Promise (truthy), which serialises to `false`. Fail loudly instead of
    // reporting a blank "usable" column.
    if (typeof ms !== "number" || !Number.isFinite(ms)) {
      throw new TypeError(`readiness predicate for ${path} returned ${JSON.stringify(ms)}; it must return a number or false, and must not be async`)
    }
    const lcp = await page.evaluate(
      () =>
        new Promise((resolve) => {
          new PerformanceObserver((list) => resolve(list.getEntries().at(-1)?.startTime ?? 0)).observe({
            type: "largest-contentful-paint",
            buffered: true,
          })
          // LCP may stop being reported; fall back to whatever we have.
          setTimeout(() => resolve(0), 1500)
        }),
    )
    const nav = await page.evaluate(() => {
      const [e] = performance.getEntriesByType("navigation")
      // Every JS file, however it was requested. `modulepreload` links show up
      // with initiatorType "other", and on the login page they account for
      // most of what is downloaded — the entry preloads vendor chunks that
      // the page never executes.
      const scripts = performance
        .getEntriesByType("resource")
        .filter((r) => /\.js$/.test(r.name))
      const bytes = (xs) => xs.reduce((sum, r) => sum + (r.encodedBodySize || r.transferSize || 0), 0)
      return {
        dcl: e ? e.domContentLoadedEventStart : 0,
        // As delivered (gzip/br), i.e. what actually crossed the wire.
        jsBytes: bytes(scripts),
        jsBytesEntry: bytes(scripts.filter((r) => r.startTime < 60)),
        jsCount: scripts.length,
      }
    })
    return { usable: ms, lcp, dcl: nav.dcl, ...nav, failed }
  } finally {
    await context.close()
  }
}

// ---- readiness predicates (run in the page) --------------------------------
//
// NOT async: waitForFunction resolves as soon as the predicate returns a
// truthy value, and a Promise is truthy — an `async` predicate would hand
// back its pending promise on the first frame and the measurement would be
// `false`. Return a number (usable now) or false (not yet).

// Signed-in chat: the composer exists, is visible, and actually took a key.
function chatReady() {
  const editor = document.querySelector(".ProseMirror[contenteditable=true]")
  if (!editor) return false
  const r = editor.getBoundingClientRect()
  if (r.width === 0 || r.height === 0) return false
  editor.focus()
  const before = editor.textContent.length
  document.execCommand("insertText", false, "x")
  return editor.textContent.length > before ? performance.now() : false
}

// Login: the form is up, and its submit button enables once both fields are
// filled. (The button stays disabled until then, so "enabled after filling"
// is what "can submit" means for a signed-out visitor.)
function loginReady() {
  const user = document.querySelector("#username")
  const pass = document.querySelector("#password")
  const submit = document.querySelector('button[type="submit"]')
  if (!user || !pass || !submit) return false
  if (user.getBoundingClientRect().width === 0) return false
  // React-controlled inputs: use the native value setter so the change is picked up.
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set
  if (!submit.disabled) return performance.now()
  if (user.value === "") {
    set.call(user, "cold-start")
    user.dispatchEvent(new Event("input", { bubbles: true }))
  }
  if (pass.value === "") {
    set.call(pass, "cold-start-password")
    pass.dispatchEvent(new Event("input", { bubbles: true }))
  }
  return false
}

const PAGES = [
  { key: "signed-in chat", path: CHAT_PATH, signedIn: true, ready: chatReady, label: "composer visible + accepts input" },
  { key: "login", path: "/login", signedIn: false, ready: loginReady, label: "login form rendered" },
]

const bases = process.argv.slice(2).filter((a) => /^https?:\/\//.test(a))
if (bases.length === 0) {
  console.error("usage: node scripts/measure-cold-start.mjs <baseUrl> [baseUrl …]")
  process.exit(2)
}

const browser = await chromium.launch({ headless: true })
const rows = []
try {
  // One uncounted warm-up per build so the server (and any JIT) is warm.
  for (const base of bases) await measure(browser, base, PAGES[0])

  for (const page of PAGES) {
    for (const base of bases) {
      const samples = []
      for (let i = 0; i < RUNS; i++) samples.push(await measure(browser, base, page))
      rows.push({
        page: page.key,
        label: page.label,
        base,
        usable: median(samples.map((s) => s.usable)),
        lcp: median(samples.map((s) => s.lcp)),
        dcl: median(samples.map((s) => s.dcl)),
        min: Math.min(...samples.map((s) => s.usable)),
        max: Math.max(...samples.map((s) => s.usable)),
        jsBytes: Math.round(median(samples.map((s) => s.jsBytes))),
        jsBytesEntry: Math.round(median(samples.map((s) => s.jsBytesEntry))),
        jsCount: samples.at(-1).jsCount,
        failedRequests: [...new Set(samples.flatMap((s) => s.failed))],
        samples,
      })
    }
  }
} finally {
  await browser.close()
}

const fmt = (n) => (n ? Math.round(n).toString() : "–")
const name = (base) => new URL(base).port || base
console.log(`cold start — ${w}×${h}, headless Chromium, cold cache, median of ${RUNS} runs (ms)`)
for (const page of PAGES) {
  const rs = rows.filter((r) => r.page === page.key)
  console.log(`\n${page.key}  (${page.label})`)
  console.log("  build            usable   lcp    dcl   min–max      js gzip (entry / total scripts)")
  for (const r of rs) {
    console.log(
      `  ${name(r.base).padEnd(14)} ${fmt(r.usable).padStart(6)} ${fmt(r.lcp).padStart(6)} ${fmt(r.dcl).padStart(6)}   ${fmt(r.min)}–${fmt(r.max)}      ${(r.jsBytesEntry / 1024).toFixed(0)} / ${(r.jsBytes / 1024).toFixed(0)} KB (${r.jsCount})`,
    )
    for (const u of r.failedRequests) console.log(`  ${" ".repeat(14)} request failed: ${u}`)
  }
  if (rs.length >= 2) {
    const a = rs[0]
    const b = rs[1]
    const pct = (k) => (a[k] ? Math.round(((b[k] - a[k]) / a[k]) * 100) : 0)
    console.log(
      `  ${name(b.base)} vs ${name(a.base)}: usable ${pct("usable") >= 0 ? "+" : ""}${pct("usable")}%   lcp ${pct("lcp") >= 0 ? "+" : ""}${pct("lcp")}%   js ${pct("jsBytes") >= 0 ? "+" : ""}${pct("jsBytes")}%`,
    )
  }
}
if (process.env.COLD_START_JSON) {
  fs.writeFileSync(process.env.COLD_START_JSON, JSON.stringify({ viewport: `${w}x${h}`, runs: RUNS, rows }, null, 2))
}
