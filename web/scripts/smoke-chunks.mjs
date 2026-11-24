/**
 * Loads and evaluates EVERY JavaScript chunk of a production build, in a real
 * browser, against the built index.html. This is what a chunk-splitting change
 * can break: a chunk that fails to fetch, or a module used before the chunk
 * that defines it has evaluated ("Cannot access 'x' before initialization",
 * the symptom of a circular chunk dependency). Both show up only when the
 * chunk actually runs, which UI smoke tests reach only for the pages they
 * click through.
 *
 *   node scripts/smoke-chunks.mjs http://127.0.0.1:4202 [--all-chunks]
 *
 * Two passes:
 *   routes   open each app route with a mocked backend (like mobile-audit.mjs)
 *            and fail on page exceptions and chunk load errors
 *   chunks   --all-chunks: import every /assets/*.js of the build in a page
 *            that booted the app, one by one, and fail on any rejection
 */
import { chromium } from "@playwright/test"
import fs from "node:fs"
import path from "node:path"

// ---- route pass -----------------------------------------------------------------

const ROUTES = [
  { path: "/login", signedIn: false, ready: "#username" },
  { path: "/", signedIn: true, ready: ".ProseMirror[contenteditable=true]" },
  { path: "/visual-dashboard", signedIn: true, ready: "#main-scroll-container" },
  { path: "/data", signedIn: true, ready: "#main-scroll-container" },
  { path: "/devices", signedIn: true, ready: "#main-scroll-container" },
  { path: "/automation", signedIn: true, ready: "#main-scroll-container" },
  { path: "/agents", signedIn: true, ready: "#main-scroll-container" },
  { path: "/messages", signedIn: true, ready: "#main-scroll-container" },
  { path: "/messages/im", signedIn: true, ready: "#main-scroll-container" },
  { path: "/extensions", signedIn: true, ready: "#main-scroll-container" },
  { path: "/system", signedIn: true, ready: "#main-scroll-container" },
]

function bodyFor(pathname) {
  if (pathname === "/api/setup/status") return { setup_required: false }
  if (pathname === "/api/auth/me") return { id: "u1", username: "smoke", role: "admin" }
  if (pathname === "/api/sessions/pending") return { sessionIds: [], count: 0 }
  if (pathname.startsWith("/api/sessions")) return []
  if (pathname.startsWith("/api/llm-backends")) {
    return {
      backends: [{ id: "mock-llm", name: "Mock", provider: "openai", model: "mock", is_active: true, capabilities: { multimodal: false } }],
      count: 1,
      active_id: "mock-llm",
    }
  }
  if (pathname.includes("/onboarding")) {
    return { dismissed: true, steps: { llm: { completed: true }, device: { completed: true } } }
  }
  if (pathname === "/api/devices") return { devices: [], count: 0 }
  if (pathname === "/api/extensions" || pathname.startsWith("/api/extensions/") && !pathname.includes("/pages")) return []
  if (pathname === "/api/extensions/pages") return { pages: [] }
  if (pathname.startsWith("/api/skills")) return { skills: [], total: 0, page: 1, page_size: 10, total_pages: 0 }
  if (pathname.startsWith("/api/automations") || pathname.startsWith("/api/rules")) return []
  return {}
}

// A bad chunk split produces one of these. Anything else is app/mock noise.
const FATAL = [
  /dynamically imported module/i,
  /Importing a module script failed/i,
  /error loading dynamically imported module/i,
  /before initialization/i,
  /ChunkLoadError/i,
  /Unexpected token '<'/,
  /MIME type .* is not executable/i,
]

async function signedInContext(browser) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } })
  await context.addInitScript(() => {
    sessionStorage.setItem("neomind_token_session", "smoke-token")
    sessionStorage.setItem("neomind_user_session", JSON.stringify({ id: "u1", username: "smoke", role: "admin" }))
    localStorage.setItem("neomind_has_llm_backends", "1")
    localStorage.setItem("i18nextLng", "zh")
    localStorage.setItem("theme", "light")
  })
  await context.route(/\/api\//, async (r) => {
    const { pathname } = new URL(r.request().url())
    if (!pathname.startsWith("/api/")) return r.continue()
    return r.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(bodyFor(pathname)) })
  })
  if (typeof context.routeWebSocket === "function") await context.routeWebSocket(/.*/, () => {})
  return context
}

async function routePass(browser, base) {
  let failed = 0
  for (const route of ROUTES) {
    const context = route.signedIn ? await signedInContext(browser) : await browser.newContext()
    if (!route.signedIn) {
      await context.route(/\/api\//, async (r) => {
        const { pathname } = new URL(r.request().url())
        if (!pathname.startsWith("/api/")) return r.continue()
        return r.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(bodyFor(pathname)) })
      })
      if (typeof context.routeWebSocket === "function") await context.routeWebSocket(/.*/, () => {})
    }
    const page = await context.newPage()
    const errors = []
    let scripts = 0
    page.on("pageerror", (e) => errors.push(`pageerror: ${e.message.slice(0, 200)}`))
    page.on("console", (m) => {
      if (m.type() === "error") errors.push(`console: ${m.text().slice(0, 200)}`)
    })
    page.on("requestfailed", (r) => {
      if (/\/assets\/.*\.js$/.test(r.url())) errors.push(`chunk request failed: ${new URL(r.url()).pathname}`)
    })
    page.on("response", (res) => {
      if (/\/assets\/.*\.js$/.test(res.url())) {
        scripts++
        if (res.status() >= 400) errors.push(`chunk HTTP ${res.status()}: ${new URL(res.url()).pathname}`)
      }
    })
    let rendered = true
    try {
      await page.goto(base + route.path, { waitUntil: "load" })
      await page.waitForSelector(route.ready, { timeout: 20_000 })
    } catch (e) {
      rendered = false
      errors.push(`never rendered ${route.ready}`)
    }
    await page.waitForTimeout(500)
    const fatal = [...new Set(errors.filter((e) => FATAL.some((re) => re.test(e))))]
    const ok = rendered && fatal.length === 0
    if (!ok) failed++
    console.log(`  ${ok ? "ok  " : "FAIL"} ${route.path.padEnd(17)} ${String(scripts).padStart(2)} scripts${fatal.length ? "\n         " + fatal.join("\n         ") : ""}`)
    await context.close()
  }
  return failed
}

// ---- all-chunks pass --------------------------------------------------------------

async function chunkPass(browser, base) {
  // List the build's JS from the server itself, via the Vite manifest …
  const dist = process.env.SMOKE_DIST_DIR
  let files = []
  if (dist && fs.existsSync(path.join(dist, "assets"))) {
    files = fs.readdirSync(path.join(dist, "assets")).filter((f) => f.endsWith(".js"))
  }
  if (files.length === 0) {
    console.log("  skipped: set SMOKE_DIST_DIR=<dist> to list the build's chunks")
    return 0
  }
  const context = await signedInContext(browser)
  const page = await context.newPage()
  const errors = []
  page.on("pageerror", (e) => errors.push(e.message.slice(0, 200)))
  await page.goto(base + "/", { waitUntil: "load" })
  await page.waitForTimeout(1500)

  // Import each chunk for real. A module that is already loaded resolves from
  // the module map; the rest are fetched and evaluated now.
  const results = await page.evaluate(async (names) => {
    const out = []
    for (const name of names) {
      try {
        await import(`/assets/${name}`)
        out.push({ name, ok: true })
      } catch (e) {
        out.push({ name, ok: false, error: String(e && e.message ? e.message : e).slice(0, 240) })
      }
    }
    return out
  }, files)

  const bad = results.filter((r) => !r.ok)
  const pageErrors = errors.filter((e) => FATAL.some((re) => re.test(e)))
  console.log(`  imported ${results.length} chunks: ${results.length - bad.length} ok, ${bad.length} failed`)
  for (const r of bad) console.log(`    FAIL ${r.name}: ${r.error}`)
  for (const e of pageErrors) console.log(`    page error: ${e}`)
  await context.close()
  return bad.length + pageErrors.length
}

// ---- main --------------------------------------------------------------------------

const args = process.argv.slice(2)
const bases = args.filter((a) => /^https?:\/\//.test(a))
const allChunks = args.includes("--all-chunks")
if (bases.length === 0) {
  console.error("usage: node scripts/smoke-chunks.mjs <baseUrl> [baseUrl …] [--all-chunks]")
  process.exit(2)
}

const browser = await chromium.launch({ headless: true })
let failed = 0
try {
  for (const base of bases) {
    console.log(`\n${base}`)
    console.log(" routes:")
    failed += await routePass(browser, base)
    if (allChunks) {
      console.log(" every chunk:")
      failed += await chunkPass(browser, base)
    }
  }
} finally {
  await browser.close()
}
console.log(failed ? `\n${failed} failure(s)` : "\nall ok")
process.exit(failed ? 1 : 0)
