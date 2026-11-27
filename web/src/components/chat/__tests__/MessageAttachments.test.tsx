import { describe, it, expect, vi } from "vitest"
import { render, screen, fireEvent } from "@testing-library/react"
import { MemoryRouter } from "react-router-dom"
import { MessageAttachments } from "../attachments/MessageAttachments"
import { mergeMessagesForDisplay } from "@/lib/messageUtils"
import type { ChatAttachment, Message } from "@/types"

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (k: string, d?: string) => (typeof d === "string" ? d : k),
      i18n: { language: "zh-CN" },
    }),
  }
})

function makeAttachment(overrides: Partial<ChatAttachment>): ChatAttachment {
  return {
    id: "device:d1",
    version: 1,
    kind: "device",
    action: "queried",
    display: "chip",
    entityId: "d1",
    title: "温室传感器",
    snapshot: {},
    ...overrides,
  }
}

describe("MessageAttachments", () => {
  it("renders media, result cards, and entity chips in separate tiers", () => {
    const attachments: ChatAttachment[] = [
      makeAttachment({
        id: "image:/api/images/cam-1.jpg",
        kind: "image",
        action: "created",
        display: "media",
        entityId: undefined,
        title: "前门抓拍",
        imageUrl: "/api/images/cam-1.jpg",
      }),
      makeAttachment({
        id: "rule:r1",
        kind: "rule",
        action: "created",
        display: "card",
        entityId: "r1",
        title: "高温告警规则",
        status: { label: "enabled", tone: "ok" },
      }),
      makeAttachment({
        id: "device:d1",
        kind: "device",
        action: "queried",
        display: "chip",
        entityId: "d1",
        title: "温室传感器",
      }),
    ]

    render(
      <MemoryRouter>
        <MessageAttachments attachments={attachments} content="已完成配置。" />
      </MemoryRouter>
    )

    expect(screen.getByTestId("attachments-media")).toBeTruthy()
    expect(screen.getByTestId("attachment-card-rule")).toBeTruthy()
    expect(screen.getByTestId("attachment-chip-device")).toBeTruthy()
  })

  it("dedupes image attachment when the same /api/images url is already inlined in markdown", () => {
    const attachments: ChatAttachment[] = [
      makeAttachment({
        id: "image:/api/images/cam-1.jpg",
        kind: "image",
        action: "created",
        display: "media",
        entityId: undefined,
        title: "前门抓拍",
        imageUrl: "/api/images/cam-1.jpg",
      }),
    ]

    const { container } = render(
      <MemoryRouter>
        <MessageAttachments
          attachments={attachments}
          content="抓拍如下：![前门](http://127.0.0.1:9375/api/images/cam-1.jpg)"
        />
      </MemoryRouter>
    )

    expect(container.querySelector('[data-testid="attachments-media"]')).toBeNull()
  })

  it("collapses cards beyond 3 and chips beyond 6 with expand controls", () => {
    const cards = Array.from({ length: 5 }, (_, i) =>
      makeAttachment({
        id: `rule:r${i}`,
        kind: "rule",
        action: "created",
        display: "card",
        entityId: `r${i}`,
        title: `规则 ${i + 1}`,
      })
    )
    const chips = Array.from({ length: 8 }, (_, i) =>
      makeAttachment({
        id: `device:d${i}`,
        kind: "device",
        action: "queried",
        display: "chip",
        entityId: `d${i}`,
        title: `设备 ${i + 1}`,
      })
    )

    render(
      <MemoryRouter>
        <MessageAttachments attachments={[...cards, ...chips]} />
      </MemoryRouter>
    )

    expect(screen.getAllByTestId("attachment-card-rule")).toHaveLength(3)
    expect(screen.getAllByTestId("attachment-chip-device")).toHaveLength(6)

    fireEvent.click(screen.getByText("展开其余 2 项"))
    expect(screen.getAllByTestId("attachment-card-rule")).toHaveLength(5)

    fireEvent.click(screen.getByText("+2"))
    expect(screen.getAllByTestId("attachment-chip-device")).toHaveLength(8)
  })

  it("triggers onAttachReference when clicking the @ button", () => {
    const onAttach = vi.fn()
    render(
      <MemoryRouter>
        <MessageAttachments
          attachments={[
            makeAttachment({
              id: "agent:ag1",
              kind: "agent",
              action: "executed",
              display: "card",
              entityId: "ag1",
              title: "巡检代理",
            }),
          ]}
          onAttachReference={onAttach}
        />
      </MemoryRouter>
    )

    fireEvent.click(screen.getByLabelText("引用 巡检代理"))
    expect(onAttach).toHaveBeenCalledWith({
      type: "agent",
      id: "ag1",
      label: "巡检代理",
    })
  })
})

describe("mergeMessagesForDisplay attachments", () => {
  it("preserves and dedupes attachments across assistant turn fragments", () => {
    const messages: Message[] = [
      { id: "u1", role: "user", content: "检查并创建规则", timestamp: 1000 },
      {
        id: "a1",
        role: "assistant",
        content: "",
        timestamp: 1001,
        tool_calls: [{ id: "tc1", name: "shell", arguments: {} }],
        attachments: [makeAttachment({ id: "device:d1", title: "传感器A" })],
      },
      {
        id: "a2",
        role: "assistant",
        content: "已创建规则。",
        timestamp: 1002,
        attachments: [
          makeAttachment({ id: "device:d1", title: "传感器A(最新)" }),
          makeAttachment({ id: "rule:r1", kind: "rule", action: "created", display: "card", entityId: "r1", title: "规则1" }),
        ],
      },
    ]

    const merged = mergeMessagesForDisplay(messages)
    expect(merged).toHaveLength(2)
    expect(merged[1].attachments).toHaveLength(2)
    expect(merged[1].attachments?.[0].title).toBe("传感器A(最新)")
    expect(merged[1].attachments?.[1].id).toBe("rule:r1")
  })
})
