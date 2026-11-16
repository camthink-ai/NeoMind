import { describe, expect, it } from "vitest"
import {
  contentNodesToInlineDoc,
  inlineDocToChatPayload,
  inlineDocToTipTapJson,
  tipTapJsonToInlineDoc,
} from "../docBridge"
import type { InlineDoc } from "@/lib/chat/inlineDoc"

describe("inline-mention docBridge", () => {
  it("round-trips in-sentence duplicate mentions", () => {
    const doc: InlineDoc = {
      nodes: [
        { kind: "text", text: "把 " },
        {
          kind: "reference",
          entityType: "device",
          id: "green-01",
          labelSnapshot: "G",
        },
        { kind: "text", text: " 复制给 " },
        {
          kind: "reference",
          entityType: "device",
          id: "green-01",
          labelSnapshot: "G",
        },
      ],
    }
    const json = inlineDocToTipTapJson(doc)
    const back = tipTapJsonToInlineDoc(json)
    const payload = inlineDocToChatPayload(back)
    expect(payload.contentNodes.filter((n) => n.kind === "reference")).toHaveLength(2)
    expect(payload.references).toHaveLength(1)
    expect(payload.message).toContain("@G")
  })

  it("restores contentNodes into an InlineDoc", () => {
    const doc = contentNodesToInlineDoc([
      { kind: "text", text: "检查 " },
      {
        kind: "reference",
        type: "device",
        id: "sensor-001",
        labelSnapshot: "温湿度",
      },
    ])
    expect(doc.nodes.some((n) => n.kind === "reference" && n.id === "sensor-001")).toBe(
      true,
    )
  })

  it("drops mention nodes with invalid/missing entityType instead of coercing to device (E18)", () => {
    const json = {
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            { type: "text", text: "看 " },
            // Pasted HTML with a bogus data-entity-type — must not become a device ref.
            { type: "mention", attrs: { id: "bogus-1", label: "假设备", entityType: "bogus" } },
            // Missing entityType entirely — same rule.
            { type: "mention", attrs: { id: "missing-1", label: "无类型" } },
            // Valid entityType survives.
            { type: "mention", attrs: { id: "rule-9", label: "高温报警", entityType: "rule" } },
          ],
        },
      ],
    }
    const doc = tipTapJsonToInlineDoc(json)
    const refs = doc.nodes.filter(
      (n): n is Extract<typeof n, { kind: "reference" }> => n.kind === "reference",
    )
    expect(refs.map((r) => r.id)).toEqual(["rule-9"])
    expect(refs[0].entityType).toBe("rule")
    expect(doc.nodes.some((n) => n.kind === "reference" && n.id === "bogus-1")).toBe(false)
    expect(doc.nodes.some((n) => n.kind === "reference" && n.id === "missing-1")).toBe(false)
  })
})
