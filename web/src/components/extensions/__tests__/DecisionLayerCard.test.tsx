/**
 * DecisionLayerCard — three-state contract:
 * 1. guidance mode (shadow=false): mode badge + backend + counters.
 * 2. shadow mode: record-only badge.
 * 3. disabled (enabled=false): off hint, no counters.
 *
 * i18n: the global setup mocks t() to return the key — assertions match on
 * keys, not translated strings.
 */
import { describe, it, expect, vi } from "vitest"
import { render, screen, waitFor } from "@testing-library/react"
import { DecisionLayerCard } from "../DecisionLayerCard"

vi.mock("@/lib/api/decisions", () => ({
  decisionsApi: {
    getDecisionStatus: vi.fn(),
  },
}))

import { decisionsApi } from "@/lib/api/decisions"
const mockGet = decisionsApi.getDecisionStatus as ReturnType<typeof vi.fn>

describe("DecisionLayerCard", () => {
  it("guidance mode shows the mode badge, backend and counters", async () => {
    mockGet.mockResolvedValue({
      enabled: true,
      available: true,
      shadow: false,
      backend: "native-onnx",
      decides_total: 12,
      cache_hits_total: 3,
      errors_total: 0,
    })
    render(<DecisionLayerCard />)
    const card = await waitFor(() => screen.getByTestId("decision-layer-card"))
    expect(screen.getByTestId("decision-layer-mode").textContent).toContain("decision.modeGuidance")
    expect(card.textContent).toContain("native-onnx")
    expect(card.textContent).toContain("12")
    expect(card.textContent).toContain("3")
  })

  it("shadow mode shows the record-only badge", async () => {
    mockGet.mockResolvedValue({
      enabled: true,
      available: true,
      shadow: true,
      backend: "native-onnx",
      decides_total: 5,
    })
    render(<DecisionLayerCard />)
    await waitFor(() => screen.getByTestId("decision-layer-mode"))
    expect(screen.getByTestId("decision-layer-mode").textContent).toContain("decision.modeShadow")
  })

  it("disabled layer shows the off hint without counters", async () => {
    mockGet.mockResolvedValue({ enabled: false })
    render(<DecisionLayerCard />)
    const card = await waitFor(() => screen.getByTestId("decision-layer-card"))
    expect(card.textContent).toContain("decision.offHint")
    expect(screen.queryByTestId("decision-layer-mode")).toBeNull()
  })

  it("fetch failure degrades to the off state", async () => {
    mockGet.mockRejectedValue(new Error("network"))
    render(<DecisionLayerCard />)
    await waitFor(() => screen.getByTestId("decision-layer-card"))
    expect(screen.queryByTestId("decision-layer-mode")).toBeNull()
  })
})
