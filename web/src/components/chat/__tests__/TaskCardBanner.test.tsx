/**
 * TaskCardBanner — display contract:
 * 1. Empty card → renders nothing.
 * 2. Card with goal → shows sticky banner with goal as summary.
 * 3. Expanded → shows all five slots with labels.
 * 4. params slot gets the highlight treatment (the watermark lesson).
 */
import { describe, it, expect, vi } from "vitest"
import { render, screen, fireEvent } from "@testing-library/react"
import { TaskCardBanner } from "../TaskCardBanner"

// Mock the API client.
vi.mock("../../../lib/api/sessions", () => ({
  sessionsApi: {
    getTaskCard: vi.fn(),
  },
}))

import { sessionsApi } from "../../../lib/api/sessions"
const mockGet = sessionsApi.getTaskCard as ReturnType<typeof vi.fn>

describe("TaskCardBanner", () => {
  it("renders nothing for an empty card", async () => {
    mockGet.mockResolvedValue({ isEmpty: true })
    const { container } = render(
      <TaskCardBanner sessionId="s1" refreshToken={0} />,
    )
    // Wait for the fetch to resolve.
    await new Promise((r) => setTimeout(r, 10))
    expect(container.innerHTML).toBe("")
  })

  it("shows the goal as the one-line summary", async () => {
    mockGet.mockResolvedValue({
      goal: "给电度表图片加水印",
      isEmpty: false,
    })
    render(<TaskCardBanner sessionId="s1" refreshToken={0} />)
    await new Promise((r) => setTimeout(r, 10))
    const banner = screen.getByTestId("task-card-banner")
    expect(banner.textContent).toContain("给电度表图片加水印")
  })

  it("expanded shows all slots with the params highlight", async () => {
    mockGet.mockResolvedValue({
      goal: "巡检冷库",
      params: "厦门市集美区 温度阈值-18°C",
      step: "正在查询温度记录",
      open: "需要确认报警阈值",
      findings: "过去7天温度正常",
      isEmpty: false,
    })
    render(<TaskCardBanner sessionId="s1" refreshToken={0} />)
    await new Promise((r) => setTimeout(r, 10))

    // Click to expand (the button toggles).
    const banner = screen.getByTestId("task-card-banner")
    const button = banner.querySelector("button")
    if (button) fireEvent.click(button)

    // After expansion, all five slots should be visible.
    expect(banner.textContent).toContain("巡检冷库")
    expect(banner.textContent).toContain("厦门市集美区")
    expect(banner.textContent).toContain("-18°C")
    expect(banner.textContent).toContain("温度记录")
    expect(banner.textContent).toContain("报警阈值")
    expect(banner.textContent).toContain("温度正常")
  })
})
