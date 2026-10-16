/**
 * Host component that loads an extension-contributed page bundle (UMD) and
 * renders its exported component. Modeled on the dashboard ComponentRenderer
 * loading path: script tag injection, global lookup, named/default export.
 */

import { useEffect, useState, type ComponentType } from 'react'

export interface ExtensionPageHostProps {
  bundleUrl: string
  exportName: string
  globalName: string | null
  title: string
}

// Track loaded bundles so navigating back doesn't re-fetch.
const loadedGlobals = new Set<string>()

function loadUmdBundle(
  bundleUrl: string,
  globalName: string,
): Promise<Record<string, unknown>> {
  return new Promise((resolve, reject) => {
    const w = window as unknown as Record<string, unknown>
    if (w[globalName] && loadedGlobals.has(globalName)) {
      resolve(w[globalName] as Record<string, unknown>)
      return
    }
    const script = document.createElement('script')
    script.src = bundleUrl
    script.async = true
    script.onload = () => {
      script.remove()
      const global = (window as unknown as Record<string, unknown>)[globalName]
      if (global) {
        loadedGlobals.add(globalName)
        resolve(global as Record<string, unknown>)
      } else {
        reject(new Error(`Bundle loaded but global "${globalName}" is missing`))
      }
    }
    script.onerror = () => {
      script.remove()
      reject(new Error(`Failed to load bundle: ${bundleUrl}`))
    }
    document.head.appendChild(script)
  })
}

export function ExtensionPageHost({ bundleUrl, exportName, globalName, title }: ExtensionPageHostProps) {
  const [Component, setComponent] = useState<ComponentType | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setComponent(null)
    setError(null)
    // Extensions without global_name can't be loaded via script tag — the
    // assets route serves the raw UMD; require the declaration.
    if (!globalName) {
      setError('Page manifest lacks global_name')
      return
    }
    loadUmdBundle(bundleUrl, globalName)
      .then((global) => {
        if (cancelled) return
        const named = global[exportName]
        const def = typeof global === 'function' ? global : (global.default as unknown)
        const resolved = (named ?? def) as ComponentType | undefined
        if (typeof resolved === 'function' || (resolved && typeof resolved === 'object')) {
          setComponent(resolved as ComponentType)
        } else {
          setError(`Export "${exportName}" not found in bundle`)
        }
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e))
      })
    return () => {
      cancelled = true
    }
  }, [bundleUrl, exportName, globalName])

  if (error) {
    return (
      <div className="flex h-full items-center justify-center p-8">
        <div className="max-w-md rounded-lg border border-destructive/40 bg-destructive/10 p-6 text-center">
          <div className="mb-1 font-semibold text-destructive">扩展页面加载失败</div>
          <div className="text-sm text-muted-foreground">{title}: {error}</div>
        </div>
      </div>
    )
  }

  if (!Component) {
    return (
      <div className="flex h-full items-center justify-center p-8 text-muted-foreground">
        正在加载 {title}…
      </div>
    )
  }

  return (
    <div className="h-full overflow-auto p-4 md:p-6">
      <Component />
    </div>
  )
}
