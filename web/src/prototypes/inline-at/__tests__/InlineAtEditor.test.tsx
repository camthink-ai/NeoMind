import { describe, expect, it, vi } from "vitest"
import { render, screen, fireEvent, waitFor } from "@testing-library/react"
import { InlineAtEditor } from "../InlineAtEditor"

vi.mock("react-i18next", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-i18next")>()
  return {
    ...actual,
    useTranslation: () => ({
      t: (_key: string, fallback?: string | { defaultValue?: string }) =>
        typeof fallback === "string"
          ? fallback
          : (fallback?.defaultValue ?? _key),
      i18n: { language: "zh" },
    }),
  }
})

vi.mock("@/hooks/useMobile", () => ({
  useIsMobile: () => false,
}))

const search = vi.fn(async (q: string) =>
  [
    { type: "device" as const, id: "green-01", label: "green-01" },
    { type: "device" as const, id: "sensor-001", label: "Living Room Sensor" },
  ].filter((o) => !q || o.label.includes(q) || o.id.includes(q)),
)

describe("InlineAtEditor prototype", () => {
  it("opens picker from toolbar @ and inserts an inline chip", async () => {
    render(<InlineAtEditor search={search} forceMobile={false} />)
    fireEvent.click(screen.getByLabelText("插入 @ 引用"))
    const opt = await screen.findByRole("option", { name: /green-01/ })
    fireEvent.click(opt)
    await waitFor(() => {
      expect(document.querySelector("[data-inline-at-chip]")).toBeTruthy()
    })
    expect(document.querySelector("[data-inline-at-plain]")?.textContent).toContain("@green-01")
  })

  it("allows duplicate entity chips at two positions", async () => {
    render(<InlineAtEditor search={search} forceMobile={false} />)
    fireEvent.click(screen.getByLabelText("插入 @ 引用"))
    fireEvent.click(await screen.findByRole("option", { name: /green-01/ }))
    await waitFor(() => expect(document.querySelectorAll("[data-inline-at-chip]").length).toBe(1))

    fireEvent.click(screen.getByLabelText("插入 @ 引用"))
    fireEvent.click(await screen.findByRole("option", { name: /green-01/ }))
    await waitFor(() => expect(document.querySelectorAll("[data-inline-at-chip]").length).toBe(2))

    const legacy = document.querySelector("[data-inline-at-legacy]")?.textContent ?? ""
    expect(legacy).toContain("device:green-01")
    expect(legacy.split("device:green-01").length - 1).toBe(1)
  })

  it("does not open picker during IME composition", () => {
    render(<InlineAtEditor search={search} forceMobile={false} />)
    const ta = screen.getAllByLabelText("消息文本")[0]
    fireEvent.compositionStart(ta)
    fireEvent.change(ta, { target: { value: "输入中 @", selectionStart: 5 } })
    expect(screen.queryByRole("listbox")).toBeNull()
  })

  it("opens picker on compositionend without an extra keystroke", async () => {
    search.mockClear()
    render(<InlineAtEditor search={search} forceMobile={false} />)
    const ta = screen.getAllByLabelText("消息文本")[0] as HTMLTextAreaElement
    fireEvent.compositionStart(ta)
    fireEvent.compositionEnd(ta, {
      target: { value: "看看 @green", selectionStart: "看看 @green".length },
    })
    // compositionend handler reads currentTarget; set value then fire properly.
    fireEvent.compositionStart(ta)
    Object.defineProperty(ta, "value", { configurable: true, value: "看看 @green" })
    Object.defineProperty(ta, "selectionStart", {
      configurable: true,
      value: "看看 @green".length,
    })
    fireEvent.compositionEnd(ta)
    await waitFor(() => expect(screen.getByRole("listbox")).toBeTruthy())
  })

  it("Escape closes picker; ArrowDown moves aria-selected", async () => {
    render(<InlineAtEditor search={search} forceMobile={false} />)
    fireEvent.click(screen.getByLabelText("插入 @ 引用"))
    const options = await screen.findAllByRole("option")
    expect(options[0]).toHaveAttribute("aria-selected", "true")
    const ta = screen.getAllByLabelText("消息文本")[0]
    fireEvent.keyDown(ta, { key: "ArrowDown" })
    await waitFor(() => {
      expect(screen.getAllByRole("option")[1]).toHaveAttribute("aria-selected", "true")
    })
    fireEvent.keyDown(ta, { key: "Escape" })
    expect(screen.queryByRole("listbox")).toBeNull()
  })

  it("IME Enter (keyCode 229) does not insert a chip", async () => {
    render(<InlineAtEditor search={search} forceMobile={false} />)
    fireEvent.click(screen.getByLabelText("插入 @ 引用"))
    await screen.findAllByRole("option")
    const ta = screen.getAllByLabelText("消息文本")[0]
    fireEvent.keyDown(ta, { key: "Enter", keyCode: 229, isComposing: true })
    expect(document.querySelector("[data-inline-at-chip]")).toBeNull()
  })

  it("exposes option aria-label with type and name for screen readers", async () => {
    render(<InlineAtEditor search={search} forceMobile={false} />)
    fireEvent.click(screen.getByLabelText("插入 @ 引用"))
    const opt = await screen.findByRole("option", { name: /green-01/ })
    expect(opt.getAttribute("aria-label") || "").toMatch(/device/)
    expect(opt.getAttribute("aria-label") || "").toMatch(/green-01/)
  })

  it("shows duplicate-name disambiguation text", async () => {
    const dupSearch = vi.fn(async () => [
      {
        type: "device" as const,
        id: "a",
        label: "温湿度传感器",
        shortId: "a",
        hint: "temp · mqtt",
        duplicateName: true,
      },
      {
        type: "device" as const,
        id: "b",
        label: "温湿度传感器",
        shortId: "b",
        hint: "temp · mqtt",
        duplicateName: true,
      },
    ])
    render(<InlineAtEditor search={dupSearch} forceMobile={false} />)
    fireEvent.click(screen.getByLabelText("插入 @ 引用"))
    const opt = await screen.findAllByRole("option")
    expect(opt[0].textContent).toContain("temp · mqtt")
    expect(opt[0].textContent).toContain("a")
  })

  it("opens listbox on mobile force path", async () => {
    render(<InlineAtEditor search={search} forceMobile />)
    fireEvent.click(screen.getByLabelText("插入 @ 引用"))
    expect(await screen.findByRole("option", { name: /green-01/ })).toBeTruthy()
  })
})
