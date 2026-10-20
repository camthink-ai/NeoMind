/**
 * Host component that loads an extension-contributed page bundle (UMD) and
 * renders its exported component. Modeled on the dashboard ComponentRenderer
 * loading path: script tag injection, global lookup, named/default export.
 */

import { Component, type ComponentType } from 'react'

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
}
