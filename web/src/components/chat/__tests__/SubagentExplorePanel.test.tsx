/**
 * SubagentExplorePanel — three-state contract (§2.0d / plan F-fixes):
 * 1. LIVE: renders round/max, current tool, elapsed, call count.
 * 2. COMPLETED: renders from the persisted tool result (survives refresh).
 * 3. DEGRADED: no detail and no result → renders nothing (old backends).
 *
 * i18n: the global setup mocks t() to return the key itself — assertions
 * match on keys and raw data, not translated strings.
 */
import { describe, it, expect } from "vitest"
import { render, screen } from "@testing-library/react"
import { SubagentExplorePanel } from "../SubagentExplorePanel"
import type { ToolCall } from "../../../types"

const baseCall: ToolCall = {
  id: "call-1",
  name: "explore",
  arguments: { question: "冷库上周异常？" },
}

describe("SubagentExplorePanel", () => {
  it("LIVE: shows round progress, current tool, call count and duration", () => {
    render(
      <SubagentExplorePanel
        toolCall={baseCall}
        isStreaming
        liveDetail={{
          round: 3,
          maxRounds: 8,
          currentTool: "device list",
          note: "查冷库设备",
          toolCallsSoFar: 11,
          wallClockMs: 84_200,
        }}
      />,
    )
    const panel = screen.getByTestId("explore-panel-live")
    expect(panel.textContent).toContain("explore.inProgress")
    expect(panel.textContent).toContain("device list")
    expect(panel.textContent).toContain("11")
    expect(panel.textContent).toContain("1m24s")
  })

  it("COMPLETED: renders from the persisted result with per-round notes", () => {
    const done: ToolCall = {
      ...baseCall,
      result: {
        summary: "冷库温度正常",
        rounds_used: 8,
        tool_calls_made: 23,
        wall_clock_ms: 112_000,
        budget_exhausted: false,
        rounds: [
          { tool: "metric_get", note: "温度 21.5°C" },
          { tool: "data query", note: "近7天记录" },
        ],
      },
    }
    render(<SubagentExplorePanel toolCall={done} liveDetail={null} />)
    const panel = screen.getByTestId("explore-panel-done")
    expect(panel.textContent).toContain("explore.completed")
    expect(panel.textContent).toContain("metric_get")
    expect(panel.textContent).toContain("21.5")
  })

  it("DEGRADED: no live detail and no result → renders nothing", () => {
    const { container } = render(
      <SubagentExplorePanel toolCall={baseCall} liveDetail={null} />,
    )
    expect(container.innerHTML).toBe("")
  })

  it("COMPLETED with budget exhausted shows the exhausted marker key", () => {
    const done: ToolCall = {
      ...baseCall,
      result: {
        summary: "partial",
        rounds_used: 8,
        tool_calls_made: 8,
        wall_clock_ms: 300_000,
        budget_exhausted: true,
        rounds: [],
      },
    }
    render(<SubagentExplorePanel toolCall={done} liveDetail={null} />)
    const panel = screen.getByTestId("explore-panel-done")
    expect(panel.textContent).toContain("explore.budgetExhausted")
  })
})
