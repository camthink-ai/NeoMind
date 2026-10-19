import React, { useEffect, useRef, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'
import type { Components } from 'react-markdown'
import { useTranslation } from 'react-i18next'
import { WrapText, ChevronDown } from 'lucide-react'
import { cn } from "@/lib/utils"
import { ErrorBoundary } from "@/components/shared/ErrorBoundary"
import { ZoomableImage } from "@/components/shared/ZoomableImage"
import { textCode, textNano } from "@/design-system/tokens/typography"
import { Copy, Check } from "@/design-system/icons"
import { copyToClipboard } from '@/lib/clipboard'
import { resolveImageSrc } from '@/lib/imageUtils'

interface MarkdownMessageProps {
  content: string
  className?: string
  variant?: 'user' | 'assistant'
  /** Renders a caret after the markdown and is part of the memo key. */
  isStreaming?: boolean
}

/**
 * Remove duplicated content when the same text appears twice in one message
 * (e.g. model repetition or backend sending same chunk twice).
 */
function dedupeRepeatedContent(content: string): string {
  const s = (content || '').trim()
  if (s.length < 2) return content
  const half = Math.floor(s.length / 2)
  const first = s.slice(0, half)
  const second = s.slice(half)
  if (first === second) return first
  return content
}

/**
 * Code block with a language label, wrap toggle, copy button, and an
 * expand/collapse footer for long snippets, replacing the bare <pre>.
 *
 * The inner <code> keeps the hljs highlight spans produced by rehype-highlight;
 * this component only wraps it with the chrome. Copying reads
 * `pre.innerText` so it captures the raw source regardless of highlight spans.
 * Surface colors come from the variant-scoped --code-* CSS variables, which
 * is what makes user-message code readable on its dark/light bubble.
 */
function CodeBlock({ children, className: _className, ...props }: React.ComponentProps<'pre'>) {
  const preRef = useRef<HTMLPreElement>(null)
  const [copied, setCopied] = useState(false)
  const [wrapped, setWrapped] = useState(false)
  const [expanded, setExpanded] = useState(false)
  const [lineCount, setLineCount] = useState(0)
  const { t } = useTranslation("chat")

  // children is the inner <code> element; pull the language from its className.
  const child: any = Array.isArray(children) ? children[0] : children
  const codeClass: string = child?.props?.className ?? ""
  const langMatch = /language-([\w+-]+)/.exec(codeClass)
  const lang = langMatch ? langMatch[1] : ""
  const isLong = lineCount > 15

  useEffect(() => {
    const el = preRef.current
    if (!el) return
    const text = el.textContent ?? ""
    setLineCount(text.split("\n").length)
  }, [children])

  const handleCopy = async () => {
    const text = preRef.current?.innerText ?? preRef.current?.textContent ?? ""
    try {
      await copyToClipboard(text)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 2000)
    } catch {
      /* clipboard blocked (permissions / non-secure context) — silent */
    }
  }

  const controlClass = cn(
    "inline-flex h-7 w-7 items-center justify-center rounded text-muted-foreground",
    "transition-colors hover:bg-[var(--code-control-hover)] hover:text-foreground",
    "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
  )

  return (
    <div className="code-block group relative my-3 overflow-hidden rounded-xl border border-[var(--code-border)] bg-[var(--code-bg)]">
      <div className="flex items-center justify-between gap-2 border-b border-[var(--code-border)] px-3 py-1.5">
        <span className={cn(textNano, "font-mono uppercase tracking-wider text-muted-foreground")}>
          {lang || "text"}
        </span>
        <div className="flex items-center gap-0.5">
          <button
            type="button"
            onClick={() => setWrapped(v => !v)}
            aria-label={wrapped ? t("code.unwrap") : t("code.wrap")}
            aria-pressed={wrapped}
            title={wrapped ? t("code.unwrap") : t("code.wrap")}
            className={controlClass}
          >
            <WrapText className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
          <button
            type="button"
            onClick={handleCopy}
            className={controlClass}
            aria-label={copied ? t("code.copied") : t("code.copy")}
            title={copied ? t("code.copied") : t("code.copy")}
          >
            {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
          </button>
        </div>
      </div>
      <pre
        ref={preRef}
        className={cn(
          "m-0 px-3 pb-3 pt-2 !bg-transparent !text-foreground",
          wrapped ? "whitespace-pre-wrap break-words" : "whitespace-pre overflow-x-auto",
          !expanded && isLong && "max-h-[420px] overflow-y-auto"
        )}
        {...props}
      >
        {children}
      </pre>
      {isLong && (
        <button
          type="button"
          onClick={() => setExpanded(v => !v)}
          aria-expanded={expanded}
          className="flex w-full items-center justify-center gap-1 border-t border-[var(--code-border)] py-1.5 text-nano text-muted-foreground transition-colors hover:bg-[var(--code-control-hover)] hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          {expanded ? t("code.collapse") : t("code.expand")}
          <ChevronDown
            className={cn("h-3.5 w-3.5 transition-transform duration-normal", expanded && "rotate-180")}
            aria-hidden="true"
          />
        </button>
      )}
    </div>
  )
}

// Static component overrides — hoisted to module scope to avoid re-allocating
// a new object (and new closure functions) on every render / streaming chunk.
const MARKDOWN_COMPONENTS: Components = {
  pre: ({ node: _node, className, children, ...props }) => (
    <CodeBlock className={className} {...(props as any)}>
      {children}
    </CodeBlock>
  ),
  code: ({ node: _node, className, children, ...props }) => {
    const isBlock = !!className
    if (!isBlock) {
      return (
        <code
          className={cn(
            "rounded bg-[var(--inline-code-bg)] px-1 py-0.5",
            textCode,
            "font-mono text-foreground",
            className
          )}
          {...(props as any)}
        >
          {children}
        </code>
      )
    }
    return (
      <code className={cn("text-foreground", className)} {...(props as any)}>
        {children}
      </code>
    )
  },
  a: ({ node: _node, className, children, href, ...props }) => (
    <a
      // Inherit color from the surrounding text so links stay readable on
      // every bubble background. In light theme `--primary` is near-white,
      // which collides with the user bubble's `--msg-user-bg` (also
      // near-white) — hardcoding `text-primary` here made user-message URLs
      // invisible. Rely on underline for link affordance instead of color.
      className={cn("text-inherit underline underline-offset-2 hover:opacity-80", className)}
      href={href as string}
      target="_blank"
      rel="noopener noreferrer"
      {...(props as any)}
    >
      {children}
    </a>
  ),
  // Backend image URLs arrive as relative /api/images/... paths. On the web
  // build (same origin) they resolve as-is; in Tauri the page origin is not
  // the API origin, so rewrite them through resolveImageSrc (which prepends
  // the server origin). Everything else passes through untouched.
  img: ({ node: _node, src, alt, ...props }) => {
    const resolved = typeof src === 'string' ? resolveImageSrc(src) : null
    if (!resolved) {
      return <img src={src as string} alt={alt ?? ''} {...(props as React.ComponentProps<'img'>)} />
    }
    return <ZoomableImage src={resolved} alt={alt ?? ''} />
  },
  // Tables scroll horizontally WITHIN the message instead of pushing the
  // whole chat panel into horizontal scroll (float chat is only 380-400px).
  table: ({ node: _node, children, ...props }) => (
    <div className="my-3 overflow-x-auto">
      <table
        className="w-full [&_th]:whitespace-nowrap [&_td]:align-top"
        {...(props as any)}
      >
        {children}
      </table>
    </div>
  ),
}

/**
 * Markdown message renderer with support for:
 * - GitHub Flavored Markdown (GFM) via remark-gfm
 * - Code blocks with syntax highlighting (rehype-highlight / hljs)
 * - Copy button + language label on code blocks
 * - Tables, lists, links, images
 * - Styled for chat interface
 *
 * Memoized: only re-renders when content/className/variant/isStreaming change.
 * Component overrides are module-scope (no per-render allocation).
 * Wrapped in ErrorBoundary so malformed markdown can't crash the chat.
 */
export const MarkdownMessage = React.memo<MarkdownMessageProps>(
  ({ content, className, variant = 'assistant', isStreaming = false }) => {
  const displayContent = dedupeRepeatedContent(content)

  return (
    <div className={cn("relative", className)}>
      <div
        className={cn(
          // Base prose classes — use prose for structure, chat-specific
          // reading sizes so long answers are not set in dashboard density.
          "prose max-w-none", "text-chat-body sm:text-chat-body-lg",
          // Text wrapping
          "break-words overflow-wrap-anywhere",
          // Keep paragraphs on the same column measure as the process card.
          // A per-paragraph max-w-prose here narrowed only the answer text,
          // making the full-width execution card look head-heavy above it.
          "prose-p:leading-relaxed prose-p:my-2",
          "prose-headings:font-semibold prose-headings:mt-5 prose-headings:mb-2.5",
          "prose-h1:text-lg prose-h1:leading-snug prose-h2:text-base prose-h2:leading-snug prose-h3:text-chat-body-lg prose-h3:leading-snug",
          // Links inherit text color (see MARKDOWN_COMPONENTS.a) so they stay
          // readable on both user and assistant bubble backgrounds. Underline
          // alone provides the link affordance — do NOT set a prose-a color
          // here, it would override the inherit and re-introduce the
          // light-theme invisibility bug on user bubbles.
          "prose-a:text-inherit prose-a:underline prose-a:underline-offset-2",
          "prose-strong:font-semibold",
          "prose-code:rounded prose-code:text-code prose-code:font-mono",
          "prose-code:break-all prose-code:whitespace-pre-wrap",
          // Code block chrome (border, bg, padding, language bar, copy button)
          // is owned by CodeBlock — only keep overflow + inline-code reset here.
          "prose-pre:overflow-x-auto prose-pre:max-w-full",
          "prose-pre:prose-code:bg-transparent prose-pre:prose-code:p-0 prose-pre:prose-code:text-foreground",
          "prose-blockquote:border-l-2 prose-blockquote:border-muted-foreground prose-blockquote:bg-muted-30 prose-blockquote:pl-3 prose-blockquote:pr-3 prose-blockquote:py-1 prose-blockquote:rounded-r-md prose-blockquote:italic",
          "prose-ul:my-1 prose-ul:pl-4 prose-ul:list-disc",
          "prose-ol:my-1 prose-ol:pl-4 prose-ol:list-decimal",
          "prose-li:my-1 prose-li:marker:text-muted-foreground",
          "prose-table:my-3 prose-table:text-body",
          "prose-th:px-2 prose-th:py-1.5 prose-th:border-b prose-th:border-border prose-th:bg-muted-50 prose-th:font-semibold",
          "prose-td:px-2 prose-td:py-1.5 prose-td:border-b prose-td:border-border",
          "prose-hr:my-2 prose-hr:border-border",
          "text-inherit"
          // Removed max height limit - messages now fully expand
        )}
        data-variant={variant}
      >
        <ErrorBoundary resetKey={displayContent}>
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            rehypePlugins={[[rehypeHighlight, { detect: true, ignoreMissing: true }]]}
            components={MARKDOWN_COMPONENTS}
          >
            {displayContent}
          </ReactMarkdown>
          {isStreaming && (
            <span
              aria-hidden="true"
              className="ml-0.5 inline-block h-[1em] w-[2px] animate-blink bg-current align-text-bottom"
            />
          )}
        </ErrorBoundary>
      </div>
    </div>
  )
},
  (prev, next) =>
    prev.content === next.content &&
    prev.className === next.className &&
    prev.variant === next.variant &&
    prev.isStreaming === next.isStreaming
)

MarkdownMessage.displayName = "MarkdownMessage"
