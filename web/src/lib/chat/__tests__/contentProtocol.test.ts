import { describe, expect, it, beforeEach, vi } from "vitest"
import {
  ContentProtocolError,
  MAX_CONTENT_NODES,
  buildChatRequestFromChips,
  buildChatRequestFromDoc,
  chipsAndTextToContentNodes,
  clearSessionDraft,
  loadSessionDraft,
  markDraftForRetry,
  saveSessionDraft,
  validateContentNodes,
} from "../contentProtocol"
import { emptyDoc, insertReference, setTextNode } from "@/prototypes/inline-at/model"
import { CHAT_CONTENT_VERSION_V1 } from "@/types"

describe("contentProtocol", () => {
  beforeEach(() => {
    // Vitest/node may lack a writable localStorage — stub a Map-backed one.
    const store = new Map<string, string>()
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => {
        store.set(k, v)
      },
      removeItem: (k: string) => {
        store.delete(k)
      },
      clear: () => store.clear(),
    })
    clearSessionDraft("s1")
    clearSessionDraft("new")
  })

  it("builds a v1 request with in-order nodes and legacy unique refs", () => {
    let doc = emptyDoc()
    ;({ doc } = setTextNode(doc, 0, "把 复制给 ", 1))
    doc = { nodes: [{ kind: "text", text: "把 复制给 " }] }
    const a = insertReference(doc, { nodeIndex: 0, offset: 1 }, {
      entityType: "device",
      id: "green-01",
      labelSnapshot: "G",
    })
    const b = insertReference(a.doc, { nodeIndex: 2, offset: 4 }, {
      entityType: "device",
      id: "green-01",
      labelSnapshot: "G",
    })
    const req = buildChatRequestFromDoc(b.doc)
    expect(req.contentVersion).toBe(CHAT_CONTENT_VERSION_V1)
    expect(req.contentNodes?.filter((n) => n.kind === "reference")).toHaveLength(2)
    expect(req.references).toHaveLength(1)
    expect(req.message).toContain("@G")
  })

  it("rejects oversized node lists with an explicit cap error", () => {
    const nodes = Array.from({ length: MAX_CONTENT_NODES + 1 }, (_, i) => ({
      kind: "text" as const,
      text: `t${i}`,
    }))
    expect(() => validateContentNodes(nodes)).toThrow(ContentProtocolError)
    try {
      validateContentNodes(nodes)
    } catch (e) {
      expect(e).toBeInstanceOf(ContentProtocolError)
      expect((e as ContentProtocolError).code).toBe("content_nodes_cap")
    }
  })

  it("builds production chip+text nodes for persistence and send", () => {
    const nodes = chipsAndTextToContentNodes("检查设备", [
      { type: "device", id: "sensor-001", label: "温湿度" },
    ])
    expect(nodes).toEqual([
      { kind: "text", text: "检查设备" },
      {
        kind: "reference",
        type: "device",
        id: "sensor-001",
        labelSnapshot: "温湿度",
      },
    ])
    const req = buildChatRequestFromChips("检查设备", [
      { type: "device", id: "sensor-001", label: "温湿度" },
    ])
    expect(req.contentVersion).toBe(CHAT_CONTENT_VERSION_V1)
    expect(req.contentNodes).toEqual(nodes)
    expect(req.references).toHaveLength(1)
  })

  it("keeps drafts isolated per session and restores after failed send", () => {
    saveSessionDraft({
      sessionId: "s1",
      message: "hello @A",
      references: [{ type: "device", id: "a", label: "A" }],
      contentNodes: [
        { kind: "text", text: "hello " },
        { kind: "reference", type: "device", id: "a", labelSnapshot: "A" },
      ],
      updatedAt: 1,
    })
    expect(loadSessionDraft("s2")).toBeNull()
    expect(loadSessionDraft("s1")?.message).toBe("hello @A")

    const restored = markDraftForRetry("s1", { message: "hello @A (retry)" })
    expect(restored.message).toContain("retry")
    expect(loadSessionDraft("s1")?.contentNodes?.length).toBe(2)
  })
})
