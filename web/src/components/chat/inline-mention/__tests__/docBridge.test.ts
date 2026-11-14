import { describe, expect, it } from "vitest"
import {
  contentNodesToInlineDoc,
  inlineDocToChatPayload,
  inlineDocToTipTapJson,
  tipTapJsonToInlineDoc,
} from "../docBridge"
import type { InlineDoc } from "@/prototypes/inline-at/model"

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
})
