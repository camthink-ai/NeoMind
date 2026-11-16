import { describe, expect, it, beforeEach, vi } from "vitest"
import {
  ContentProtocolError,
  MAX_CONTENT_NODES,
  buildChatRequestFromChips,
  buildChatRequestFromDoc,
  chipsAndTextToContentNodes,
  canHydrateDraftIntoEditor,
  clearPendingDraftsForTests,
  clearSessionDraft,
  contentNodesHaveReferences,
  contentNodesToComposerFields,
  loadSessionDraft,
  markDraftForRetry,
  normalizeContentNodes,
  pendingDraftCount,
  pruneSessionDrafts,
  saveSessionDraft,
  settlePendingDraft,
  stashPendingDraft,
  validateContentNodes,
  validateSendContent,
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
      get length() {
        return store.size
      },
      key: (i: number) => Array.from(store.keys())[i] ?? null,
    })
    clearSessionDraft("s1")
    clearSessionDraft("new")
    clearSessionDraft("side-chat")
    clearPendingDraftsForTests()
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

  it("failed-send stash restores via markDraftForRetry without requiring a prior clear (P2)", () => {
    // Mimic the composer stash→clear-UI→send-fail path: nothing was cleared
    // from storage yet; markDraftForRetry must still write a recoverable draft.
    clearSessionDraft("side-chat")
    expect(loadSessionDraft("side-chat")).toBeNull()
    const stashed = markDraftForRetry("side-chat", {
      message: "panel draft",
      references: [{ type: "agent", id: "ag-1", label: "巡检" }],
      contentNodes: [
        { kind: "text", text: "panel " },
        { kind: "reference", type: "agent", id: "ag-1", labelSnapshot: "巡检" },
      ],
    })
    expect(stashed.message).toBe("panel draft")
    expect(loadSessionDraft("side-chat")?.references).toHaveLength(1)
    expect(loadSessionDraft("side-chat")?.contentNodes).toHaveLength(2)
  })

  it("pending draft stays until settle accepted; reject restores stash by request id", () => {
    stashPendingDraft("req-1", "s1", {
      message: "keep me",
      references: [{ type: "device", id: "d1", label: "D1" }],
      contentNodes: [{ kind: "text", text: "keep me" }],
    })
    expect(pendingDraftCount()).toBe(1)
    expect(loadSessionDraft("s1")?.message).toBe("keep me")

    // Local wire/queue success must NOT clear — only accepted settle does.
    expect(settlePendingDraft("req-1", "accepted")).toBeNull()
    expect(loadSessionDraft("s1")).toBeNull()
    expect(pendingDraftCount()).toBe(0)

    stashPendingDraft("req-2", "side-chat", {
      message: "panel keep",
      references: [],
    })
    const restored = settlePendingDraft("req-2", "failed")
    expect(restored?.message).toBe("panel keep")
    expect(restored?.sessionId).toBe("side-chat")
    expect(loadSessionDraft("side-chat")?.message).toBe("panel keep")
  })

  it("failed settle keeps original session draft; hydrate only when same session + empty editor", () => {
    stashPendingDraft("req-a", "s1", { message: "original", references: [] })
    const restored = settlePendingDraft("req-a", "failed")
    expect(restored?.sessionId).toBe("s1")
    expect(loadSessionDraft("s1")?.message).toBe("original")
    // Switched away — do not hydrate into the other session's editor.
    expect(
      canHydrateDraftIntoEditor("s1", "s2", {
        message: "",
        references: [],
      }),
    ).toBe(false)
    // Same session but user already typed — do not overwrite.
    expect(
      canHydrateDraftIntoEditor("s1", "s1", {
        message: "new typing",
        references: [],
      }),
    ).toBe(false)
    // Same session + empty composer after optimistic clear — ok to hydrate.
    expect(
      canHydrateDraftIntoEditor("s1", "s1", {
        message: "",
        references: [],
        contentNodes: [],
        images: [],
      }),
    ).toBe(true)
  })

  it("send on A then switch to B: A timeout must not hydrate B (live session ref)", () => {
    // Mimic ACK callback reading a LIVE session ref after a route switch.
    const liveSessionRef = { current: "session-a" }
    stashPendingDraft("req-switch", "session-a", {
      message: "draft for A",
      references: [],
    })
    // User navigates to B while waiting for A's ACK.
    liveSessionRef.current = "session-b"
    const restored = settlePendingDraft("req-switch", "failed")
    expect(restored?.sessionId).toBe("session-a")
    expect(loadSessionDraft("session-a")?.message).toBe("draft for A")
    expect(loadSessionDraft("session-b")).toBeNull()
    expect(
      canHydrateDraftIntoEditor(restored!.sessionId, liveSessionRef.current, {
        message: "",
        references: [],
      }),
    ).toBe(false)
  })

  it("validateSendContent mirrors the server caps for the send path (E14)", () => {
    expect(validateSendContent([])).toBeNull()
    expect(validateSendContent([{ kind: "text", text: "hello" }])).toBeNull()

    // 20 duplicate inline mentions of ONE entity: the unique-count insertion
    // guard passes and the total ref-node cap is exactly at its boundary —
    // still sendable.
    const twentyDup = [
      { kind: "text" as const, text: "x " },
      ...Array.from({ length: 20 }, () => ({
        kind: "reference" as const,
        type: "device" as const,
        id: "same-1",
        labelSnapshot: "同一设备",
      })),
    ]
    expect(validateSendContent(twentyDup)).toBeNull()

    // 21 total nodes of the same entity — the exact client gap that used to
    // pass insertion-time checks and hit the server 400 after the composer
    // was cleared.
    const twentyOneDup = [...twentyDup, twentyDup[1]]
    expect(validateSendContent(twentyOneDup)).toBe("content_ref_nodes_cap")

    // 21 UNIQUE references are equally over the total ref-node cap.
    const unique21 = Array.from({ length: 21 }, (_, i) => ({
      kind: "reference" as const,
      type: "device" as const,
      id: `d-${i}`,
      labelSnapshot: `D${i}`,
    }))
    expect(validateSendContent(unique21)).toBe("content_ref_nodes_cap")

    // Text char cap: 8000 boundary passes, 8001 fails (with refs attached so
    // the nodes are actually sent).
    const boundary = [
      { kind: "reference" as const, type: "rule" as const, id: "r-1", labelSnapshot: "R" },
      { kind: "text" as const, text: "a".repeat(8000) },
    ]
    expect(validateSendContent(boundary)).toBeNull()
    const over = [
      boundary[0],
      { kind: "text" as const, text: "a".repeat(8001) },
    ]
    expect(validateSendContent(over)).toBe("content_text_cap")
  })

  it("draft lifecycle: save → load restores composer fields, clear removes the key (E15)", () => {
    saveSessionDraft({
      sessionId: "s9",
      message: "检查 @温湿度",
      references: [{ type: "device", id: "sensor-001", label: "温湿度" }],
      contentNodes: [
        { kind: "text", text: "检查 " },
        { kind: "reference", type: "device", id: "sensor-001", labelSnapshot: "温湿度" },
      ],
      updatedAt: 1,
    })
    const loaded = loadSessionDraft("s9")
    expect(loaded?.message).toBe("检查 @温湿度")
    expect(loaded?.references).toHaveLength(1)
    expect(loaded?.contentNodes).toHaveLength(2)
    // Sessions stay isolated — the neighboring key is untouched.
    expect(loadSessionDraft("s10")).toBeNull()
    // Successful-send cleanup: the key is gone after clear.
    clearSessionDraft("s9")
    expect(loadSessionDraft("s9")).toBeNull()
    // Clearing a missing key (send after draft expiry) is a no-op.
    expect(() => clearSessionDraft("s9")).not.toThrow()
  })

  it("normalizeContentNodes accepts camelCase and snake_case aliases", () => {
    const nodes = normalizeContentNodes([
      { kind: "text", text: "把 " },
      {
        kind: "reference",
        type: "device",
        id: "green-01",
        label_snapshot: "温室A",
      },
      { kind: "text", text: " 的规则复制给 " },
      {
        kind: "reference",
        type: "device",
        id: "green-02",
        labelSnapshot: "温室B",
      },
    ])
    expect(nodes).toHaveLength(4)
    expect(contentNodesHaveReferences(nodes)).toBe(true)
    expect(nodes![1]).toMatchObject({
      kind: "reference",
      id: "green-01",
      labelSnapshot: "温室A",
    })
    const fields = contentNodesToComposerFields(nodes!)
    expect(fields.message).toBe("把 @温室A 的规则复制给 @温室B")
    expect(fields.references).toHaveLength(2)
    expect(fields.contentNodes).toEqual(nodes)
  })

  it("normalizeContentNodes drops junk and empty arrays", () => {
    expect(normalizeContentNodes([])).toBeUndefined()
    expect(normalizeContentNodes([{ kind: "reference", type: "nope", id: "x" }])).toBeUndefined()
    expect(normalizeContentNodes(null)).toBeUndefined()
  })

  it("pruneSessionDrafts keeps the newest N draft keys and drops the rest", () => {
    // Seed 6 drafts, then patch deterministic timestamps (saveSessionDraft
    // stamps Date.now(), which would make the ordering ambiguous).
    const ids = ["s0", "s1", "s2", "s3", "s4", "new"]
    ids.forEach((id) =>
      saveSessionDraft({ sessionId: id, message: `m-${id}`, references: [], updatedAt: Date.now() }),
    )
    ids.forEach((id, i) => {
      const key = `neomind:chat-draft:v1:${id}`
      const raw = localStorage.getItem(key)
      if (raw) {
        localStorage.setItem(key, JSON.stringify({ ...JSON.parse(raw), updatedAt: 1000 * (i + 1) }))
      }
    })
    pruneSessionDrafts(3)
    expect(loadSessionDraft("s0")).toBeNull()
    expect(loadSessionDraft("s1")).toBeNull()
    expect(loadSessionDraft("s2")).toBeNull()
    expect(loadSessionDraft("s3")).not.toBeNull()
    expect(loadSessionDraft("s4")).not.toBeNull()
    expect(loadSessionDraft("new")).not.toBeNull()
    // Non-draft keys untouched.
    localStorage.setItem("neomind:other", "x")
    pruneSessionDrafts(0)
    expect(localStorage.getItem("neomind:other")).toBe("x")
  })
})
