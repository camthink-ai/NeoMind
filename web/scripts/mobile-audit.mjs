import { chromium } from "@playwright/test"
import fs from "node:fs/promises"
import { existsSync } from "node:fs"
import { fileURLToPath } from "node:url"

const outDir = new URL("../../work/mobile-audit/", import.meta.url)
const outDirPath = fileURLToPath(outDir)
await fs.mkdir(outDir, { recursive: true })

const devices = [
  { id: "camera-entry", device_id: "camera-entry", name: "入口摄像头", device_type: "camera", adapter_type: "mqtt", status: "offline", online: false, last_seen: new Date(Date.now() - 720000).toISOString() },
  { id: "cold-room-02", device_id: "cold-room-02", name: "冷库温度传感器", device_type: "temperature_sensor", adapter_type: "mqtt", status: "offline", online: false, last_seen: new Date(Date.now() - 420000).toISOString() },
  { id: "door-01", device_id: "door-01", name: "仓库门磁", device_type: "door_sensor", adapter_type: "webhook", status: "online", online: true, last_seen: new Date().toISOString() },
]
const detailDevice = {
  ...devices[1],
  adapter_id: "internal-mqtt",
  transport_connected: true,
  effective_offline_timeout_secs: 300,
  metric_count: 2,
  command_count: 1,
  connection_config: {
    telemetry_topic: "warehouse/cold-room-02/telemetry",
    command_topic: "warehouse/cold-room-02/command",
  },
  current_values: { temperature: 4.2, humidity: 68 },
}
const detailDeviceType = {
  device_type: "temperature_sensor",
  name: "温度传感器",
  description: "冷库温湿度监测",
  categories: ["environment"],
  metrics: [
    { name: "temperature", display_name: "温度", data_type: "float", unit: "°C" },
    { name: "humidity", display_name: "湿度", data_type: "float", unit: "%" },
  ],
  commands: [{ name: "calibrate", display_name: "校准传感器", parameters: [] }],
}
const messages = [
  { id: "m1", title: "入口摄像头离线", message: "设备已离线 12 分钟", category: "alert", severity: "warning", status: "active", timestamp: new Date().toISOString(), source: "camera-entry" },
  { id: "m2", title: "冷库温度传感器离线", message: "设备已离线 7 分钟", category: "alert", severity: "critical", status: "active", timestamp: new Date().toISOString(), source: "cold-room-02" },
]
const notificationChannels = [
  { name: "warehouse-email", channel_type: "email", enabled: true, recipients: ["ops@example.com", "owner@example.com"], config: {} },
  { name: "incident-webhook", channel_type: "webhook", enabled: false, config: { url: "https://example.com/hooks/incidents" } },
]
const agents = [
  { id: "a1", name: "仓库巡检", description: "检查离线设备与异常告警", user_prompt: "检查仓库设备", status: "Active", execution_mode: "free", memory_mode: "assistant", execution_count: 18, success_count: 17, last_execution_at: new Date().toISOString() },
  { id: "a2", name: "冷库温度监控", description: "监控冷库温度变化", user_prompt: "分析温度趋势", status: "Paused", execution_mode: "free", memory_mode: "assistant", execution_count: 8, success_count: 8, last_execution_at: new Date().toISOString() },
]
const detailAgent = {
  ...agents[0],
  execution_mode: "free",
  memory_mode: "assistant",
  priority: 5,
  llm_backend_id: "qwen3",
  created_at: new Date(Date.now() - 86400000 * 10).toISOString(),
  updated_at: new Date().toISOString(),
  avg_duration_ms: 2400,
  error_count: 1,
  output_schema: [],
  resources: [
    { resource_type: "device", resource_id: "camera-entry", name: "入口摄像头" },
    { resource_type: "device", resource_id: "cold-room-02", name: "冷库温度传感器" },
  ],
  schedule: { schedule_type: "manual" },
  stats: { total_executions: 18, successful_executions: 17, failed_executions: 1, avg_duration_ms: 2400 },
}
const rules = [
  {
    id: "r1", name: "冷库高温告警", enabled: true, trigger_count: 12,
    created_at: new Date().toISOString(), updated_at: new Date().toISOString(),
    last_triggered: new Date(Date.now() - 3600000).toISOString(),
    trigger: { trigger_type: "data_change", sources: ["device:cold-room-02:temperature"] },
    condition: { condition_type: "comparison", source: "device:cold-room-02:temperature", operator: ">", threshold: 30 },
    actions: [{ type: "notify", message: "冷库温度过高", severity: "warning" }],
    dsl_preview: "WHEN device:cold-room-02:temperature > 30\nDO\n    NOTIFY warning \"冷库温度过高\"\nEND",
  },
  {
    id: "r2", name: "入口设备离线提醒", enabled: false, trigger_count: 3,
    created_at: new Date(Date.now() - 86400000).toISOString(), updated_at: new Date().toISOString(),
    trigger: { trigger_type: "data_change", sources: ["device:camera-entry:online"] },
    condition: { condition_type: "comparison", source: "device:camera-entry:online", operator: "==", threshold_value: "false" },
    actions: [{ type: "notify", message: "入口设备已离线", severity: "critical" }],
    dsl_preview: "WHEN device:camera-entry:online == false\nDO\n    NOTIFY critical \"入口设备已离线\"\nEND",
  },
]
const transforms = [
  {
    id: "t1", name: "温度单位标准化", description: "摄氏温度标准化", enabled: true,
    type: "transform", scope: { device_type: "temperature_sensor" }, output_prefix: "normalized",
    complexity: 1, js_code: "return { temperature: input.temperature };",
    created_at: Date.now(), updated_at: Date.now(), execution_count: 42, last_executed: Date.now() - 60000,
  },
  {
    id: "t2", name: "设备状态摘要", description: "生成状态摘要", enabled: false,
    type: "transform", scope: "global", output_prefix: "summary", complexity: 1,
    js_code: "return input;", created_at: Date.now() - 86400000, updated_at: Date.now(), execution_count: 0, last_executed: null,
  },
]
const extensions = [
  {
    id: "mqtt-bridge", name: "MQTT Bridge", version: "1.4.0",
    description: "Connect MQTT devices and normalize telemetry", author: "NeoMind",
    state: "Active", health_status: "healthy", enabled: true, commands: [], metrics: [],
  },
  {
    id: "vision-analyzer", name: "Vision Analyzer", version: "2.1.0",
    description: "Analyze camera frames with local vision models", author: "NeoMind",
    state: "Warning", health_status: "degraded", enabled: true, commands: [], metrics: [],
  },
]
const imBridge = { id: "telegram", platform: "telegram", status: "running" }
const imSessions = [
  { chat_id: "warehouse-ops", bound_agent_id: "仓库巡检", neo_session_id: "s1", last_active: Date.now(), created_at: Date.now() - 86400000 },
  { chat_id: "cold-room-alerts", bound_agent_id: "冷库温度监控", neo_session_id: "s2", last_active: Date.now() - 3600000, created_at: Date.now() - 86400000 },
]
const chatSessions = [
  { sessionId: "audit-chat", id: "audit-chat", createdAt: Date.now() - 3600000, updatedAt: Date.now(), messageCount: 4, title: "冷库状态分析", preview: "建议先检查二号传感器" },
]
const chatMessages = [
  { id: "cm1", role: "user", content: "帮我看一下冷库现在的状态", timestamp: Date.now() - 120000 },
  { id: "cm2", role: "assistant", content: "当前冷库温度为 **4.2°C**，整体处于正常范围。二号传感器最近一次上报稍有延迟，建议检查网络连接。", timestamp: Date.now() - 90000 },
  { id: "cm3", role: "user", content: "需要马上处理吗？", timestamp: Date.now() - 60000 },
  { id: "cm4", role: "assistant", content: "暂时不需要停机。可以先观察下一次上报；如果 10 分钟内仍无数据，再安排现场检查。", timestamp: Date.now() - 30000 },
]
const backend = {
  id: "qwen3", name: "Qwen3 8B", model: "qwen3-8b", backend_type: "builtin",
  active: true, healthy: true, context_window: 32768,
  capabilities: { supports_tools: true, supports_multimodal: false, supports_thinking: true },
}
const fallbackBackend = {
  id: "qwen3-cloud", name: "Qwen3 Cloud", model: "qwen3-max", backend_type: "openai_compatible",
  active: false, healthy: true, context_window: 65536,
  capabilities: { supports_tools: true, supports_multimodal: true, supports_thinking: true },
}

function bodyFor(url) {
  const path = new URL(url).pathname.replace(/^\/api/, "")
  if (path === "/setup/status") return { setup_required: false }
  if (path.includes("/auth/me") || path.includes("/auth/current")) return { id: "u1", username: "shenmingming", role: "admin" }
  if (path.startsWith("/llm-backends")) return { backends: [backend, fallbackBackend], count: 2, active_id: backend.id }
  if (path === "/devices") return { devices, count: devices.length }
  if (path === "/devices/cold-room-02") return detailDevice
  if (path === "/devices/cold-room-02/current") return {
    device: detailDevice,
    metrics: {
      temperature: { name: "temperature", display_name: "温度", data_type: "float", unit: "°C", value: 4.2, is_virtual: false },
      humidity: { name: "humidity", display_name: "湿度", data_type: "float", unit: "%", value: 68, is_virtual: false },
    },
    commands: detailDeviceType.commands,
  }
  if (path === "/device-types") return { device_types: [{ device_type: "temperature_sensor", name: "温度传感器", description: "", categories: [] }], count: 1 }
  if (path === "/device-types/temperature_sensor") return detailDeviceType
  if (path.startsWith("/devices/drafts")) return { devices: [], drafts: [], count: 0 }
  if (path === "/sessions/pending") return { sessionIds: [], count: 0 }
  if (/\/sessions\/audit-chat\/history/.test(path)) return { messages: chatMessages, count: chatMessages.length, total: chatMessages.length, has_more: false }
  if (path === "/sessions/audit-chat") return { sessionId: "audit-chat", state: { id: "audit-chat", created_at: Date.now() - 3600000, last_activity: Date.now(), message_count: chatMessages.length } }
  if (path === "/sessions") return chatSessions
  if (path.startsWith("/sessions?")) return chatSessions
  if (path === "/skills" || path.startsWith("/skills?")) return { skills: [], total: 0, page: 1, page_size: 10, total_pages: 0 }
  if (path === "/agents/a1/executions") return { executions: [] }
  if (path === "/agents/a1/memory") return { memories: [], files: [], knowledge_files: [] }
  if (path === "/agents/a1/available-resources") return { devices: [], device_types: [], extensions: [] }
  if (path === "/agents/a1") return detailAgent
  if (path.startsWith("/agents/skills")) return { skills: [], total: 0 }
  if (path.startsWith("/agents/tools")) return { tools: [] }
  if (path === "/agents" || path.startsWith("/agents?")) return { agents, count: agents.length }
  if (path.includes("/channels")) return { channels: notificationChannels, count: notificationChannels.length }
  if (path.includes("/messages")) return { messages, count: messages.length, total: messages.length }
  if (path.includes("/onboarding")) return { dismissed: true, steps: { llm: { completed: true }, device: { completed: true } } }
  if (path.includes("/extensions/pages")) return { pages: [] }
  if (path === "/extensions" || path.startsWith("/extensions?")) return extensions
  if (path.includes("/transforms")) return { transforms, count: transforms.length }
  if (path.includes("/automation") || path.includes("/rules")) return { rules, count: rules.length }
  if (path.includes("/dashboard")) return { dashboards: [], count: 0 }
  if (/\/im-bridges\/[^/]+\/sessions/.test(path)) return { sessions: imSessions }
  if (path === "/im-bridges") return { bridges: [imBridge] }
  if (path.includes("/mqtt/status")) return { status: { connected: true, server_ip: "192.168.1.10" } }
  return {}
}

// CI containers pin /usr/bin/chromium; locally set MOBILE_AUDIT_CHROME to a
// Playwright-cached build (or leave unset for the bundled default).
const browser = await chromium.launch({
  ...(process.env.MOBILE_AUDIT_CHROME || existsSync("/usr/bin/chromium")
    ? { executablePath: process.env.MOBILE_AUDIT_CHROME || "/usr/bin/chromium" }
    : {}),
  headless: true,
})
const standardRoutes = [
  "/chat", "/chat/audit-chat",
  "/devices", "/devices/cold-room-02", "/devices/types", "/devices/drafts",
  "/agents", "/agents/memory", "/agents/skills", "/agents/tools",
  "/system",
  "/messages", "/messages/channels", "/messages/im",
  "/automation", "/automation/transforms",
  "/data", "/extensions", "/visual-dashboard",
]
const routes = process.env.AUDIT_ROUTE
  ? [process.env.AUDIT_ROUTE]
  : process.env.AUDIT_CHAT_ONLY === "1"
    ? ["/chat"]
    : standardRoutes
const allViewports = process.env.AUDIT_DESKTOP_ONLY === "1" ? [
  { name: "desktop", width: 1440, height: 960 },
] : [
  { name: "narrow", width: 320, height: 700 },
  { name: "standard", width: 390, height: 844 },
  { name: "large", width: 430, height: 932 },
  { name: "landscape", width: 844, height: 390 },
]
const viewports = process.env.AUDIT_STANDARD_ONLY === "1"
  ? allViewports.filter((viewport) => viewport.name === "standard")
  : allViewports
const results = []
const interactionResults = []
const navigationResults = []

async function waitForRouteReady(page, route) {
  await page.waitForFunction(() => document.querySelector('#root')?.childElementCount, null, { timeout: 5000 })
  if (route.startsWith("/chat")) {
    await page.waitForSelector("textarea", { state: "visible", timeout: 10000 })
  } else if ((page.viewportSize()?.width ?? 0) < 768) {
    await page.waitForSelector(".mobile-primary-nav", { state: "visible", timeout: 10000 })
  } else {
    await page.waitForFunction(
      () => (document.querySelector("#main-scroll-container")?.textContent?.trim().length ?? 0) > 0,
      null,
      { timeout: 10000 },
    )
  }
  if ((page.viewportSize()?.width ?? 0) < 768 && !route.startsWith("/chat")) {
    await page.waitForFunction(
      () => (document.querySelector(".safe-top h1")?.textContent?.trim().length ?? 0) > 0,
      null,
      { timeout: 10000 },
    )
  }
  // Allow the route's first data-driven render to settle after its lazy chunk
  // and persistent mobile chrome are mounted.
  await page.waitForTimeout(350)
}

for (const viewport of viewports) {
  const context = await browser.newContext({
    viewport: { width: viewport.width, height: viewport.height },
    locale: "zh-CN",
    hasTouch: true,
    isMobile: true,
  })
  await context.addInitScript(() => {
    sessionStorage.setItem("neomind_token_session", "audit-token")
    sessionStorage.setItem("neomind_user_session", JSON.stringify({ id: "u1", username: "shenmingming", role: "admin" }))
    localStorage.setItem("neomind_has_llm_backends", "1")
    localStorage.setItem("i18nextLng", "zh")
  })
  const page = await context.newPage()
  const pageErrors = []
  page.on("pageerror", (error) => pageErrors.push(error.message))
  await page.route(/\/api\//, async (route) => {
    const pathname = new URL(route.request().url()).pathname
    // Vite source modules such as /src/lib/api/client.ts also contain
    // "/api/". Only mock the backend namespace rooted at /api/.
    if (!pathname.startsWith("/api/")) {
      await route.continue()
      return
    }
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(bodyFor(route.request().url())) })
  })
  if (typeof page.routeWebSocket === "function") {
    await page.routeWebSocket(/.*/, () => {})
  }

  for (const route of routes) {
    pageErrors.length = 0
    await page.goto(`http://127.0.0.1:5173${route}`, { waitUntil: "domcontentloaded" })
    await waitForRouteReady(page, route)
    const audit = await page.evaluate((routePath) => {
      const pageScroller = document.querySelector("[data-page-scroll-container]")
      const primaryNav = document.querySelector(".mobile-primary-nav")
      const primaryNavStyle = primaryNav ? getComputedStyle(primaryNav) : null
      const primaryNavRect = primaryNav?.getBoundingClientRect()
      const visible = (el) => {
        const style = getComputedStyle(el)
        const rect = el.getBoundingClientRect()
        return style.visibility !== "hidden" && style.display !== "none" && rect.width > 0 && rect.height > 0
      }
      const tinyTargets = [...document.querySelectorAll("button, a, [role=button], input, textarea, [role=tab]")]
        .filter(visible)
        .filter((el) => !el.classList.contains("sr-only") && el.getAttribute("role") !== "switch")
        .map((el) => {
          const r = el.getBoundingClientRect()
          return { text: (el.getAttribute("aria-label") || el.textContent || "").trim().slice(0, 40), w: Math.round(r.width), h: Math.round(r.height) }
        })
        .filter((item) => item.w < 40 || item.h < 40)
      return {
        route: routePath,
        horizontalOverflow: document.documentElement.scrollWidth > window.innerWidth + 1,
        scrollWidth: document.documentElement.scrollWidth,
        viewportWidth: window.innerWidth,
        contentHorizontalOverflow: pageScroller
          ? pageScroller.scrollWidth > pageScroller.clientWidth + 1
          : false,
        contentScrollLeft: pageScroller?.scrollLeft ?? 0,
        primaryNavCount: document.querySelectorAll(".mobile-primary-nav").length,
        primaryNavVisible: primaryNav ? visible(primaryNav) : false,
        primaryNavState: primaryNavRect && primaryNavStyle ? {
          x: Math.round(primaryNavRect.x), y: Math.round(primaryNavRect.y),
          width: Math.round(primaryNavRect.width), height: Math.round(primaryNavRect.height),
          display: primaryNavStyle.display,
          visibility: primaryNavStyle.visibility,
          opacity: primaryNavStyle.opacity,
          transform: primaryNavStyle.transform,
          bodyClass: document.body.className,
        } : null,
        tinyTargets: tinyTargets.slice(0, 12),
      }
    }, route)
    results.push({ viewport: viewport.name, ...audit, pageErrors: [...pageErrors] })
    if (viewport.name === "standard" || viewport.name === "desktop") {
      const file = route.replaceAll("/", "_") || "_root"
      const suffix = viewport.name === "desktop" ? "_desktop" : ""
      await page.screenshot({ path: `${outDirPath}${file}${suffix}.png`, fullPage: false })
      if (viewport.name === "standard" && route.startsWith("/chat")) {
        const composer = page.locator(".chat-mobile-shell textarea")
        if (await composer.count()) {
          await composer.focus()
          await page.waitForTimeout(100)
          await page.screenshot({ path: `${outDirPath}${file}_focused.png`, fullPage: false })
          await composer.evaluate((el) => el.blur())
        }
      }
    }
  }

  if (viewport.name === "standard") {
    try {
      pageErrors.length = 0
      await page.goto("http://127.0.0.1:5173/agents", { waitUntil: "domcontentloaded" })
      await waitForRouteReady(page, "/agents")
      await page.getByText("仓库巡检", { exact: true }).first().click()
      const detail = page.getByRole("dialog", { name: "仓库巡检" })
      await detail.waitFor({ state: "visible" })
      const viewport = detail.locator('[data-radix-scroll-area-viewport]').last()
      const scrollState = await viewport.evaluate((el) => {
        const before = el.scrollTop
        el.scrollTop = el.scrollHeight
        return {
          before,
          after: el.scrollTop,
          clientHeight: el.clientHeight,
          scrollHeight: el.scrollHeight,
        }
      })
      interactionResults.push({
        name: "agent-detail-scroll",
        dialogCount: 1,
        bounds: await detail.evaluate((el) => {
          const r = el.getBoundingClientRect()
          return { x: Math.round(r.x), y: Math.round(r.y), width: Math.round(r.width), height: Math.round(r.height) }
        }),
        horizontalOverflow: await detail.evaluate((el) => el.scrollWidth > el.clientWidth + 1),
        scrollState,
        pageErrors: [...pageErrors],
      })
      await page.screenshot({ path: `${outDirPath}interaction-agent-detail.png`, fullPage: false })
      await page.keyboard.press("Escape")
    } catch (error) {
      interactionResults.push({ name: "agent-detail-scroll", error: String(error) })
    }

    try {
      // Chat special case: no bottom capsule on chat routes. Verify the
      // capsule is absent, then leave through the direct workspace button.
      await page.goto("http://127.0.0.1:5173/chat", { waitUntil: "domcontentloaded" })
      await waitForRouteReady(page, "/chat")
      const navCountOnChat = await page.locator(".mobile-primary-nav").count()
      await page.locator(".chat-mobile-shell textarea").focus()
      await page.evaluate(() => (document.activeElement instanceof HTMLElement) && document.activeElement.blur())
      await page.getByTestId("chat-workspace-button").click()
      await page.waitForURL(/\/system$/)
      await waitForRouteReady(page, "/system")
      const workspacePath = new URL(page.url()).pathname
      const primaryNavItems = await page.locator(".mobile-primary-nav button").count()
      await page.getByRole("button", { name: /可视化仪表板/ }).click()
      await page.waitForURL(/\/visual-dashboard$/)
      const dashboardPath = new URL(page.url()).pathname
      await page.getByTestId("mobile-primary-system").click()
      await page.waitForURL(/\/system$/)
      const systemPath = new URL(page.url()).pathname
      await page.getByRole("button", { name: /^设备/ }).click()
      await page.waitForURL(/\/devices$/)
      const devicesPath = new URL(page.url()).pathname
      // Devices has the capsule: its chat entry returns to Chat.
      await page.getByTestId("mobile-primary-chat").click()
      await page.waitForURL(/\/chat$/)
      const chatPath = new URL(page.url()).pathname
      await page.goto("http://127.0.0.1:5173/automation", { waitUntil: "domcontentloaded" })
      await waitForRouteReady(page, "/automation")
      await page.getByRole("button", { name: /返回|Back/i }).first().click()
      await page.waitForURL(/\/system$/)
      navigationResults.push({ name: "primary-navigation", workspacePath, primaryNavItems, dashboardPath, systemPath, devicesPath, chatPath, backPath: new URL(page.url()).pathname, navCountOnChat })
    } catch (error) {
      navigationResults.push({ name: "primary-navigation", error: String(error) })
    }

    const interactions = [
      { name: "device-create", route: "/devices", button: "添加设备" },
      { name: "agent-create", route: "/agents", button: "创建 AI 智能体" },
      { name: "message-create", route: "/messages", button: "创建消息" },
      { name: "automation-create", route: "/automation", button: "创建" },
      { name: "settings", route: "/system", button: "设置" },
      { name: "extension-upload", route: "/extensions", button: "上传扩展" },
    ]
    for (const item of interactions) {
      try {
        pageErrors.length = 0
        await page.goto(`http://127.0.0.1:5173${item.route}`, { waitUntil: "domcontentloaded" })
        await waitForRouteReady(page, item.route)
        await page.getByRole("button", { name: item.button, exact: item.name !== "settings" }).first().click()
        await page.waitForTimeout(350)
        const state = await page.evaluate(() => {
          const dialogs = [...document.querySelectorAll('[role="dialog"]')].filter((el) => {
            const r = el.getBoundingClientRect()
            return r.width > 0 && r.height > 0
          })
          const dialog = dialogs.at(-1)
          const r = dialog?.getBoundingClientRect()
          const fields = dialog ? [...dialog.querySelectorAll('input, textarea, select')] : []
          return {
            dialogCount: dialogs.length,
            bounds: r ? { x: Math.round(r.x), y: Math.round(r.y), width: Math.round(r.width), height: Math.round(r.height) } : null,
            horizontalOverflow: dialog ? dialog.scrollWidth > dialog.clientWidth + 1 : false,
            fields: fields.map((el) => ({
              fontSize: getComputedStyle(el).fontSize,
              type: el.getAttribute("type") || el.tagName.toLowerCase(),
              name: el.getAttribute("name") || el.getAttribute("aria-label") || el.getAttribute("placeholder") || "",
            })),
          }
        })
        interactionResults.push({ name: item.name, ...state, pageErrors: [...pageErrors] })
        await page.screenshot({ path: `${outDirPath}interaction-${item.name}.png`, fullPage: false })
        await page.keyboard.press("Escape")
      } catch (error) {
        interactionResults.push({ name: item.name, error: String(error) })
      }
    }

    try {
      pageErrors.length = 0
      await page.goto("http://127.0.0.1:5173/agents", { waitUntil: "domcontentloaded" })
      await waitForRouteReady(page, "/agents")
      await page.getByRole("switch").first().click()
      const sheet = page.getByRole("alertdialog")
      await sheet.waitFor({ state: "visible" })
      const bounds = await sheet.evaluate((el) => {
        const r = el.getBoundingClientRect()
        return {
          x: Math.round(r.x), y: Math.round(r.y),
          width: Math.round(r.width), height: Math.round(r.height),
        }
      })
      interactionResults.push({
        name: "confirm-sheet",
        dialogCount: 1,
        bounds,
        horizontalOverflow: await sheet.evaluate((el) => el.scrollWidth > el.clientWidth + 1),
        fields: [],
        pageErrors: [...pageErrors],
      })
      await page.screenshot({ path: `${outDirPath}interaction-confirm-sheet.png`, fullPage: false })
      await page.keyboard.press("Escape")
    } catch (error) {
      interactionResults.push({ name: "confirm-sheet", error: String(error) })
    }
  }
  await context.close()
}

await browser.close()
await fs.writeFile(new URL("report.json", outDir), JSON.stringify(results, null, 2))
await fs.writeFile(new URL("interaction-report.json", outDir), JSON.stringify(interactionResults, null, 2))
await fs.writeFile(new URL("navigation-report.json", outDir), JSON.stringify(navigationResults, null, 2))
// Chat routes render no bottom capsule (header menu drawer instead); every
// other mobile page renders exactly one.
const expectedNavCount = (r) =>
  r.route === "/chat" || r.route.startsWith("/chat/") ? 0 : 1

const failures = results.filter((r) =>
  r.horizontalOverflow || r.contentHorizontalOverflow || r.contentScrollLeft !== 0 || r.pageErrors.length ||
  (r.viewport === "landscape" || r.viewportWidth >= 768
    ? r.primaryNavCount !== 0 || r.primaryNavVisible
    : r.route.startsWith("/chat")
      ? r.primaryNavCount !== 0 || r.primaryNavVisible
      : r.primaryNavCount !== 1 || !r.primaryNavVisible)
)
const interactionFailures = interactionResults.filter((r) =>
  r.error || r.dialogCount < 1 || r.horizontalOverflow || r.pageErrors?.length ||
  r.fields?.some((field) => Number.parseFloat(field.fontSize) < 16) ||
  (r.name === "agent-detail-scroll" && (!r.scrollState || r.scrollState.scrollHeight <= r.scrollState.clientHeight || r.scrollState.after <= r.scrollState.before))
)
const navigationFailures = navigationResults.filter((r) =>
  r.error || r.workspacePath !== "/system" || r.primaryNavItems !== 2 || r.dashboardPath !== "/visual-dashboard" || r.systemPath !== "/system" ||
  r.devicesPath !== "/devices" || r.chatPath !== "/chat" ||
  r.backPath !== "/system" || r.navCountOnChat !== 0
)
console.log(JSON.stringify({ checked: results.length, failures, interactionResults, interactionFailures, navigationResults, navigationFailures, reports: `${outDirPath}report.json` }, null, 2))
if (failures.length || interactionFailures.length || navigationFailures.length) process.exitCode = 1
