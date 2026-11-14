/**
 * Standalone inline-@ prototype editor (plan iter B).
 *
 * Not wired into production ChatComposer. Uses segmented textareas + atomic
 * chips (no hand-rolled contentEditable). Desktop = floating listbox;
 * mobile = bottom Sheet. Same entity may appear more than once.
 */
import { useEffect, useMemo, useRef, useState } from "react"
import { useTranslation } from "react-i18next"
import { AtSign, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Sheet, SheetContent, SheetDescription, SheetTitle } from "@/components/ui/sheet"
import { useIsMobile } from "@/hooks/useMobile"
import { cn } from "@/lib/utils"
import { MAX_REFERENCES, type MentionOption } from "@/lib/api/mentions"
import {
  CapError,
  backspace,
  consumeAtTrigger,
  deleteReferenceAt,
  detectAtTrigger,
  emptyDoc,
  insertReference,
  setTextNode,
  toLegacyUniqueReferences,
  toPlainText,
  type Caret,
  type EntityType,
  type InlineDoc,
} from "./model"

export interface InlineAtEditorProps {
  /** Injected search — prototype page supplies a mock or live API. */
  search: (query: string) => Promise<MentionOption[]>
  className?: string
  /** Viewport force for tests / side-by-side demos. */
  forceMobile?: boolean
  onDocChange?: (doc: InlineDoc) => void
}

export function InlineAtEditor({
  search,
  className,
  forceMobile,
  onDocChange,
}: InlineAtEditorProps) {
  const { t } = useTranslation()
  const isMobileHook = useIsMobile()
  const isMobile = forceMobile ?? isMobileHook

  const [doc, setDoc] = useState<InlineDoc>(() => emptyDoc())
  const [caret, setCaret] = useState<Caret>({ nodeIndex: 0, offset: 0 })
  const [mentionQuery, setMentionQuery] = useState<string | null>(null)
  const [triggerStart, setTriggerStart] = useState(0)
  const [options, setOptions] = useState<MentionOption[]>([])
  const [active, setActive] = useState(0)
  const [loading, setLoading] = useState(false)
  const [capMessage, setCapMessage] = useState<string | null>(null)
  const composing = useRef(false)
  const inputRefs = useRef<Map<number, HTMLTextAreaElement>>(new Map())

  const plain = useMemo(() => toPlainText(doc), [doc])
  const legacy = useMemo(() => toLegacyUniqueReferences(doc), [doc])

  useEffect(() => {
    onDocChange?.(doc)
  }, [doc, onDocChange])

  useEffect(() => {
    if (mentionQuery === null) return
    let cancelled = false
    setLoading(true)
    const timer = setTimeout(async () => {
      try {
        const opts = await search(mentionQuery)
        if (!cancelled) {
          setOptions(opts)
          setActive(0)
        }
      } catch {
        if (!cancelled) setOptions([])
      } finally {
        if (!cancelled) setLoading(false)
      }
    }, 120)
    return () => {
      // Stale-request guard: ignore results after a newer query starts.
      cancelled = true
      clearTimeout(timer)
    }
  }, [mentionQuery, search])

  const closeMention = () => {
    setMentionQuery(null)
    setOptions([])
  }

  const focusCaret = (next: Caret) => {
    setCaret(next)
    requestAnimationFrame(() => {
      const el = inputRefs.current.get(next.nodeIndex)
      el?.focus()
      try {
        el?.setSelectionRange(next.offset, next.offset)
      } catch {
        // best-effort
      }
    })
  }

  const applyDoc = (next: InlineDoc, nextCaret: Caret) => {
    setDoc(next)
    focusCaret(nextCaret)
  }

  const openFromToolbar = () => {
    const c = caret
    const node = doc.nodes[c.nodeIndex]
    if (node?.kind !== "text") return
    const text = node.text
    const withAt = text.slice(0, c.offset) + "@" + text.slice(c.offset)
    try {
      const r = setTextNode(doc, c.nodeIndex, withAt, c.offset + 1)
      applyDoc(r.doc, r.caret)
      setTriggerStart(c.offset)
      setMentionQuery("")
      setCapMessage(null)
    } catch (e) {
      if (e instanceof CapError) setCapMessage(e.message)
    }
  }

  const onTextChange = (nodeIndex: number, value: string, sel: number) => {
    if (composing.current) {
      // Keep DOM text in sync but do not open the picker mid-composition.
      try {
        const r = setTextNode(doc, nodeIndex, value, sel)
        setDoc(r.doc)
        setCaret(r.caret)
      } catch (e) {
        if (e instanceof CapError) setCapMessage(e.message)
      }
      return
    }
    try {
      const r = setTextNode(doc, nodeIndex, value, sel)
      setDoc(r.doc)
      setCaret(r.caret)
      setCapMessage(null)
      const before = value.slice(0, sel)
      const trig = detectAtTrigger(before)
      if (trig) {
        setTriggerStart(trig.start)
        setMentionQuery(trig.query)
      } else {
        closeMention()
      }
    } catch (e) {
      if (e instanceof CapError) setCapMessage(e.message)
    }
  }

  const selectOption = (opt: MentionOption) => {
    if (mentionQuery === null) return
    try {
      const stripped = consumeAtTrigger(doc, caret, triggerStart)
      const inserted = insertReference(stripped.doc, stripped.caret, {
        entityType: opt.type as EntityType,
        id: opt.id,
        labelSnapshot: opt.label,
      })
      applyDoc(inserted.doc, inserted.caret)
      closeMention()
      setCapMessage(null)
    } catch (e) {
      if (e instanceof CapError) {
        setCapMessage(e.message)
        closeMention()
      }
    }
  }

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>, nodeIndex: number) => {
    const ime =
      composing.current ||
      e.nativeEvent.isComposing ||
      (e.nativeEvent as KeyboardEvent).keyCode === 229
    if (ime) return
    if (mentionQuery !== null) {
      if (e.key === "ArrowDown") {
        e.preventDefault()
        setActive((i) => Math.min(i + 1, Math.max(options.length - 1, 0)))
        return
      }
      if (e.key === "ArrowUp") {
        e.preventDefault()
        setActive((i) => Math.max(i - 1, 0))
        return
      }
      if ((e.key === "Enter" || e.key === "Tab") && options[active]) {
        e.preventDefault()
        selectOption(options[active])
        return
      }
      if (e.key === "Escape") {
        e.preventDefault()
        closeMention()
        return
      }
    }
    if (e.key === "Backspace") {
      const el = e.currentTarget
      if (el.selectionStart === 0 && el.selectionEnd === 0) {
        const result = backspace(doc, { nodeIndex, offset: 0 })
        if (result) {
          e.preventDefault()
          applyDoc(result.doc, result.caret)
        }
      }
    }
  }

  const pickerBody = (
    <div
      role="listbox"
      id="inline-at-listbox"
      aria-label={t("chat:mention.title", "选择要引用的对象")}
      aria-busy={loading || undefined}
      aria-activedescendant={options[active] ? `inline-at-opt-${active}` : undefined}
      data-inline-at-picker
      className={cn(
        "overflow-auto rounded-md border border-border bg-popover text-sm shadow-lg",
        isMobile ? "max-h-[50vh]" : "absolute bottom-full left-0 right-0 z-[200] mb-1 max-h-56",
      )}
    >
      {loading ? (
        <div className="px-3 py-2 text-muted-foreground" role="status">
          {t("chat:mention.searching", "搜索中…")}
        </div>
      ) : options.length === 0 ? (
        <div className="px-3 py-2 text-muted-foreground" role="status">
          {t("chat:mention.noResults", "没有匹配的对象")}
        </div>
      ) : (
        options.map((opt, i) => (
          <button
            key={`${opt.type}:${opt.id}`}
            id={`inline-at-opt-${i}`}
            type="button"
            role="option"
            aria-selected={i === active}
            aria-label={[
              t(`chat:mention.${opt.type}`, opt.type),
              opt.label,
              opt.duplicateName ? opt.shortId || opt.id.slice(0, 8) : "",
              opt.hint || "",
            ]
              .filter(Boolean)
              .join(" ")}
            className={cn(
              "flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-muted",
              i === active && "bg-muted",
            )}
            onMouseEnter={() => setActive(i)}
            onClick={() => selectOption(opt)}
          >
            <span className="shrink-0 text-muted-foreground">
              {t(`chat:mention.${opt.type}`, opt.type)}
            </span>
            <span className="min-w-0 flex-1">
              <span className="block truncate">{opt.label}</span>
              {(opt.duplicateName || opt.hint) && (
                <span className="mt-0.5 block truncate text-xs text-muted-foreground">
                  {[opt.shortId || opt.id.slice(0, 8), opt.hint].filter(Boolean).join(" · ")}
                </span>
              )}
            </span>
            {!opt.duplicateName && !opt.hint && (
              <span className="shrink-0 font-mono text-xs text-muted-foreground">
                {opt.shortId || opt.id.slice(0, 8)}
              </span>
            )}
          </button>
        ))
      )}
    </div>
  )

  return (
    <div className={cn("flex flex-col gap-2", className)} data-inline-at-prototype>
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs text-muted-foreground">
          {t(
            "chat:mention.protoHint",
            "行内引用原型：@ 或工具栏插入；同一对象可出现多次；芯片整块删除。",
          )}
        </p>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={openFromToolbar}
          aria-label={t("chat:mention.insertAt", "插入 @ 引用")}
        >
          <AtSign className="h-4 w-4" />
          <span className="ml-1">@</span>
        </Button>
      </div>

      <div className="relative">
        {!isMobile && mentionQuery !== null && pickerBody}

        <div
          className="flex min-h-[3rem] flex-wrap items-center gap-1 rounded-md border border-border bg-background px-2 py-1.5"
          data-inline-at-surface
        >
          {doc.nodes.map((node, idx) => {
            if (node.kind === "reference") {
              return (
                <span
                  key={`ref-${idx}-${node.id}`}
                  className="inline-flex items-center gap-1 rounded-md border border-border bg-muted-30 px-1.5 py-0.5 text-xs"
                  data-inline-at-chip
                  data-entity-type={node.entityType}
                  data-entity-id={node.id}
                >
                  <span className="text-muted-foreground">
                    {t(`chat:mention.${node.entityType}`, node.entityType)}
                  </span>
                  <span className="max-w-[10rem] truncate font-medium">{node.labelSnapshot}</span>
                  <button
                    type="button"
                    className="rounded-sm p-0.5 text-muted-foreground hover:text-foreground"
                    aria-label={t("chat:mention.remove", "移除引用")}
                    onClick={() => {
                      const r = deleteReferenceAt(doc, idx)
                      applyDoc(r.doc, r.caret)
                    }}
                  >
                    <X className="h-3 w-3" />
                  </button>
                </span>
              )
            }
            return (
              <textarea
                key={`text-${idx}`}
                ref={(el) => {
                  if (el) inputRefs.current.set(idx, el)
                  else inputRefs.current.delete(idx)
                }}
                rows={1}
                value={node.text}
                aria-label={t("chat:mention.protoText", "消息文本")}
                className="min-w-[2ch] flex-1 resize-none border-0 bg-transparent p-0 text-sm outline-none focus-visible:ring-0"
                style={{ width: `${Math.max(2, node.text.length + 1)}ch` }}
                onChange={(e) =>
                  onTextChange(idx, e.target.value, e.target.selectionStart ?? e.target.value.length)
                }
                onSelect={(e) => {
                  const el = e.currentTarget
                  setCaret({ nodeIndex: idx, offset: el.selectionStart ?? 0 })
                }}
                onKeyDown={(e) => onKeyDown(e, idx)}
                onCompositionStart={() => {
                  composing.current = true
                }}
                onCompositionEnd={(e) => {
                  composing.current = false
                  const el = e.currentTarget
                  onTextChange(idx, el.value, el.selectionStart ?? el.value.length)
                }}
              />
            )
          })}
        </div>
      </div>

      {isMobile && (
        <Sheet open={mentionQuery !== null} onOpenChange={(open) => !open && closeMention()}>
          <SheetContent side="bottom" className="z-[110] max-h-[70vh] overflow-hidden p-0">
            <SheetTitle className="sr-only">
              {t("chat:mention.title", "选择要引用的对象")}
            </SheetTitle>
            <SheetDescription className="sr-only">
              {t("chat:mention.protoMobile", "移动端引用选择面板")}
            </SheetDescription>
            {pickerBody}
          </SheetContent>
        </Sheet>
      )}

      {capMessage && (
        <p className="text-xs text-error" role="alert" data-inline-at-cap>
          {capMessage}
        </p>
      )}

      <div className="grid gap-1 rounded-md border border-border bg-muted-30 p-2 text-xs">
        <div>
          <span className="text-muted-foreground">
            {t("chat:mention.protoPlain", "明文")}:{" "}
          </span>
          <code data-inline-at-plain>{plain || "—"}</code>
        </div>
        <div>
          <span className="text-muted-foreground">
            {t("chat:mention.protoLegacy", "旧芯片投影")}:{" "}
          </span>
          <code data-inline-at-legacy>
            {legacy.length === 0
              ? "—"
              : legacy.map((r) => `${r.type}:${r.id}`).join(", ")}
          </code>
          <span className="ml-2 text-muted-foreground">
            ({legacy.length}/{MAX_REFERENCES})
          </span>
        </div>
      </div>
    </div>
  )
}
