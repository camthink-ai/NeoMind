import React, { useRef, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'
import type { Components } from 'react-markdown'
import { useTranslation } from 'react-i18next'
import { cn } from "@/lib/utils"
import { ErrorBoundary } from "@/components/shared/ErrorBoundary"
import { textCode } from "@/design-system/tokens/typography"
import { Copy, Check } from "@/design-system/icons"
import { copyToClipboard } from '@/lib/clipboard'

interface MarkdownMessageProps {
  content: string
  className?: string
  variant?: 'user' | 'assistant'
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
 * Code block with a language label and a copy button, replacing the bare <pre>.
 *
 * The inner <code> keeps the hljs highlight spans produced by rehype-highlight;
 * this component only wraps it with a header bar (language name + copy button).
 * Copying reads `pre.innerText` so it captures the raw source text regardless of
 * the highlight spans inside. The language is extracted from the <code>
 * element's `className` (`language-xxx`), which react-markdown sets from the
 * fenced code fence info string.
 */
function CodeBlock({ children, ...props }: React.ComponentProps<'pre'>) {
  const preRef = useRef<HTMLPreElement>(null)
  const [copied, setCopied] = useState(false)
  const { t } = useTranslation()

  // children is the inner <code> element; pull the language from its className.
  const child: any = Array.isArray(children) ? children[0] : children
  const codeClass: string = child?.props?.className ?? ""
  const langMatch = /language-([\w+-]+)/.exec(codeClass)
  const lang = langMatch ? langMatch[1] : ""

  const handleCopy = async () => {
    const text = preRef.current?.innerText ?? ""
    try {
      await copyToClipboard(text)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 2000)
    } catch {
      /* clipboard blocked (permissions / non-secure context) — silent */
    }
  }

  return (
    <div className="code-block group relative my-2 overflow-hidden rounded-lg bg-muted">
      <div className="flex items-center justify-between px-3 pt-1.5 pb-1">
        <span className="font-mono text-nano uppercase tracking-wider text-muted-foreground">{lang || "text"}</span>
        <button
          type="button"
          onClick={handleCopy}
          className="inline-flex h-6 w-6 items-center justify-center rounded text-muted-foreground transition-colors hover:bg-muted-50 hover:text-foreground"
          aria-label={copied ? t("chat:code.copied", "已复制") : t("chat:code.copy", "复制代码")}
          title={copied ? t("chat:code.copied", "已复制") : t("chat:code.copy", "复制代码")}
        >
          {copied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
        </button>
      </div>
      <pre
        ref={preRef}
        className="m-0 overflow-x-auto px-3 pb-3 pt-0.5 text-foreground"
        {...props}
      >
        {children}
      </pre>
    </div>
  )
}

// Static component overrides — hoisted to module scope to avoid re-allocating
// a new object (and new closure functions) on every render / streaming chunk.
const MARKDOWN_COMPONENTS: Components = {
  pre: ({ node, className, children, ...props }) => (
    <CodeBlock className={className} {...(props as any)}>
      {children}
    </CodeBlock>
  ),
  code: ({ node, className, children, ...props }) => {
    const isBlock = !!className
    if (!isBlock) {
      return (
        <code className={cn("bg-muted px-1 py-0.5 rounded", textCode, "font-mono text-foreground", className)} {...(props as any)}>
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
  a: ({ node, className, children, href, ...props }) => (
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
  // Tables scroll horizontally WITHIN the message instead of pushing the
  // whole chat panel into horizontal scroll (float chat is only 380-400px).
  table: ({ node, children, ...props }) => (
    <div className="overflow-x-auto">
      <table className="w-full" {...(props as any)}>{children}</table>
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
 * Memoized: only re-renders when content/className/variant change.
 * Component overrides are module-scope (no per-render allocation).
 * Wrapped in ErrorBoundary so malformed markdown can't crash the chat.
 */
export const MarkdownMessage = React.memo<MarkdownMessageProps>(
  ({ content, className, variant = 'assistant' }) => {
  const displayContent = dedupeRepeatedContent(content)

  return (
    <div className={cn("relative", className)}>
      <div
        className={cn(
          // Base prose classes — use prose for structure, override size to 13px
          "prose max-w-none", "text-body sm:text-sm",
          // Text wrapping
          "break-words overflow-wrap-anywhere",
          "prose-p:leading-relaxed prose-p:my-1",
          "prose-headings:font-semibold prose-headings:mt-4 prose-headings:mb-2",
          "prose-h1:text-base prose-h2:text-heading prose-h3:text-sm",
          // Links inherit text color (see MARKDOWN_COMPONENTS.a) so they stay
          // readable on both user and assistant bubble backgrounds. Underline
          // alone provides the link affordance — do NOT set a prose-a color
          // here, it would override the inherit and re-introduce the
          // light-theme invisibility bug on user bubbles.
          "prose-a:text-inherit prose-a:underline prose-a:underline-offset-2",
          "prose-strong:font-semibold",
          "prose-code:rounded prose-code:bg-muted prose-code:px-1 prose-code:py-0.5 prose-code:text-code prose-code:font-mono",  // text-code kept for Tailwind prose modifier
          "prose-code:break-all prose-code:whitespace-pre-wrap",
          // Code block chrome (border, bg, padding, language bar, copy button)
          // is owned by CodeBlock — only keep overflow + inline-code reset here.
          "prose-pre:overflow-x-auto prose-pre:max-w-full",
          "prose-pre:prose-code:bg-transparent prose-pre:prose-code:p-0 prose-pre:prose-code:text-foreground",
          "prose-blockquote:border-l-2 prose-blockquote:border-muted-foreground prose-blockquote:bg-muted-30 prose-blockquote:pl-3 prose-blockquote:pr-3 prose-blockquote:py-1 prose-blockquote:rounded-r-md prose-blockquote:italic",
          "prose-ul:my-1 prose-ul:pl-4 prose-ul:list-disc",
          "prose-ol:my-1 prose-ol:pl-4 prose-ol:list-decimal",
          "prose-li:my-0.5 prose-li:marker:text-muted-foreground",
          "prose-table:my-2 prose-table:text-body",
          "prose-th:px-2 prose-th:py-1.5 prose-th:border-b-2 prose-th:border-border prose-th:bg-muted-50 prose-th:font-semibold",
          "prose-td:px-2 prose-td:py-1.5 prose-td:border-b prose-td:border-muted-30",
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
        </ErrorBoundary>
      </div>
    </div>
  )
},
  (prev, next) =>
    prev.content === next.content &&
    prev.className === next.className &&
    prev.variant === next.variant
)

MarkdownMessage.displayName = "MarkdownMessage"
