/**
 * Dev-only prototype page for plan iter B (inline @).
 * Route: /dev/inline-at — not linked from primary nav.
 */
import { useCallback, useState } from "react"
import { useTranslation } from "react-i18next"
import { PageLayout } from "@/components/layout/PageLayout"
import type { MentionOption } from "@/lib/api/mentions"
import { InlineAtEditor } from "./InlineAtEditor"
import type { InlineDoc } from "./model"
import { toContentNodes, toPlainText } from "./model"

const MOCK_OPTIONS: MentionOption[] = [
  { type: "device", id: "green-01", label: "green-01", shortId: "green-01", hint: "camera · mqtt" },
  {
    type: "device",
    id: "sensor-001",
    label: "温湿度传感器",
    shortId: "sensor-0",
    hint: "temp_humidity · mqtt",
    duplicateName: true,
  },
  {
    type: "device",
    id: "sensor-002",
    label: "温湿度传感器",
    shortId: "sensor-0",
    hint: "temp_humidity · mqtt",
    duplicateName: true,
  },
  { type: "rule", id: "rule-battery-low", label: "rule-battery-low", shortId: "rule-bat", hint: "enabled" },
  { type: "rule", id: "high-temp-green", label: "high-temp-green", shortId: "high-tem", hint: "enabled" },
  { type: "transform", id: "temp-f", label: "temp-f", shortId: "temp-f", hint: "enabled · global" },
  { type: "agent", id: "agent-watch", label: "Watcher", shortId: "agent-wa", hint: "active" },
]

function mockSearch(query: string): Promise<MentionOption[]> {
  const q = query.trim().toLowerCase()
  const filtered = !q
    ? MOCK_OPTIONS.slice(0, 6)
    : MOCK_OPTIONS.filter(
        (o) =>
          o.label.toLowerCase().includes(q) ||
          o.id.toLowerCase().includes(q) ||
          o.type.includes(q),
      )
  return Promise.resolve(filtered)
}

export function InlineAtPrototypePage() {
  const { t } = useTranslation()
  const [doc, setDoc] = useState<InlineDoc | null>(null)
  const [viewport, setViewport] = useState<"auto" | "desktop" | "mobile">("auto")
  const search = useCallback((q: string) => mockSearch(q), [])

  const forceMobile =
    viewport === "mobile" ? true : viewport === "desktop" ? false : undefined

  return (
    <PageLayout
      title={t("chat:mention.protoTitle", "行内 @ 原型")}
      subtitle={t(
        "chat:mention.protoDesc",
        "独立编辑器原型：评估行内引用交互；不写入生产 ChatComposer。",
      )}
      maxWidth="lg"
    >
      <div className="mx-auto flex w-full max-w-3xl flex-col gap-4 p-4 pb-24 md:pb-8">
        <div className="flex flex-wrap gap-2">
          {(
            [
              ["auto", t("chat:mention.protoViewportAuto", "跟随窗口")],
              ["desktop", t("chat:mention.protoViewportDesktop", "桌面浮层")],
              ["mobile", t("chat:mention.protoViewportMobile", "移动面板")],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              className={`rounded-md border px-3 py-1.5 text-sm ${
                viewport === id
                  ? "border-border bg-muted"
                  : "border-border bg-background"
              }`}
              onClick={() => setViewport(id)}
              data-viewport-toggle={id}
            >
              {label}
            </button>
          ))}
        </div>

        <InlineAtEditor
          search={search}
          forceMobile={forceMobile}
          onDocChange={setDoc}
        />

        <section className="rounded-md border border-border p-3">
          <h2 className="mb-2 text-sm font-medium">
            {t("chat:mention.protoHistory", "发送后历史预览（行内标签）")}
          </h2>
          <div
            className="rounded-md bg-muted-30 px-3 py-2 text-sm leading-relaxed"
            data-inline-at-history
          >
            {!doc || toPlainText(doc).length === 0 ? (
              <span className="text-muted-foreground">
                {t("chat:mention.protoHistoryEmpty", "输入并插入引用后在此预览")}
              </span>
            ) : (
              toContentNodes(doc).map((n, i) =>
                n.kind === "text" ? (
                  <span key={i}>{n.text}</span>
                ) : (
                  <span
                    key={i}
                    className="mx-0.5 inline-flex items-center rounded-md border border-border bg-background px-1.5 py-0.5 text-xs"
                  >
                    <span className="mr-1 text-muted-foreground">
                      {t(`chat:mention.${n.entityType}`, n.entityType)}
                    </span>
                    {n.labelSnapshot}
                  </span>
                ),
              )
            )}
          </div>
        </section>

        <p className="text-xs text-muted-foreground">
          {t(
            "chat:mention.protoTradeoffLink",
            "原型取舍文档已归档；生产聊天使用 TipTap Mention。",
          )}
        </p>
      </div>
    </PageLayout>
  )
}

export default InlineAtPrototypePage
