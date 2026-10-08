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

  it("COMPLETED (live wire): string-shaped result is parsed and rendered", () => {
    // The ToolCallEnd wire event carries the result as a display STRING —
    // the JSON text — not the parsed object (only persisted history has the
    // object). This is the "explore 没有任何内容" regression: string.summary
    // was undefined and the body rendered empty.
    const done: ToolCall = {
      ...baseCall,
      result: JSON.stringify({
        summary: "3号冷库压缩机停机2小时",
        rounds_used: 3,
        tool_calls_made: 5,
        wall_clock_ms: 42_000,
        budget_exhausted: false,
        rounds: [{ tool: "metric_get", note: "电流过载" }],
      }),
    }
    render(<SubagentExplorePanel toolCall={done} liveDetail={null} />)
    const panel = screen.getByTestId("explore-panel-done")
    expect(panel.textContent).toContain("3号冷库压缩机停机2小时")
    expect(panel.textContent).toContain("metric_get")
  })

  it("COMPLETED (live wire): non-JSON string degrades to a summary line, not empty", () => {
    const done: ToolCall = {
      ...baseCall,
      // e.g. sanitizer-truncated JSON — unparseable, must not blank the panel
      result: '{"summary": "冷库温度正…[trunc',
    }
    render(<SubagentExplorePanel toolCall={done} liveDetail={null} />)
    const panel = screen.getByTestId("explore-panel-done")
    expect(panel.textContent).toContain("冷库温度正")
  })

  it("COMPLETED shows the summary above the rounds list", () => {
    const done: ToolCall = {
      ...baseCall,
      result: {
        summary: "巡检结论：一切正常",
        rounds_used: 2,
        tool_calls_made: 4,
        wall_clock_ms: 10_000,
        rounds: [{ tool: "web_fetch", note: "抓取设备页" }],
      },
    }
    render(<SubagentExplorePanel toolCall={done} liveDetail={null} />)
    const panel = screen.getByTestId("explore-panel-done")
    expect(panel.textContent).toContain("巡检结论：一切正常")
    expect(panel.textContent).toContain("web_fetch")
  })
})
