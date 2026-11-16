import { describe, expect, it } from "vitest"
import {
  CapError,
  MAX_REFERENCE_NODES,
  backspace,
  consumeAtTrigger,
  deleteReferenceAt,
  detectAtTrigger,
  emptyDoc,
  insertReference,
  toContentNodes,
  toLegacyUniqueReferences,
  toPlainText,
} from "../inlineDoc"

describe("inlineDoc model", () => {
  it("starts empty with a single text node", () => {
    expect(emptyDoc()).toEqual({ nodes: [{ kind: "text", text: "" }] })
  })

  it("inserts two references mid-sentence and preserves order", () => {
    // "把 _ 的规则复制给 _"
    const base = { nodes: [{ kind: "text" as const, text: "把 的规则复制给" }] }
    const step1 = insertReference(base, { nodeIndex: 0, offset: 1 }, {
      entityType: "device",
      id: "dev-a",
      labelSnapshot: "设备A",
    })
    // nodes: ["把", refA, "的规则复制给"]
    const after = step1.doc.nodes[2]
    expect(after.kind).toBe("text")
    const step2 = insertReference(
      step1.doc,
      { nodeIndex: 2, offset: (after as { text: string }).text.length },
      { entityType: "device", id: "dev-b", labelSnapshot: "设备B" },
    )
    expect(toPlainText(step2.doc)).toBe("把@设备A 的规则复制给@设备B")
    expect(toContentNodes(step2.doc).filter((n) => n.kind === "reference")).toHaveLength(2)
  })

  it("allows the same entity twice in-document (no node dedupe)", () => {
    const base = { nodes: [{ kind: "text" as const, text: "compare  vs " }] }
    const first = insertReference(base, { nodeIndex: 0, offset: 8 }, {
      entityType: "device",
      id: "sensor-001",
      labelSnapshot: "Living Room",
    })
    const second = insertReference(first.doc, { nodeIndex: 2, offset: 4 }, {
      entityType: "device",
      id: "sensor-001",
      labelSnapshot: "Living Room",
    })
    expect(toContentNodes(second.doc).filter((n) => n.kind === "reference")).toHaveLength(2)
    expect(toLegacyUniqueReferences(second.doc)).toHaveLength(1)
  })

  it("backspace deletes an atomic reference at offset 0 after it", () => {
    const inserted = insertReference(
      { nodes: [{ kind: "text", text: "see " }] },
      { nodeIndex: 0, offset: 4 },
      { entityType: "rule", id: "r1", labelSnapshot: "high-temp" },
    )
    const after = backspace(inserted.doc, inserted.caret)
    expect(after).not.toBeNull()
    expect(toPlainText(after!.doc)).toBe("see ")
    expect(toContentNodes(after!.doc).some((n) => n.kind === "reference")).toBe(false)
  })

  it("deleteReferenceAt merges surrounding text", () => {
    const inserted = insertReference(
      { nodes: [{ kind: "text", text: "left right" }] },
      { nodeIndex: 0, offset: 5 },
      { entityType: "agent", id: "a1", labelSnapshot: "Watcher" },
    )
    const deleted = deleteReferenceAt(inserted.doc, 1)
    expect(toPlainText(deleted.doc)).toBe("left right")
    expect(deleted.caret).toEqual({ nodeIndex: 0, offset: 5 })
  })

  it("detectAtTrigger ignores email-like @ and accepts Chinese boundaries", () => {
    expect(detectAtTrigger("a@b.com")).toBeNull()
    expect(detectAtTrigger("contact a@b")).toBeNull()
    expect(detectAtTrigger("看看 @温")).toEqual({ query: "温", start: 3 })
    expect(detectAtTrigger("@")).toEqual({ query: "", start: 0 })
    expect(detectAtTrigger("!!!@设备")).toEqual({ query: "设备", start: 3 })
  })

  it("consumeAtTrigger strips @query before inserting", () => {
    const doc = { nodes: [{ kind: "text" as const, text: "检查 @温湿度" }] }
    const caret = { nodeIndex: 0, offset: "检查 @温湿度".length }
    const trig = detectAtTrigger(doc.nodes[0].text)!
    const stripped = consumeAtTrigger(doc, caret, trig.start)
    expect(toPlainText(stripped.doc)).toBe("检查 ")
  })

  it("throws CapError when reference node cap exceeded", () => {
    let doc = emptyDoc()
    for (let i = 0; i < MAX_REFERENCE_NODES; i++) {
      const textIdx = doc.nodes.findIndex((n) => n.kind === "text")
      doc = insertReference(
        doc,
        { nodeIndex: textIdx, offset: 0 },
        { entityType: "device", id: `d-${i}`, labelSnapshot: `D${i}` },
      ).doc
    }
    expect(() =>
      insertReference(
        doc,
        { nodeIndex: doc.nodes.findIndex((n) => n.kind === "text"), offset: 0 },
        { entityType: "device", id: "overflow", labelSnapshot: "X" },
      ),
    ).toThrow(CapError)
  })
})
