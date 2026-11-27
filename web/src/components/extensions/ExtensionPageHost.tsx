/**
 * Host component that loads an extension-contributed page bundle (UMD) and
 * renders its exported component. Modeled on the dashboard ComponentRenderer
 * loading path: script tag injection, global lookup, named/default export.
 */

import { Component, type ComponentType } from 'react'
import i18n from 'i18next'
import { AlertCircle, Loader2 } from 'lucide-react'

interface HostState {
  Component: ComponentType | null
  error: string | null
}

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

export class ExtensionPageHost extends Component<ExtensionPageHostProps, HostState> {
  // Class component on purpose: the function version gets a react-refresh
  // `_s()` signature sigil in dev, whose hook-collection window corrupts
  // the dispatcher when a bundle-loaded component first mounts from inside
  // useEffect — React 18 dev then throws "Should have a queue" and the
  // ErrorBoundary eats the route. Classes take a different refresh path
  // (no hook signature collection), so this host behaves identically in
  // dev and production.
  state: HostState = { Component: null, error: null }
  private cancelled = false

  componentDidMount() {
    this.load()
  }

  componentDidUpdate(prev: ExtensionPageHostProps) {
    if (prev.bundleUrl !== this.props.bundleUrl) {
      this.setState({ Component: null, error: null })
      this.load()
    }
  }

  componentWillUnmount() {
    this.cancelled = true
  }

  private load() {
    this.cancelled = false
    if (!this.props.globalName) {
      this.setState({ error: 'Page manifest lacks global_name' })
      return
    }
    loadUmdBundle(this.props.bundleUrl, this.props.globalName)
      .then((global) => {
        if (this.cancelled) return
        const named = global[this.props.exportName]
        const def = typeof global === 'function' ? global : (global.default as unknown)
        const resolved = (named ?? def) as ComponentType | undefined
        if (typeof resolved === 'function' || (resolved && typeof resolved === 'object')) {
          this.setState({ Component: resolved as ComponentType })
        } else {
          this.setState({ error: `Export "${this.props.exportName}" not found in bundle` })
        }
      })
      .catch((e) => {
        if (!this.cancelled) this.setState({ error: e instanceof Error ? e.message : String(e) })
      })
  }

  render() {
    const { title } = this.props
    const { Component, error } = this.state
    if (error) {
      return (
        <div className="flex h-full items-center justify-center p-8">
          <div className="max-w-md rounded-xl border border-error bg-error-light p-6 text-center space-y-2">
            <div className="flex items-center justify-center gap-2 text-sm font-semibold text-error">
              <AlertCircle className="h-4 w-4 shrink-0" />
              <span>{i18n.t('extensions:pageHost.loadFailed', { defaultValue: 'Failed to load extension page' })}</span>
            </div>
            <div className="text-xs leading-relaxed text-muted-foreground break-words [overflow-wrap:anywhere]">
              {title}: {error}
            </div>
          </div>
        </div>
      )
    }
    if (!Component) {
      return (
        <div className="flex h-full items-center justify-center gap-2.5 p-8 text-sm text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin text-primary" />
          <span>{i18n.t('extensions:pageHost.loading', { title, defaultValue: `Loading ${title}...` })}</span>
        </div>
      )
    }
    return (
      <div className="h-full overflow-auto p-4 md:p-6">
        <Component />
      </div>
    )
  }
}
