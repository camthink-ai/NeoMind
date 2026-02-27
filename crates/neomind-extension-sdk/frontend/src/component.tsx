/**
 * NeoMind Extension Component Template
 *
 * Base template for creating extension frontend components.
 * Copy and customize this file for your extension.
 */

import {
  forwardRef,
  useEffect,
  useState,
  useCallback,
  type ReactNode,
  type Ref,
  type CSSProperties,
} from 'react'
import {
  type DataSource,
  type ApiResponse,
  executeExtensionCommand,
  getExtensionMetrics,
  getApiBase,
  getAuthToken,
} from './types'

// ============================================================================
// Component Props
// ============================================================================

export interface ExtensionCardProps {
  /** Card title */
  title?: string
  /** Data source configuration */
  dataSource?: DataSource
  /** Custom class name */
  className?: string
  /** Custom styles */
  style?: CSSProperties
  /** Default configuration values */
  defaultConfig?: Record<string, unknown>
  /** Refresh interval in milliseconds (0 = no auto-refresh) */
  refreshInterval?: number
  /** Dark mode flag */
  darkMode?: boolean
  /** Language for i18n */
  language?: string
}

// ============================================================================
// Component State Types
// ============================================================================

interface ComponentState<T = unknown> {
  /** Current data */
  data: T | null
  /** Loading state */
  loading: boolean
  /** Error message */
  error: string | null
  /** Last update time */
  lastUpdated: Date | null
}

// ============================================================================
// API Helpers (Component-scoped)
// ============================================================================

/**
 * Fetch data from extension
 */
async function fetchExtensionData<T>(
  extensionId: string,
  command: string,
  args: Record<string, unknown> = {}
): Promise<T | null> {
  const apiBase = getApiBase()
  const token = getAuthToken()

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  }

  if (token) {
    headers['Authorization'] = `Bearer ${token}`
  }

  try {
    const response = await fetch(`${apiBase}/extensions/${extensionId}/command`, {
      method: 'POST',
      headers,
      body: JSON.stringify({ command, args }),
    })

    if (!response.ok) {
      return null
    }

    const result: ApiResponse<T> = await response.json()
    return result.success ? result.data ?? null : null
  } catch {
    return null
  }
}

// ============================================================================
// Default Component Styles
// ============================================================================

const defaultStyles = `
  .ext-card {
    --ext-bg: rgba(255, 255, 255, 0.25);
    --ext-fg: hsl(240 10% 10%);
    --ext-muted: hsl(240 5% 40%);
    --ext-border: rgba(255, 255, 255, 0.5);
    --ext-accent: hsl(221 83% 53%);
    --ext-glass: rgba(255, 255, 255, 0.3);
    --ext-error: hsl(0 84% 60%);

    width: 100%;
    height: 100%;
    font-family: system-ui, -apple-system, sans-serif;
  }

  .dark .ext-card,
  [data-theme="dark"] .ext-card {
    --ext-bg: rgba(30, 30, 30, 0.4);
    --ext-fg: hsl(0 0% 95%);
    --ext-muted: hsl(0 0% 65%);
    --ext-border: rgba(255, 255, 255, 0.1);
    --ext-glass: rgba(255, 255, 255, 0.06);
  }

  .ext-card-container {
    background: var(--ext-bg);
    backdrop-filter: blur(16px) saturate(180%);
    -webkit-backdrop-filter: blur(16px) saturate(180%);
    border: 1px solid var(--ext-border);
    border-radius: 12px;
    padding: 12px;
    height: 100%;
    width: 100%;
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    overflow: hidden;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.08);
  }

  .ext-card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--ext-border);
    margin-bottom: 8px;
  }

  .ext-card-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--ext-fg);
    margin: 0;
  }

  .ext-card-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .ext-card-loading,
  .ext-card-error,
  .ext-card-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--ext-muted);
    font-size: 12px;
  }

  .ext-card-error {
    color: var(--ext-error);
  }

  .ext-spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--ext-border);
    border-top-color: var(--ext-accent);
    border-radius: 50%;
    animation: ext-spin 0.8s linear infinite;
  }

  @keyframes ext-spin {
    to { transform: rotate(360deg); }
  }

  .ext-card-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    padding-top: 8px;
    border-top: 1px solid var(--ext-border);
    margin-top: 8px;
    font-size: 10px;
    color: var(--ext-muted);
  }

  .ext-refresh-btn {
    background: transparent;
    border: none;
    color: var(--ext-muted);
    cursor: pointer;
    padding: 4px;
    border-radius: 4px;
    transition: color 0.2s, background 0.2s;
  }

  .ext-refresh-btn:hover {
    color: var(--ext-accent);
    background: var(--ext-glass);
  }

  .ext-refresh-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .ext-refresh-btn svg {
    width: 14px;
    height: 14px;
  }
`

// ============================================================================
// Icons
// ============================================================================

const RefreshIcon = (
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
    <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
    <path d="M21 3v5h-5" />
  </svg>
)

// ============================================================================
// Component Implementation
// ============================================================================

/**
 * Extension Card Component Template
 *
 * Customize this component for your extension's needs.
 */
export const ExtensionCard = forwardRef<HTMLDivElement, ExtensionCardProps>(
  function ExtensionCard(props, ref) {
    const {
      title = 'Extension Card',
      dataSource,
      className = '',
      style,
      defaultConfig = {},
      refreshInterval = 0,
      darkMode,
    } = props

    // State
    const [state, setState] = useState<ComponentState>({
      data: null,
      loading: true,
      error: null,
      lastUpdated: null,
    })

    // Get extension ID from data source or use default
    const extensionId = dataSource?.extensionId || 'your-extension-id'

    // Fetch data function
    const fetchData = useCallback(async () => {
      setState(prev => ({ ...prev, loading: true, error: null }))

      try {
        // Example: Fetch metrics from extension
        const data = await fetchExtensionData<Record<string, unknown>>(
          extensionId,
          dataSource?.command || 'get_status',
          { ...defaultConfig, ...dataSource?.args }
        )

        setState({
          data,
          loading: false,
          error: null,
          lastUpdated: new Date(),
        })
      } catch (err) {
        setState(prev => ({
          ...prev,
          loading: false,
          error: err instanceof Error ? err.message : 'Failed to fetch data',
        }))
      }
    }, [extensionId, dataSource, defaultConfig])

    // Initial fetch and refresh interval
    useEffect(() => {
      fetchData()

      if (refreshInterval > 0) {
        const interval = setInterval(fetchData, refreshInterval)
        return () => clearInterval(interval)
      }
    }, [fetchData, refreshInterval])

    // Render loading state
    if (state.loading && !state.data) {
      return (
        <div ref={ref} className={`ext-card ${className}`} style={style}>
          <style>{defaultStyles}</style>
          <div className="ext-card-container">
            <div className="ext-card-loading">
              <div className="ext-spinner" />
            </div>
          </div>
        </div>
      )
    }

    // Render error state
    if (state.error) {
      return (
        <div ref={ref} className={`ext-card ${className}`} style={style}>
          <style>{defaultStyles}</style>
          <div className="ext-card-container">
            <div className="ext-card-error">{state.error}</div>
          </div>
        </div>
      )
    }

    // Render main content
    return (
      <div ref={ref} className={`ext-card ${className}`} style={style}>
        <style>{defaultStyles}</style>
        <div className="ext-card-container">
          {/* Header */}
          <div className="ext-card-header">
            <h3 className="ext-card-title">{title}</h3>
            <button
              className="ext-refresh-btn"
              onClick={fetchData}
              disabled={state.loading}
              title="Refresh"
            >
              {RefreshIcon}
            </button>
          </div>

          {/* Content - Customize this for your extension */}
          <div className="ext-card-content">
            {state.data ? (
              <pre style={{ fontSize: 11, color: 'var(--ext-fg)', margin: 0, overflow: 'auto' }}>
                {JSON.stringify(state.data, null, 2)}
              </pre>
            ) : (
              <div className="ext-card-empty">No data available</div>
            )}
          </div>

          {/* Footer */}
          <div className="ext-card-footer">
            {state.lastUpdated && (
              <span>Updated: {state.lastUpdated.toLocaleTimeString()}</span>
            )}
          </div>
        </div>
      </div>
    )
  }
)

ExtensionCard.displayName = 'ExtensionCard'

// ============================================================================
// Component Metadata
// ============================================================================

export const componentMetadata = {
  name: 'ExtensionCard',
  type: 'card' as const,
  displayName: 'Extension Card',
  description: 'A customizable card component for extensions',
  defaultSize: { width: 300, height: 200 },
  minSize: { width: 200, height: 150 },
  maxSize: { width: 600, height: 400 },
}

// ============================================================================
// Default Export
// ============================================================================

export default {
  ExtensionCard,
  componentMetadata,
}
