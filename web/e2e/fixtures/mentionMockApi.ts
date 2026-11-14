/**
 * API + auth stubs for mention a11y Playwright (no live backend / no Spark).
 * Pattern mirrors web/scripts/mobile-audit.mjs so we never touch eval :29431.
 */
import type { Page } from "@playwright/test"

const MENTION_ITEMS = [
  {
    type: "device",
    id: "sensor-001",
    label: "温湿度传感器",
    shortId: "sensor-0",
    hint: "temp_humidity · mqtt",
    duplicateName: true,
  },
  {
    type: "device",
    id: "sensor-002",
    label: "温湿度传感器",
    shortId: "sensor-0",
    hint: "temp_humidity · mqtt",
    duplicateName: true,
  },
  {
    type: "device",
    id: "green-01",
    label: "温室摄像头A",
    shortId: "green-01",
    hint: "camera · mqtt",
  },
  {
    type: "rule",
    id: "rule-high-temp",
    label: "高温报警",
    shortId: "rule-hig",
    hint: "enabled",
  },
  {
    type: "transform",
    id: "avg-temp",
    label: "温度均值",
    shortId: "avg-temp",
    hint: "enabled",
  },
  {
    type: "agent",
    id: "agent-watch",
    label: "巡检代理",
    shortId: "agent-wa",
    hint: "active",
  },
]

const HISTORY_WITH_NODES = [
  {
    id: "u-hist-1",
    role: "user",
    content: "把 @温室摄像头A 的规则复制给 @温湿度传感器",
    timestamp: Math.floor(Date.now() / 1000) - 120,
    contentNodes: [
      { kind: "text", text: "把 " },
      {
        kind: "reference",
        type: "device",
        id: "green-01",
        labelSnapshot: "温室摄像头A",
      },
      { kind: "text", text: " 的规则复制给 " },
      {
        kind: "reference",
        type: "device",
        id: "sensor-001",
        labelSnapshot: "温湿度传感器",
      },
    ],
    contentVersion: 1,
  },
  {
    id: "a-hist-1",
    role: "assistant",
    content: "已按引用处理。",
    timestamp: Math.floor(Date.now() / 1000) - 60,
  },
]

function mentionsBody(url: string) {
  const u = new URL(url)
  const q = (u.searchParams.get("q") || "").trim().toLowerCase()
  const items = !q
    ? MENTION_ITEMS
    : MENTION_ITEMS.filter(
        (o) =>
          o.label.toLowerCase().includes(q) ||
          o.id.toLowerCase().includes(q) ||
          o.type.includes(q),
      )
  return {
    items,
    total: items.length,
    limit: 20,
    offset: 0,
    hasMore: false,
    nextOffset: null,
    typeErrors: {},
  }
}

function bodyFor(url: string): unknown {
  const path = new URL(url).pathname.replace(/^\/api/, "")
  if (path === "/setup/status") return { setup_required: false }
  if (path.includes("/auth/me") || path.includes("/auth/current")) {
    return { id: "u1", username: "a11y-runner", role: "admin" }
  }
  if (path.startsWith("/mentions/search")) return mentionsBody(url)
  if (path.startsWith("/mentions/resolve")) {
    return {
      ok: true,
      resolutions: MENTION_ITEMS.slice(0, 2).map((o) => ({
        ok: true,
        type: o.type,
        id: o.id,
        label: o.label,
      })),
    }
  }
  if (path.startsWith("/llm-backends")) {
    return {
      backends: [
        {
          id: "mock-llm",
          name: "Mock",
          provider: "openai",
          model: "mock",
          is_active: true,
          capabilities: { multimodal: false },
        },
      ],
      count: 1,
      active_id: "mock-llm",
    }
  }
  if (path === "/sessions/pending") return { sessionIds: [], count: 0 }
  if (path === "/sessions" || path.startsWith("/sessions?")) {
    return [
      {
        sessionId: "mention-a11y-session",
        id: "mention-a11y-session",
        title: "Mention a11y",
        createdAt: Date.now() - 3600000,
        updatedAt: Date.now(),
        messageCount: HISTORY_WITH_NODES.length,
        preview: "把 @温室摄像头A 的规则复制给 @温湿度传感器",
      },
    ]
  }
  if (/\/sessions\/[^/]+\/history/.test(path)) {
    return {
      messages: HISTORY_WITH_NODES,
      count: HISTORY_WITH_NODES.length,
      total: HISTORY_WITH_NODES.length,
      has_more: false,
      hasMore: false,
    }
  }
  if (/\/sessions\/[^/]+$/.test(path) && !path.includes("history") && !path.includes("pending")) {
    return {
      sessionId: "mention-a11y-session",
      state: {
        id: "mention-a11y-session",
        created_at: Date.now() - 3600000,
        last_activity: Date.now(),
        message_count: HISTORY_WITH_NODES.length,
      },
    }
  }
  if (path === "/devices") return { devices: [], count: 0 }
  if (path.includes("/onboarding")) {
    return { dismissed: true, steps: { llm: { completed: true }, device: { completed: true } } }
  }
  if (path.includes("/extensions/pages")) return { pages: [] }
  if (path.includes("/skills")) return { skills: [], total: 0, page: 1, page_size: 10, total_pages: 0 }
  return {}
}

/** Install session token + /api mocks. Call before any navigation. */
export async function installMentionA11yMocks(page: Page): Promise<void> {
  await page.addInitScript(() => {
    sessionStorage.setItem("neomind_token_session", "mention-a11y-token")
    sessionStorage.setItem(
      "neomind_user_session",
      JSON.stringify({ id: "u1", username: "a11y-runner", role: "admin" }),
    )
    localStorage.setItem("neomind_has_llm_backends", "1")
    localStorage.setItem("i18nextLng", "zh")
    localStorage.setItem("theme", "light")
  })

  await page.route(/\/api\//, async (route) => {
    const pathname = new URL(route.request().url()).pathname
    if (!pathname.startsWith("/api/")) {
      await route.continue()
      return
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(bodyFor(route.request().url())),
    })
  })

  if (typeof page.routeWebSocket === "function") {
    await page.routeWebSocket(/.*/, () => {})
  }
}

export { MENTION_ITEMS, HISTORY_WITH_NODES }
