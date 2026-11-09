/** Tests for isToolError — mirrors the backend's `effective_success` for the
 * shell branch so the UI can't render a failed command as successful.
 * Review cases (2026-11-10) plus REAL replay fixtures from the 2026-11-08
 * eval batch (rule-create 400 with suggestion; device-list success). */
import { describe, expect, it } from "vitest"
import { isToolError } from "../toolCallUtils"
import type { ToolCall } from "@/types"

const tc = (over: Partial<ToolCall>): ToolCall => ({
  id: "t", name: "shell", arguments: {}, ...over,
})

describe("isToolError — shell envelope (effective_success mirror)", () => {
  it("outer success=true + exit_code=2 is a FAILURE (the original bug)", () => {
    expect(isToolError(tc({ success: true, result: { exit_code: 2, stdout: "", stderr: "error: unexpected argument '--x' found" } }))).toBe(true)
  })

  it("timed_out=true is a failure", () => {
    expect(isToolError(tc({ success: true, result: { exit_code: 124, timed_out: true, stdout: "" } }))).toBe(true)
  })

  it("exit_code=0 with clean output is a success", () => {
    expect(isToolError(tc({ success: true, result: { exit_code: 0, stdout: "ok", stderr: "" } }))).toBe(false)
  })

  it("pipeline masking: exit 0 + 'command not found' text is a failure (backend parity)", () => {
    expect(isToolError(tc({ success: true, result: { exit_code: 0, stdout: "…: command not found", stderr: "" } }))).toBe(true)
  })

  it("JSON-STRING result shape is parsed (persisted/eval records)", () => {
    expect(isToolError(tc({ result: '{"exit_code":2,"stdout":"","stderr":"error: unexpected argument"}' }))).toBe(true)
    expect(isToolError(tc({ result: '{"exit_code":0,"stdout":"[]","stderr":""}' }))).toBe(false)
  })

  it("no `success` flag (reloaded history) still detects failures via the envelope", () => {
    expect(isToolError(tc({ result: { exit_code: 1, stderr: "API error (400)" } }))).toBe(true)
  })

  it("REGRESSION (2026-11-10 review): shell result with an error envelope and NO exit code is a failure", () => {
    expect(isToolError(tc({ result: { error: "Tool execution failed" } }))).toBe(true)
  })

  it("REGRESSION: same case in JSON-string shape", () => {
    expect(isToolError(tc({ result: '{"error":"Tool execution failed"}' }))).toBe(true)
  })

  it("clean shell envelope with no error envelope remains a success (fall-through must not over-flag)", () => {
    expect(isToolError(tc({ result: { exit_code: 0, stdout: "ok", stderr: "" } }))).toBe(false)
  })
})

describe("isToolError — domain tools", () => {
  it("outer success=false is a failure (unchanged)", () => {
    expect(isToolError(tc({ name: "memory", success: false }))).toBe(true)
  })

  it("non-empty error envelope is a failure", () => {
    expect(isToolError(tc({ name: "memory", result: { error: "not found" } }))).toBe(true)
  })

  it("business data merely CONTAINING an error field is NOT a failure", () => {
    expect(isToolError(tc({ name: "memory", result: { error: null, data: { items: 3 } } }))).toBe(false)
    expect(isToolError(tc({ name: "memory", result: { data: { error_count: 5 } } }))).toBe(false)
  })
})

describe("isToolError — real replay fixtures from the 2026-11-08 batch", () => {
  it("rule-create 400 with suggestion renders as failed", () => {
    const real = {
      command: "neomind rule create --name recovery-metric --trigger-device sensor-001 --metric humidity",
      description: "Create rule",
      exit_code: 1,
      stdout: "",
      stderr: "API error (400 Bad Request): Rule references unavailable resources:\n- Metric 'humidity' not supported by device 'sensor-001' (condition)",
      suggestion: "Rule JSON format: ...",
      timed_out: false,
    }
    expect(isToolError(tc({ success: true, result: real }))).toBe(true)
  })

  it("successful device telemetry query renders as success", () => {
    const real = { exit_code: 0, stdout: '[{"id":"sensor-001","temperature":22.5}]', stderr: "", timed_out: false }
    expect(isToolError(tc({ success: true, result: real }))).toBe(false)
  })
})
