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
const isTauri =
  typeof window !== 'undefined' &&
  ('__TAURI__' in window || '__TAURI_INTERNALS__' in window)

const DEFAULT_SAFE_SANDBOX = 'allow-scripts allow-forms allow-popups'

/**
 * Strictly validate that a URL uses http: or https: protocol.
 * Rejects javascript:, data:, vbscript:, file:, and malformed/relative URLs.
 */
export function sanitizeHttpUrl(raw: string | undefined): string | null {
  if (!raw) return null
  const trimmed = raw.trim()
  if (!trimmed) return null
  try {
    const parsed = new URL(trimmed)
    if (parsed.protocol === 'http:' || parsed.protocol === 'https:') {
      return parsed.href
    }
    return null
  } catch {
    return null
  }
}

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
// IMPORTANT: allow-scripts + allow-same-origin together neutralize the sandbox
// (the iframe can remove its own sandbox attribute). We enforce mutual exclusion
// and always return a safe sandbox attribute (never undefined) so embedded pages
// cannot access the parent origin.
function getSandboxPolicy(props: {
  sandbox?: boolean
  allowScripts?: boolean
  allowSameOrigin?: boolean
  allowForms?: boolean
  allowPopups?: boolean
}): string {
  if (props.sandbox === false) return DEFAULT_SAFE_SANDBOX

  const policies: string[] = []
  // Mutual exclusion: if both are requested, prefer allow-scripts (safer default
  // for dashboard widgets that need JS but shouldn't access parent origin).
  if (props.allowScripts && props.allowSameOrigin) {
    policies.push('allow-scripts')
  } else {
    if (props.allowScripts) policies.push('allow-scripts')
    if (props.allowSameOrigin) policies.push('allow-same-origin')
  }
  if (props.allowForms) policies.push('allow-forms')
  if (props.allowPopups) policies.push('allow-popups')

  return policies.join(' ')
}

export function WebDisplay({
  dataSource,
  src: propSrc,
  title,
  size = 'md',
  sandbox = true,
  allowFullscreen = true,
  allowScripts = true,
  allowSameOrigin = false,
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
  const [activeUrl, setActiveUrl] = useState(src)
  const [isTauriEnv, setIsTauriEnv] = useState(false)

  // Check Tauri environment
  useEffect(() => {
    setIsTauriEnv(isTauri)
  }, [])

  // Sync URL
  useEffect(() => {
    setCurrentUrl(src)
    setActiveUrl(src)
    setIframeLoading(true)
  }, [src])

  const safeSrc = useMemo(() => sanitizeHttpUrl(src), [src])
  const safeActiveUrl = useMemo(() => sanitizeHttpUrl(activeUrl), [activeUrl])

  const sandboxPolicy = useMemo(() => getSandboxPolicy({
    sandbox,
    allowScripts,
    allowSameOrigin,
    allowForms,
    allowPopups,
  }), [sandbox, allowScripts, allowSameOrigin, allowForms, allowPopups])

  const sizeConfig = dashboardComponentSize[size]

  const handleRefresh = useCallback(() => {
    if (!safeActiveUrl) return
    setIframeLoading(true)
    if (iframeRef.current) {
      iframeRef.current.src = ''
      setTimeout(() => {
        if (iframeRef.current) {
          iframeRef.current.src = safeActiveUrl
        }
      }, 0)
    }
  }, [safeActiveUrl])

  const handleUrlSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    setActiveUrl(currentUrl)
    if (sanitizeHttpUrl(currentUrl)) {
      setIframeLoading(true)
    }
  }

  // Open in new Tauri window
  const openInNewWindow = useCallback(async () => {
    if (!isTauriEnv || !safeSrc) return

    try {
      const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow')
      const label = `webview-${Date.now()}`
      const windowTitle = title || safeSrc.replace(/^https?:\/\//, '').split('/')[0]

      new WebviewWindow(label, {
        url: safeSrc,
        title: windowTitle,
        width: 1200,
        height: 800,
        center: true,
        resizable: true,
      })
    } catch (err) {
      console.error('Failed to open window:', err)
      window.open(safeSrc, '_blank', 'noopener,noreferrer')
    }
  }, [isTauriEnv, safeSrc, title])

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

  // No source — only show empty state when loading is done
  if (!loading && !src) {
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

  // Blocked / invalid URL protocol (e.g. javascript:, data:, vbscript:, file:)
  if (!loading && src && !safeSrc) {
    return (
      <EmptyState
        size={size}
        className={className}
        icon={<Lock />}
        message={t('webDisplay.invalidUrl', 'Invalid URL')}
        subMessage={t('webDisplay.invalidUrlHint', 'Only http:// and https:// URLs are allowed')}
      />
    )
  }

  // Tauri: Show placeholder with open button
  if (isTauriEnv) {
    const displayHost = (safeSrc ?? '').replace(/^https?:\/\//, '').split('/')[0]
    return (
      <div className={cn(dashboardCardBase, 'flex flex-col overflow-hidden', !borderless && 'border', className)}>
        {showHeader && (
          <div className={cn('flex items-center gap-2 px-3 py-2 bg-muted-30 border-b', size === 'sm' ? 'py-1.5' : '')}>
            <Globe className="h-4 w-4 text-muted-foreground shrink-0" />
            {title && <span className="min-w-0 flex-1 truncate text-sm font-medium">{title}</span>}
            {sandbox && <Lock className="h-4 w-4 shrink-0 text-muted-foreground" />}
            {!title && <div className="min-w-0 flex-1" />}
            <Button variant="ghost" size="icon" className="h-6 w-6 shrink-0 sm:h-6 sm:w-6" onClick={handleRefresh}>
              <RefreshCw className="h-4 w-4" />
            </Button>
          </div>
        )}
        <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-4 bg-muted p-4">
          <Globe className="h-12 w-12 text-muted-foreground" />
          <div className="min-w-0 max-w-full text-center">
            <p className="mb-1 truncate text-sm text-muted-foreground">
              {title || displayHost}
            </p>
            <p className="text-xs text-muted-foreground mb-4">
              {t('webDisplay.openInWindowHint', 'Click to open in a separate window')}
            </p>
          </div>
          <Button onClick={openInNewWindow} disabled={!safeSrc} className="gap-2">
            <Maximize2 className="h-4 w-4" />
            {t('webDisplay.openInNewWindow', 'Open in new window')}
          </Button>
        </div>
      </div>
    )
  }

  // Browser: Use iframe
  const headerContent = (
    <div className={cn('flex items-center gap-2 px-3 py-2 bg-muted-30 border-b', size === 'sm' ? 'py-1.5' : '')}>
      <Globe className="h-4 w-4 text-muted-foreground shrink-0" />
      {title && <span className="min-w-0 flex-1 truncate text-sm font-medium">{title}</span>}
      {sandbox && <Lock className="h-4 w-4 shrink-0 text-muted-foreground" />}
      {!title && <div className="min-w-0 flex-1" />}
      <Button variant="ghost" size="icon" className="h-6 w-6 shrink-0 sm:h-6 sm:w-6" onClick={handleRefresh} disabled={!safeActiveUrl}>
        <RefreshCw className={cn('h-4 w-4', iframeLoading && safeActiveUrl && 'animate-spin')} />
      </Button>
      {safeActiveUrl && (
        <Button variant="ghost" size="icon" className="h-6 w-6 shrink-0 sm:h-6 sm:w-6" asChild>
          <a href={safeActiveUrl} target="_blank" rel="noopener noreferrer">
            <ExternalLink className="h-4 w-4" />
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
        {!safeActiveUrl ? (
          <EmptyState
            size={size}
            className="h-full border-0"
            icon={<Lock />}
            message={t('webDisplay.invalidUrl', 'Invalid URL')}
            subMessage={t('webDisplay.invalidUrlHint', 'Only http:// and https:// URLs are allowed')}
          />
        ) : (
          <>
            {iframeLoading && (
              <div className="absolute inset-0 flex items-center justify-center bg-muted z-10">
                <Skeleton className="w-full h-full" />
              </div>
            )}
            <iframe
              ref={iframeRef}
              src={safeActiveUrl}
              title={title || 'Web content'}
              className={cn('w-full h-full border-0', transparent && 'bg-transparent', !borderless && 'bg-background')}
              sandbox={sandboxPolicy}
              allowFullScreen={allowFullscreen}
              onLoad={handleIframeLoad}
              onError={handleIframeError}
            />
          </>
        )}
      </div>
    </div>
  )
}