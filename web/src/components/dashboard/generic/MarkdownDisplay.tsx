/**
 * Markdown Display Component
 *
 * Renders markdown content using react-markdown with proper styling.
 * Uses the same renderer as the chat component for consistency.
 */

import { memo, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { Components } from 'react-markdown'
import { Skeleton } from '@/components/ui/skeleton'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { FileText } from 'lucide-react'
import type { DataSource } from '@/types/dashboard'
import { EmptyState } from '../shared'

export interface MarkdownDisplayProps {
  dataSource?: DataSource
  content?: string
  size?: 'sm' | 'md' | 'lg'

  // Display options
  variant?: 'default' | 'compact' | 'minimal'
  allowHtml?: boolean
  maxLines?: number

  className?: string
}

// Custom components for react-markdown
const markdownComponents: Components = {
  // Custom code block rendering
  pre: ({ node, className, children, ...props }) => (
    <pre className={cn("overflow-x-auto bg-muted p-2 rounded-md my-2", className)} {...(props as any)}>
      {children}
    </pre>
  ),
  // Custom inline code
  code: ({ node, className, children, ...props }) => {
    const inline = (props as any).inline
    if (inline) {
      return (
        <code className={cn("bg-muted px-1 py-0.5 rounded text-xs font-mono", className)} {...(props as any)}>
          {children}
        </code>
      )
    }
    return (
      <code className={cn("text-xs font-mono", className)} {...(props as any)}>
        {children}
      </code>
    )
  },
  // Custom link rendering
  a: ({ node, className, children, href, ...props }) => (
    <a
      className={cn("text-primary hover:underline", className)}
      href={href as string}
      target="_blank"
      rel="noopener noreferrer"
      {...(props as any)}
    >
      {children}
    </a>
  ),
}

const MarkdownContent = memo(({ content, className, maxLines }: { content: string; className?: string; maxLines?: number }) => {
  return (
    <div className={cn(
      "prose prose-sm dark:prose-invert max-w-none",
      "prose-p:leading-relaxed prose-p:my-1",
      "prose-headings:font-semibold prose-headings:my-2",
      "prose-h1:text-base prose-h2:text-sm prose-h3:text-xs",
      "prose-a:text-primary prose-a:no-underline hover:prose-a:underline",
      "prose-strong:font-semibold",
      "prose-em:italic",
      "prose-code:rounded prose-code:bg-muted prose-code:px-1 prose-code:py-0.5 prose-code:text-xs prose-code:font-mono",
      "prose-pre:bg-muted prose-pre:p-2 prose-pre:rounded-md prose-pre:my-2",
      "prose-pre:prose-code:bg-transparent prose-pre:prose-code:p-0",
      "prose-blockquote:border-l-2 prose-blockquote:border-muted-foreground/30 prose-blockquote:pl-3 prose-blockquote:italic prose-blockquote:my-2",
      "prose-ul:my-1 prose-ul:pl-4 prose-ul:list-disc",
      "prose-ol:my-1 prose-ol:pl-4 prose-ol:list-decimal",
      "prose-li:my-0 prose-li:text-xs",
      "prose-table:my-2 prose-table:text-xs",
      "prose-th:px-2 prose-th:py-1 prose-th:border prose-th:border-border prose-th:bg-muted/50",
      "prose-td:px-2 prose-th:py-1 prose-td:border prose-td:border-border",
      "prose-hr:my-2 prose-hr:border-border",
      maxLines && `line-clamp-${maxLines}`,
      className
    )}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
        {content}
      </ReactMarkdown>
    </div>
  )
})
MarkdownContent.displayName = 'MarkdownContent'

export function MarkdownDisplay({
  dataSource,
  content: propContent,
  size = 'md',
  variant = 'default',
  allowHtml = false,
  maxLines,
  className,
}: MarkdownDisplayProps) {
  const { t } = useTranslation('dashboardComponents')

  // Always call useDataSource - it will handle undefined dataSource internally
  // This ensures proper cleanup when dataSource is removed
  const { data, loading, error } = useDataSource<string>(dataSource, {
    fallback: propContent,
  })

  const hasDataSource = dataSource !== undefined

  // Content determination: use fetched data if available, otherwise use prop
  const content = useMemo(() => {
    // Only use data source when we have one and it's valid
    if (hasDataSource && !error && data !== undefined && data !== null) {
      // Safely convert data to string, handling arrays and other types
      if (typeof data === 'string') return data
      if (Array.isArray(data)) {
        const firstItem = data[0]
        return typeof firstItem === 'string' ? firstItem : String(firstItem ?? '')
      }
      return String(data ?? '')
    }
    // No dataSource or error - use propContent directly
    return propContent ?? ''
  }, [hasDataSource, error, data, propContent])

  const sizeConfig = dashboardComponentSize[size]

  // Loading state - only show loading if we have a dataSource and no content yet
  if (hasDataSource && loading && !content) {
    return (
      <div className={cn(dashboardCardBase, sizeConfig.padding, className)}>
        <div className="w-full space-y-2">
          <Skeleton className="h-4 w-3/4" />
          <Skeleton className="h-4 w-1/2" />
          <Skeleton className="h-4 w-2/3" />
        </div>
      </div>
    )
  }

  // Empty state
  if (!content) {
    return (
      <EmptyState
        size={size}
        className={className}
        icon={<FileText />}
        message={t('markdownDisplay.noContent')}
        subMessage={t('markdownDisplay.addContent')}
      />
    )
  }

  // Variant styles
  const variantStyles = {
    default: '',
    compact: 'text-xs',
    minimal: 'p-0',
  }

  return (
    <div className={cn(dashboardCardBase, 'overflow-hidden flex flex-col', className)}>
      <div className={cn(
        'flex-1 min-h-0 overflow-auto',
        sizeConfig.padding,
        variantStyles[variant]
      )}>
        <MarkdownContent content={content} maxLines={maxLines} />
      </div>
    </div>
  )
}
