/**
 * Web Display Component
 *
 * Displays web content via iframe.
 * Supports URL configuration, sandboxing, and loading states.
 */

import { useState, useRef, useMemo } from 'react'
import { Skeleton } from '@/components/ui/skeleton'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { ExternalLink, RefreshCw, Globe, Lock } from 'lucide-react'
import type { DataSource } from '@/types/dashboard'

export interface WebDisplayProps {
  dataSource?: DataSource
  src?: string
  title?: string
  size?: 'sm' | 'md' | 'lg'

  // Iframe options
  sandbox?: boolean
  allowFullscreen?: boolean
  allowScripts?: boolean
  allowSameOrigin?: boolean
  allowForms?: boolean
  allowPopups?: boolean

  // Display options
  showHeader?: boolean
  showUrlBar?: boolean
  transparent?: boolean
  borderless?: boolean

  className?: string
}

// Get sandbox policy string based on permissions
function getSandboxPolicy(props: {
  sandbox?: boolean
  allowScripts?: boolean
  allowSameOrigin?: boolean
  allowForms?: boolean
  allowPopups?: boolean
}): string {
  if (!props.sandbox) return ''

  const policies: string[] = []

  if (!props.allowScripts) policies.push('scripts')
  if (!props.allowSameOrigin) policies.push('same-origin')
  if (!props.allowForms) policies.push('forms')
  if (!props.allowPopups) policies.push('popups')

  // Default allowed: allow-popups, allow-forms, allow-same-origin, allow-scripts
  // When sandbox is true, we specify what to disable
  return policies.length > 0 ? policies.join(' ') : 'allow-popups allow-forms allow-same-origin allow-scripts'
}

export function WebDisplay({
  dataSource,
  src: propSrc,
  title,
  size = 'md',
  sandbox = true,
  allowFullscreen = true,
  allowScripts = false,
  allowSameOrigin = false,
  allowForms = false,
  allowPopups = false,
  showHeader = true,
  showUrlBar = false,
  transparent = false,
  borderless = false,
  className,
}: WebDisplayProps) {
  const { data, loading, error } = useDataSource<string>(dataSource, {
    fallback: propSrc,
  })

  const src = error ? propSrc : (data ?? propSrc ?? '')
  const iframeRef = useRef<HTMLIFrameElement>(null)
  const [iframeLoading, setIframeLoading] = useState(true)
  const [currentUrl, setCurrentUrl] = useState(src)

  const sandboxPolicy = useMemo(() => getSandboxPolicy({
    sandbox,
    allowScripts,
    allowSameOrigin,
    allowForms,
    allowPopups,
  }), [sandbox, allowScripts, allowSameOrigin, allowForms, allowPopups])

  const sizeConfig = dashboardComponentSize[size]

  const handleRefresh = () => {
    setIframeLoading(true)
    if (iframeRef.current) {
      iframeRef.current.src = iframeRef.current.src
    }
  }

  const handleUrlSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (currentUrl) {
      setIframeLoading(true)
    }
  }

  // Initial loading state
  if (loading && !src) {
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', sizeConfig.padding, className)}>
        <Skeleton className="w-full h-full" />
      </div>
    )
  }

  // No source state
  if (!src) {
    return (
      <div className={cn(dashboardCardBase, 'flex flex-col items-center justify-center gap-3', sizeConfig.padding, className)}>
        <Globe className={cn('text-muted-foreground', size === 'sm' ? 'h-8 w-8' : size === 'md' ? 'h-12 w-12' : 'h-16 w-16')} />
        <div className="text-center">
          <p className="text-muted-foreground text-sm">No URL specified</p>
          <p className="text-muted-foreground/60 text-xs mt-1">Configure a data source or URL</p>
        </div>
      </div>
    )
  }

  const headerContent = (
    <div className={cn(
      'flex items-center gap-2 px-3 py-2 bg-muted/30 border-b',
      size === 'sm' ? 'py-1.5' : ''
    )}>
      <Globe className="h-4 w-4 text-muted-foreground shrink-0" />
      {title && (
        <span className="font-medium text-sm truncate flex-1">{title}</span>
      )}
      {sandbox && (
        <Lock className="h-3.5 w-3.5 text-muted-foreground/60 shrink-0" />
      )}
      <div className="flex-1" />
      <Button
        variant="ghost"
        size="icon"
        className="h-7 w-7 shrink-0"
        onClick={handleRefresh}
      >
        <RefreshCw className={cn('h-3.5 w-3.5', iframeLoading && 'animate-spin')} />
      </Button>
      {src && (
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 shrink-0"
          asChild
        >
          <a href={src} target="_blank" rel="noopener noreferrer">
            <ExternalLink className="h-3.5 w-3.5" />
          </a>
        </Button>
      )}
    </div>
  )

  const urlBarContent = showUrlBar && (
    <form onSubmit={handleUrlSubmit} className="px-3 pb-2">
      <Input
        type="url"
        value={currentUrl}
        onChange={(e) => setCurrentUrl(e.target.value)}
        placeholder="https://example.com"
        className="h-8 text-xs"
      />
    </form>
  )

  return (
    <div className={cn(dashboardCardBase, 'flex flex-col overflow-hidden', !borderless && 'border', className)}>
      {showHeader && headerContent}
      {urlBarContent}
      <div className="flex-1 relative min-h-0">
        {iframeLoading && (
          <div className="absolute inset-0 flex items-center justify-center bg-muted/10 z-10">
            <Skeleton className="w-full h-full" />
          </div>
        )}
        <iframe
          ref={iframeRef}
          src={currentUrl || src}
          title={title || 'Web content'}
          className={cn(
            'w-full h-full border-0',
            transparent && 'bg-transparent',
            !borderless && 'bg-background'
          )}
          sandbox={sandboxPolicy || undefined}
          allowFullScreen={allowFullscreen}
          onLoad={() => setIframeLoading(false)}
          onError={() => setIframeLoading(false)}
        />
      </div>
    </div>
  )
}
