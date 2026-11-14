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

  it("opens listbox on mobile force path", async () => {
    render(<InlineAtEditor search={search} forceMobile />)
    fireEvent.click(screen.getByLabelText("插入 @ 引用"))
    expect(await screen.findByRole("option", { name: /green-01/ })).toBeTruthy()
  })
})
