/**
 * Production TipTap inline-@ editor (plan §12 C).
 *
 * Owns the ProseMirror surface + mention atoms. Suggestion search UI stays in
 * ChatComposer (listbox / type filters / pagination / IME guards).
 */
import { useCallback, useEffect, useMemo, useRef } from "react"
import { EditorContent, useEditor, type Editor } from "@tiptap/react"
import StarterKit from "@tiptap/starter-kit"
import Placeholder from "@tiptap/extension-placeholder"
import type { SuggestionKeyDownProps, SuggestionProps } from "@tiptap/suggestion"
import { cn } from "@/lib/utils"
import type { ChatContentNode, ChatReference } from "@/types"
import type { InlineDoc } from "@/lib/chat/inlineDoc"
import { emptyDoc } from "@/lib/chat/inlineDoc"
import {
  EntityMention,
  type EntityMentionAttrs,
  type EntityMentionItem,
} from "./entityMention"
import {
  contentNodesToInlineDoc,
  inlineDocToChatPayload,
  inlineDocToTipTapJson,
  tipTapJsonToInlineDoc,
} from "./docBridge"
import { mentionChipHostSelectors } from "@/components/chat/mentionChipStyles"

export type MentionSuggestionApi = {
  query: string
  command: (item: EntityMentionItem) => void
}

export interface InlineMentionEditorProps {
  value: string
  contentNodes?: ChatContentNode[]
  placeholder?: string
  disabled?: boolean
  className?: string
  minHeight?: number
  maxHeight?: number
  listboxId?: string
  mentionOpen?: boolean
  invalidRefs?: Record<string, string>
  /** Focus / host handle — ProseMirror root is assigned here. */
  editorHostRef?: React.RefObject<HTMLElement | null>
  onPayloadChange: (payload: {
    message: string
    references: ChatReference[]
    contentNodes: ChatContentNode[]
    doc: InlineDoc
  }) => void
  onSuggestionStart: (api: MentionSuggestionApi) => void
  onSuggestionUpdate: (api: MentionSuggestionApi) => void
  /**
   * TipTap suggestion exited. `editorFocused` distinguishes blur (mobile Sheet
   * / option tap) from an in-editor dismiss (deleted `@`, Escape).
   */
  onSuggestionExit: (meta?: { editorFocused: boolean }) => void
  onSuggestionKeyDown?: (props: SuggestionKeyDownProps) => boolean
  /** Return true to claim the key (e.g. Enter-to-send). */
  onKeyDown?: (event: KeyboardEvent) => boolean | void
  onFocus?: () => void
  onBlur?: () => void
  /** Bump to insert `@` at the caret (toolbar). */
  openMentionRequest?: number
  /** TipTap composition lifecycle — host uses this for IME Enter/mention guards. */
  onCompositionChange?: (composing: boolean) => void
  /** Optional TipTap handle (tests / advanced hosts). */
  editorRef?: React.MutableRefObject<Editor | null>
}

function contentNodesKey(nodes: ChatContentNode[] | undefined): string {
  return nodes?.length ? JSON.stringify(nodes) : ""
}

function toSuggestionApi(
  props: SuggestionProps<EntityMentionItem, EntityMentionAttrs>,
): MentionSuggestionApi {
  return {
    query: props.query,
    command: (item) => {
      props.command({
        id: item.id,
        label: item.label,
        entityType: item.entityType,
        typeLabel: item.typeLabel ?? null,
      })
    },
  }
}

export function InlineMentionEditor({
  value,
  contentNodes,
  placeholder,
  disabled = false,
  className,
  minHeight = 44,
  maxHeight = 160,
  listboxId,
  mentionOpen = false,
  invalidRefs = {},
  editorHostRef,
  onPayloadChange,
  onSuggestionStart,
  onSuggestionUpdate,
  onSuggestionExit,
  onSuggestionKeyDown,
  onKeyDown,
  onFocus,
  onBlur,
  openMentionRequest = 0,
  onCompositionChange,
  editorRef,
}: InlineMentionEditorProps) {
  const suggestionKeyDownRef = useRef(onSuggestionKeyDown)
  const onSuggestionStartRef = useRef(onSuggestionStart)
  const onSuggestionUpdateRef = useRef(onSuggestionUpdate)
  const onSuggestionExitRef = useRef(onSuggestionExit)
  const onPayloadChangeRef = useRef(onPayloadChange)
  const onKeyDownRef = useRef(onKeyDown)
  const onCompositionChangeRef = useRef(onCompositionChange)
  const skipEmit = useRef(false)
  const isComposing = useRef(false)
  /** Plain text at compositionstart — used to detect a real IME commit. */
  const composeStartPlainRef = useRef("")
  const lastPlain = useRef(value)
  /**
   * External value/contentNodes update that arrived while an IME composition
   * was active. setContent mid-composition aborts CJK input, so the sync is
   * DEFERRED (not dropped): applied once on compositionend, after the
   * composition's own flush. Without this a draft restore / message reuse
   * racing an active IME session was silently lost forever.
   */
  const pendingSyncRef = useRef<{
    value: string
    contentNodes?: ChatContentNode[]
  } | null>(null)
  // Empty until the first sync/emit — initial contentNodes (history fill) must
  // still enter via setContent; only post-emit echoes share this key.
  const lastNodesKey = useRef("")
  const lastOpenMention = useRef(openMentionRequest)
  const hostDivRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    suggestionKeyDownRef.current = onSuggestionKeyDown
    onSuggestionStartRef.current = onSuggestionStart
    onSuggestionUpdateRef.current = onSuggestionUpdate
    onSuggestionExitRef.current = onSuggestionExit
    onPayloadChangeRef.current = onPayloadChange
    onKeyDownRef.current = onKeyDown
    onCompositionChangeRef.current = onCompositionChange
  })

  const mentionExtension = useMemo(
    () =>
      EntityMention.configure({
        deleteTriggerWithBackspace: true,
        HTMLAttributes: { class: "nm-mention" },
        renderText({ node }) {
          return `@${node.attrs.label ?? node.attrs.id ?? ""}`
        },
        suggestion: {
          char: "@",
          allowSpaces: false,
          // Space / punctuation / CJK ideographic space — not word chars
          // (blocks a@b.com). Start-of-block is allowed by TipTap match.
          allowedPrefixes: [" ", "\n", "(", "[", "{", "\u3000", '"', "'"],
          items: async () =>
            // Host owns the real option list; a non-empty stub keeps the
            // suggestion lifecycle alive for onStart/onUpdate/onExit.
            [{ id: "__host__", label: "", entityType: "device" as const }],
          render: () => ({
            onStart: (props) => {
              onSuggestionStartRef.current(toSuggestionApi(props))
            },
            onUpdate: (props) => {
              onSuggestionUpdateRef.current(toSuggestionApi(props))
            },
            onExit: (props) => {
              onSuggestionExitRef.current({
                editorFocused: props.editor.view.hasFocus(),
              })
            },
            onKeyDown: (props) => {
              if (suggestionKeyDownRef.current) {
                return suggestionKeyDownRef.current(props)
              }
              return false
            },
          }),
          command: ({ editor, range, props }) => {
            const attrs = props as EntityMentionAttrs
            if (!attrs.id || attrs.id === "__host__") return
            editor
              .chain()
              .focus()
              .insertContentAt(range, [
                {
                  type: "mention",
                  attrs: {
                    id: attrs.id,
                    label: attrs.label,
                    entityType: attrs.entityType,
                    typeLabel: attrs.typeLabel ?? null,
                    mentionSuggestionChar: "@",
                  },
                },
                { type: "text", text: " " },
              ])
              .run()
          },
        },
      }),
    [],
  )

  const editor = useEditor({
    immediatelyRender: false,
    extensions: [
      StarterKit.configure({
        heading: false,
        bold: false,
        italic: false,
        strike: false,
        code: false,
        codeBlock: false,
        blockquote: false,
        horizontalRule: false,
        bulletList: false,
        orderedList: false,
        listItem: false,
        dropcursor: false,
        gapcursor: false,
      }),
      Placeholder.configure({
        placeholder: placeholder ?? "",
        emptyEditorClass: "is-editor-empty",
      }),
      mentionExtension,
    ],
    editable: !disabled,
    editorProps: {
      attributes: {
        role: "textbox",
        "aria-multiline": "true",
        class: cn(
          "nm-inline-mention-editor outline-none max-w-full break-words",
          "text-base leading-5 sm:text-sm caret-foreground",
        ),
      },
      handleKeyDown: (_view, event) => {
        return onKeyDownRef.current?.(event) === true
      },
    },
    onUpdate: ({ editor: ed }) => {
      // Mid-IME: ProseMirror may fire updates for intermediate marks; do not
      // push those to the controlled parent (echo setContent kills composition).
      if (skipEmit.current || isComposing.current) return
      const doc = tipTapJsonToInlineDoc(ed.getJSON())
      const payload = inlineDocToChatPayload(doc)
      lastPlain.current = payload.message
      // Mark this snapshot as already applied so the controlled contentNodes
      // prop does not bounce back through setContent on every keystroke.
      lastNodesKey.current = contentNodesKey(payload.contentNodes)
      onPayloadChangeRef.current({ ...payload, doc })
    },
    onFocus: () => onFocus?.(),
    onBlur: () => onBlur?.(),
  })

  useEffect(() => {
    if (!editorRef) return
    editorRef.current = editor
    return () => {
      editorRef.current = null
    }
  }, [editor, editorRef])

  // aria-controls / expanded follow the host listbox.
  useEffect(() => {
    if (!editor) return
    const dom = editor.view.dom
    if (listboxId && mentionOpen) {
      dom.setAttribute("aria-controls", listboxId)
      dom.setAttribute("aria-expanded", "true")
      dom.setAttribute("aria-autocomplete", "list")
    } else {
      dom.removeAttribute("aria-controls")
      dom.setAttribute("aria-expanded", "false")
      if (listboxId) dom.setAttribute("aria-autocomplete", "list")
    }
  }, [editor, listboxId, mentionOpen])

  // CJK/Japanese IME: never rewrite the document while composing.
  useEffect(() => {
    if (!editor) return
    const dom = editor.view.dom
    const onStart = () => {
      isComposing.current = true
      composeStartPlainRef.current = lastPlain.current
      onCompositionChangeRef.current?.(true)
    }
    const onEnd = () => {
      isComposing.current = false
      onCompositionChangeRef.current?.(false)
      let flushedMessage: string | null = null
      if (!skipEmit.current) {
        const doc = tipTapJsonToInlineDoc(editor.getJSON())
        const payload = inlineDocToChatPayload(doc)
        flushedMessage = payload.message
        lastPlain.current = payload.message
        lastNodesKey.current = contentNodesKey(payload.contentNodes)
        onPayloadChangeRef.current({ ...payload, doc })
      }
      // Replay a mid-composition external update only when the IME did not
      // advance the doc (empty/cancelled compose). If the user committed
      // characters, prefer that flush — replaying a parked draft / clear
      // would wipe the just-committed CJK (draft lifecycle switch race).
      const pending = pendingSyncRef.current
      pendingSyncRef.current = null
      if (!pending) return
      const composedAdvanced =
        flushedMessage != null && flushedMessage !== composeStartPlainRef.current
      if (composedAdvanced) return
      applyExternalSyncRef.current(pending.value, pending.contentNodes)
      const doc = tipTapJsonToInlineDoc(editor.getJSON())
      const payload = inlineDocToChatPayload(doc)
      lastPlain.current = payload.message
      lastNodesKey.current = contentNodesKey(payload.contentNodes)
      onPayloadChangeRef.current({ ...payload, doc })
    }
    dom.addEventListener("compositionstart", onStart)
    dom.addEventListener("compositionend", onEnd)
    return () => {
      dom.removeEventListener("compositionstart", onStart)
      dom.removeEventListener("compositionend", onEnd)
    }
  }, [editor])

  // Sync external clear / welcome prompt / draft restore.
  // Own emits update lastPlain/lastNodesKey first so they never re-enter here.
  const applyExternalSync = useCallback(
    (value: string, contentNodes?: ChatContentNode[]) => {
      if (!editor) return
      const nodesKey = contentNodesKey(contentNodes)
      if (contentNodes && contentNodes.length > 0 && nodesKey !== lastNodesKey.current) {
        const fromNodes = contentNodesToInlineDoc(contentNodes)
        const fromPayload = inlineDocToChatPayload(fromNodes)
        skipEmit.current = true
        editor.commands.setContent(inlineDocToTipTapJson(fromNodes), {
          emitUpdate: false,
        })
        skipEmit.current = false
        lastPlain.current = fromPayload.message
        lastNodesKey.current = nodesKey
        return
      }
      if (value !== lastPlain.current) {
        const current = inlineDocToChatPayload(tipTapJsonToInlineDoc(editor.getJSON()))
        if (value !== current.message) {
          skipEmit.current = true
          if (!value.trim()) {
            editor.commands.clearContent(true)
          } else {
            editor.commands.setContent(
              inlineDocToTipTapJson({
                nodes: [{ kind: "text", text: value }],
              }),
              { emitUpdate: false },
            )
          }
          skipEmit.current = false
        }
        lastPlain.current = value
        if (!value) lastNodesKey.current = ""
      }
    },
    [editor],
  )

  // Keep the compositionend handler on the freshest sync fn (the listener
  // effect only re-subscribes when `editor` changes).
  const applyExternalSyncRef = useRef(applyExternalSync)
  useEffect(() => {
    applyExternalSyncRef.current = applyExternalSync
  }, [applyExternalSync])

  useEffect(() => {
    if (!editor) return
    // setContent mid-composition aborts the IME session (CJK input breaks).
    // Defer: stash the latest external props and let compositionend replay.
    if (isComposing.current) {
      pendingSyncRef.current = { value, contentNodes }
      return
    }
    applyExternalSync(value, contentNodes)
  }, [editor, value, contentNodes, applyExternalSync])

  useEffect(() => {
    if (!editor) return
    // Avoid emitUpdate → onPayloadChange → parent rerender loops in tests.
    editor.setEditable(!disabled, false)
  }, [editor, disabled])

  useEffect(() => {
    if (!editor || openMentionRequest === lastOpenMention.current) return
    lastOpenMention.current = openMentionRequest
    if (openMentionRequest <= 0) return
    editor.chain().focus().insertContent("@").run()
  }, [editor, openMentionRequest])

  useEffect(() => {
    const root = hostDivRef.current
    if (!root) return
    root.querySelectorAll<HTMLElement>("span.nm-mention[data-id]").forEach((el) => {
      const id = el.getAttribute("data-id") || ""
      const type = el.getAttribute("data-entity-type") || ""
      const key = `${type}:${id}`
      const invalid = !!invalidRefs[key]
      el.dataset.invalid = invalid ? "true" : "false"
      if (invalid) el.setAttribute("title", invalidRefs[key])
      else el.removeAttribute("title")
    })
  }, [invalidRefs, value, contentNodes, editor])

  useEffect(() => {
    if (!editorHostRef) return
    const assign = (node: HTMLElement | null) => {
      ;(editorHostRef as React.MutableRefObject<HTMLElement | null>).current = node
    }
    assign(hostDivRef.current)
    return () => assign(null)
  }, [editorHostRef, editor])

  if (!editor) return null

  return (
    <div
      ref={hostDivRef}
      data-inline-mention-host
      className={cn(
        "nm-inline-mention-host w-full overflow-y-auto px-4 pb-1 pt-3",
        "[&_.ProseMirror]:min-h-[inherit] [&_.ProseMirror]:outline-none",
        "[&_.ProseMirror_p]:m-0 [&_.ProseMirror_p]:leading-5",
        "[&_.is-editor-empty:first-child::before]:pointer-events-none",
        "[&_.is-editor-empty:first-child::before]:float-left",
        "[&_.is-editor-empty:first-child::before]:h-0",
        "[&_.is-editor-empty:first-child::before]:text-muted-foreground",
        "[&_.is-editor-empty:first-child::before]:content-[attr(data-placeholder)]",
        mentionChipHostSelectors,
        disabled && "opacity-60",
        className,
      )}
      style={{ minHeight: `${minHeight}px`, maxHeight: `${maxHeight}px` }}
      onClick={() => {
        if (!disabled) editor.commands.focus("end")
      }}
    >
      <EditorContent editor={editor} />
    </div>
  )
}

export function emptyMentionPayload() {
  const doc = emptyDoc()
  return { ...inlineDocToChatPayload(doc), doc }
}
