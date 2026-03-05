/**
 * Web Display Component
 *
 * Displays web content via iframe in browser.
 * In Tauri, shows a placeholder with button to open in new window (due to CSP restrictions).
 */

import { useState, useRef, useMemo, useEffect, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Skeleton } from '@/components/ui/skeleton'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { ExternalLink, RefreshCw, Globe, Lock, Maximize2 } from 'lucide-react'
import type { DataSource } from '@/types/dashboard'
import { EmptyState } from '../shared'

// Check if running in Tauri environment
const isTauri = typeof window !== 'undefined' && '__TAURI__' in window

export interface WebDisplayProps {
  dataSource?: DataSource
  src?: string
  title?: string
  size?: 'sm' | 'md' | 'lg'

  // Iframe options (browser only)
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

// Get sandbox policy string (browser only)
function getSandboxPolicy(props: {
  sandbox?: boolean
  allowScripts?: boolean
  allowSameOrigin?: boolean
  allowForms?: boolean
  allowPopups?: boolean
}): string {
  if (!props.sandbox) return ''

  const policies: string[] = []
  if (props.allowScripts) policies.push('allow-scripts')
  if (props.allowSameOrigin) policies.push('allow-same-origin')
  if (props.allowForms) policies.push('allow-forms')
  if (props.allowPopups) policies.push('allow-popups')

  return policies.join(' ')
}

export function WebDisplay({
  dataSource,
  src: propSrc,
  title,
  size = 'md',
  sandbox = false,
  allowFullscreen = true,
  allowScripts = true,
  allowSameOrigin = true,
  allowForms = true,
  allowPopups = false,
  showHeader = true,
  showUrlBar = false,
  transparent = false,
  borderless = false,
  className,
}: WebDisplayProps) {
  const { t } = useTranslation('dashboardComponents')

  const { data, loading, error } = useDataSource<string>(dataSource, {
    fallback: propSrc,
  })

  const hasDataSource = dataSource !== undefined

  const src = useMemo(() => {
    if (hasDataSource && !error && data !== undefined && data !== null) {
      if (typeof data === 'string') return data
      if (Array.isArray(data)) {
        const firstItem = data[0]
        return typeof firstItem === 'string' ? firstItem : String(firstItem ?? '')
      }
      return String(data ?? '')
    }
    return propSrc ?? ''
  }, [hasDataSource, error, data, propSrc])

  // Refs
  const iframeRef = useRef<HTMLIFrameElement>(null)

  // State
  const [iframeLoading, setIframeLoading] = useState(true)
  const [currentUrl, setCurrentUrl] = useState(src)
  const [isTauriEnv, setIsTauriEnv] = useState(false)

  // Check Tauri environment
  useEffect(() => {
    setIsTauriEnv(isTauri)
  }, [])

  // Sync URL
  useEffect(() => {
    setCurrentUrl(src)
    setIframeLoading(true)
  }, [src])

  const sandboxPolicy = useMemo(() => getSandboxPolicy({
    sandbox,
    allowScripts,
    allowSameOrigin,
    allowForms,
    allowPopups,
  }), [sandbox, allowScripts, allowSameOrigin, allowForms, allowPopups])

  const sizeConfig = dashboardComponentSize[size]

  const handleRefresh = useCallback(() => {
    setIframeLoading(true)
    if (iframeRef.current) {
      const currentSrc = iframeRef.current.src
      iframeRef.current.src = ''
      setTimeout(() => {
        if (iframeRef.current) {
          iframeRef.current.src = currentSrc
        }
      }, 0)
    }
  }, [])

  const handleUrlSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (currentUrl) {
      setIframeLoading(true)
    }
  }

  // Open in new Tauri window
  const openInNewWindow = useCallback(async () => {
    if (!isTauriEnv || !src) return

    try {
      const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow')
      const label = `webview-${Date.now()}`
      const windowTitle = title || src.replace(/^https?:\/\//, '').split('/')[0]

      new WebviewWindow(label, {
        url: src,
        title: windowTitle,
        width: 1200,
        height: 800,
        center: true,
        resizable: true,
      })
    } catch (err) {
      console.error('Failed to open window:', err)
      window.open(src, '_blank')
    }
  }, [isTauriEnv, src, title])

  // Iframe handlers
  const handleIframeLoad = useCallback(() => {
    setIframeLoading(false)
  }, [])

  const handleIframeError = useCallback(() => {
    setIframeLoading(false)
  }, [])

  // Loading state
  if (hasDataSource && loading && !src) {
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', sizeConfig.padding, className)}>
        <Skeleton className="w-full h-full" />
      </div>
    )
  }

  // No source
  if (!src) {
    return (
      <EmptyState
        size={size}
        className={className}
        icon={<Globe />}
        message={t('webDisplay.noUrlSpecified')}
        subMessage={t('webDisplay.configureUrl')}
      />
    )
  }

  // Tauri: Show placeholder with open button
  if (isTauriEnv) {
    return (
      <div className={cn(dashboardCardBase, 'flex flex-col overflow-hidden', !borderless && 'border', className)}>
        {showHeader && (
          <div className={cn('flex items-center gap-2 px-3 py-2 bg-muted/30 border-b', size === 'sm' ? 'py-1.5' : '')}>
            <Globe className="h-4 w-4 text-muted-foreground shrink-0" />
            {title && <span className="font-medium text-sm truncate flex-1">{title}</span>}
            {sandbox && <Lock className="h-3.5 w-3.5 text-muted-foreground/60 shrink-0" />}
            <div className="flex-1" />
            <Button variant="ghost" size="icon" className="h-7 w-7 shrink-0" onClick={handleRefresh}>
              <RefreshCw className="h-3.5 w-3.5" />
            </Button>
          </div>
        )}
        <div className="flex-1 flex flex-col items-center justify-center gap-4 p-4 bg-muted/10">
          <Globe className="h-12 w-12 text-muted-foreground/50" />
          <div className="text-center">
            <p className="text-sm text-muted-foreground mb-1">
              {title || src.replace(/^https?:\/\//, '').split('/')[0]}
            </p>
            <p className="text-xs text-muted-foreground/70 mb-4">
              {t('webDisplay.openInWindowHint', 'Click to open in a separate window')}
            </p>
          </div>
          <Button onClick={openInNewWindow} className="gap-2">
            <Maximize2 className="h-4 w-4" />
            {t('webDisplay.openInNewWindow', 'Open in new window')}
          </Button>
        </div>
      </div>
    )
  }

  // Browser: Use iframe
  const headerContent = (
    <div className={cn('flex items-center gap-2 px-3 py-2 bg-muted/30 border-b', size === 'sm' ? 'py-1.5' : '')}>
      <Globe className="h-4 w-4 text-muted-foreground shrink-0" />
      {title && <span className="font-medium text-sm truncate flex-1">{title}</span>}
      {sandbox && <Lock className="h-3.5 w-3.5 text-muted-foreground/60 shrink-0" />}
      <div className="flex-1" />
      <Button variant="ghost" size="icon" className="h-7 w-7 shrink-0" onClick={handleRefresh}>
        <RefreshCw className={cn('h-3.5 w-3.5', iframeLoading && 'animate-spin')} />
      </Button>
      {src && (
        <Button variant="ghost" size="icon" className="h-7 w-7 shrink-0" asChild>
          <a href={src} target="_blank" rel="noopener noreferrer">
            <ExternalLink className="h-3.5 w-3.5" />
          </a>
        </Button>
      )}
    </div>
  )

  const urlBarContent = showUrlBar && (
    <form onSubmit={handleUrlSubmit} className="px-3 pb-2">
      <Input type="url" value={currentUrl} onChange={(e) => setCurrentUrl(e.target.value)} placeholder={t('webDisplay.urlPlaceholder')} className="h-8 text-xs" />
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
          className={cn('w-full h-full border-0', transparent && 'bg-transparent', !borderless && 'bg-background')}
          sandbox={sandboxPolicy || undefined}
          allowFullScreen={allowFullscreen}
          onLoad={handleIframeLoad}
          onError={handleIframeError}
        />
      </div>
    </div>
  )
}