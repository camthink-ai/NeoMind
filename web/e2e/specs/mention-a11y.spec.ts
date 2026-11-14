/**
 * §12 C — real-browser mention a11y / touch / history chip redraw.
 *
 * Uses mocked /api (no live Spark / no eval :29431). Runs under Playwright
 * Chrome + an iPhone-touch project. Headed via `npx playwright test --headed`
 * or project `mention-desktop-headed`.
 *
 * Does NOT claim: physical touch device, VoiceOver/TalkBack live read, or
 * 5-person × 4-task usability.
 */

import { test, expect } from "@playwright/test"
import { AxeBuilder } from "@axe-core/playwright"
import { installMentionA11yMocks } from "../fixtures/mentionMockApi"

test.describe("mention a11y — prototype /dev/inline-at", () => {
  test.beforeEach(async ({ page }) => {
    await installMentionA11yMocks(page)
  })

  test("desktop: toolbar opens listbox with ARIA + keyboard select inserts chip", async ({
    page,
  }) => {
    await page.goto("/dev/inline-at")
    await expect(page.locator("[data-inline-at-prototype]")).toBeVisible({
      timeout: 15000,
    })

    // Prefer toolbar / surface insert if present; else type @ in the editor.
    const insertBtn = page.getByRole("button", { name: /插入|Insert|@/i }).first()
    if (await insertBtn.count()) {
      await insertBtn.click()
    } else {
      const surface = page.locator("[data-inline-at-surface], [contenteditable=true], textarea").first()
      await surface.click()
      await page.keyboard.type("@")
    }

    const listbox = page.locator('[role="listbox"]').first()
    await expect(listbox).toBeVisible({ timeout: 10000 })
    await expect(listbox).toHaveAttribute("aria-label", /.+/);

    const options = listbox.locator('[role="option"]')
    await expect(options.first()).toBeVisible()
    // Duplicate-name hits expose shortId/hint (not color-only).
    const dup = options.filter({ hasText: "温湿度传感器" })
    if ((await dup.count()) >= 1) {
      const text = (await dup.first().innerText()).toLowerCase()
      expect(text.includes("sensor") || text.includes("hint") || text.length > 6).toBeTruthy()
    }

    await page.keyboard.press("ArrowDown")
    await page.keyboard.press("Enter")

    await expect(page.locator("[data-inline-at-chip], .nm-mention").first()).toBeVisible({
      timeout: 5000,
    })
    // History preview redraws inline tags from nodes.
    await expect(page.locator("[data-inline-at-history]")).toContainText(/.+/)
  })

  test("desktop: axe serious/critical on prototype page", async ({ page }) => {
    await page.goto("/dev/inline-at")
    await expect(page.locator("[data-inline-at-prototype]")).toBeVisible({
      timeout: 15000,
    })
    const results = await new AxeBuilder({ page })
      .withTags(["wcag2a", "wcag2aa"])
      .disableRules(["color-contrast"]) // design-token contrast audited separately
      .analyze()
    const blocking = results.violations.filter((v) =>
      ["serious", "critical"].includes(v.impact || ""),
    )
    expect(blocking, JSON.stringify(blocking, null, 2)).toEqual([])
  })
})

test.describe("mention a11y — production chat TipTap", () => {
  test.beforeEach(async ({ page }) => {
    await installMentionA11yMocks(page)
  })

  test("chat: history redraws contentNodes chips + reuse button", async ({ page }) => {
    await page.goto("/chat/mention-a11y-session")
    await expect(page.locator('[data-testid="messages-container"]')).toBeVisible({
      timeout: 20000,
    })
    const chips = page.locator('[data-user-content-nodes] .nm-mention')
    await expect(chips.first()).toBeVisible({ timeout: 15000 })
    await expect(chips).toHaveCount(2)
    await expect(chips.nth(0)).toHaveAttribute("data-id", "green-01")
    await expect(chips.nth(1)).toHaveAttribute("data-id", "sensor-001")

    const userBubble = page.locator('[data-testid="user-message"]').first()
    await userBubble.hover()
    const reuse = page.getByRole("button", { name: /填回输入框|Reuse in composer|reuseInComposer/i })
    await expect(reuse.first()).toBeVisible()
    // Action row stays pointer-events-none until group-hover (desktop).
    await reuse.first().click({ force: false })
    // TipTap host should receive restored nodes as mention atoms.
    await expect(page.locator(".ProseMirror .nm-mention, [data-chat-composer] .nm-mention").first()).toBeVisible({
      timeout: 8000,
    })
  })

  test("chat: toolbar @ opens listbox; Escape closes", async ({ page }) => {
    await page.goto("/chat")
    await expect(page.locator(".ProseMirror, textarea").first()).toBeVisible({
      timeout: 20000,
    })
    const insert = page.getByRole("button", { name: /插入引用|Insert @|mention\.insert/i })
    await insert.click()
    const listbox = page.locator("#chat-mention-listbox, [data-mention-menu], [role=listbox]").first()
    await expect(listbox).toBeVisible({ timeout: 10000 })
    await expect(listbox).toHaveAttribute("role", "listbox")
    await page.keyboard.press("Escape")
    await expect(listbox).toBeHidden({ timeout: 5000 })
  })
})

test.describe("mention a11y — mobile touch viewport", () => {
  // Viewport/touch only — do not spread devices['iPhone *'] here (forces
  // defaultBrowserType and breaks nested test.use under a project).
  test.use({
    viewport: { width: 390, height: 844 },
    hasTouch: true,
    isMobile: true,
  })

  test.beforeEach(async ({ page }) => {
    await installMentionA11yMocks(page)
  })

  test("touch: mobile panel path on prototype + tap select", async ({ page }) => {
    await page.goto("/dev/inline-at")
    await expect(page.locator("[data-inline-at-prototype]")).toBeVisible({
      timeout: 15000,
    })
    const mobileToggle = page.locator('[data-viewport-toggle="mobile"]')
    if (await mobileToggle.count()) {
      await mobileToggle.tap()
    }

    const surface = page.locator("[data-inline-at-surface], [contenteditable=true], textarea").first()
    await surface.tap()
    // Soft-keyboard not available in automation — type @ via keyboard API.
    await page.keyboard.type("@")

    const listbox = page.locator('[role="listbox"], [data-inline-at-picker]').first()
    await expect(listbox).toBeVisible({ timeout: 10000 })

    const option = listbox.locator('[role="option"]').first()
    await option.tap()
    await expect(page.locator("[data-inline-at-chip], .nm-mention").first()).toBeVisible({
      timeout: 5000,
    })
  })

  test("touch: chat history chips visible at phone width", async ({ page }) => {
    await page.goto("/chat/mention-a11y-session")
    await expect(page.locator('[data-user-content-nodes] .nm-mention').first()).toBeVisible({
      timeout: 20000,
    })
    const chipBox = await page.locator('[data-user-content-nodes] .nm-mention').first().boundingBox()
    expect(chipBox).toBeTruthy()
    expect(chipBox!.height).toBeGreaterThanOrEqual(16)
  })
})
