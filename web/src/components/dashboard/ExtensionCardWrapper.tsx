/**
 * Extension Card Wrapper Component
 *
 * A wrapper component for displaying extension-provided dashboard cards.
 * Handles dynamic loading, error states, and provides a consistent interface.
 */

import * as React from 'react'
import { useExtensionComponent } from '@/lib/extension-component-loader'
import { Loader2 } from 'lucide-react'

/**
 * Props for ExtensionCardWrapper
 */
export interface ExtensionCardWrapperProps {
  /** Extension ID */
  extensionId: string
  /** Component name (type) */
  componentName: string
  /** Card title */
  title?: string
  /** Additional CSS classes */
  className?: string
  /** Props to pass to the component */
  componentProps?: Record<string, unknown>
  /** Loading component */
  loadingComponent?: React.ReactNode
  /** Error component */
  errorComponent?: React.ReactNode
  /** Empty component (when component is null) */
  emptyComponent?: React.ReactNode
  /** Callback when component loads successfully */
  onLoad?: () => void
  /** Callback when component fails to load */
  onError?: (error: Error) => void
}

/**
 * Default loading component
 */
const DefaultLoadingComponent: React.FC<{ className?: string }> = ({ className = '' }) => (
  <div className={`extension-card-loading flex flex-col items-center justify-center p-8 ${className}`}>
    <Loader2 className="w-8 h-8 animate-spin text-blue-500 mb-4" />
    <p className="text-sm text-gray-600 dark:text-gray-400">Loading extension component...</p>
  </div>
)

/**
 * Default error component
 */
const DefaultErrorComponent: React.FC<{ error?: Error; className?: string }> = ({ error, className = '' }) => (
  <div className={`extension-card-error p-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg ${className}`}>
    <p className="text-sm font-semibold text-red-800 dark:text-red-200">
      Failed to load extension component
    </p>
    {error?.message && (
      <p className="text-xs text-red-600 dark:text-red-400 mt-1">{error.message}</p>
    )}
  </div>
)

/**
 * Extension Card Wrapper
 *
 * @example
 * ```tsx
 * <ExtensionCardWrapper
 *   extensionId="weather-forecast-v2"
 *   componentName="weather-card"
 *   title="Weather Forecast"
 *   className="col-span-2 row-span-2"
 *   componentProps={{ city: 'San Francisco' }}
 *   onLoad={() => console.log('Loaded!')}
 *   onError={(err) => console.error(err)}
 * />
 * ```
 */
export const ExtensionCardWrapper: React.FC<ExtensionCardWrapperProps> = ({
  extensionId,
  componentName,
  title,
  className = '',
  componentProps = {},
  loadingComponent,
  errorComponent,
  emptyComponent,
  onLoad,
  onError,
}) => {
  const { component, loading, error } = useExtensionComponent(extensionId, componentName)

  // Handle load callback
  React.useEffect(() => {
    if (component && !loading && !error) {
      onLoad?.()
    }
  }, [component, loading, error, onLoad])

  // Handle error callback
  React.useEffect(() => {
    if (error) {
      onError?.(error)
    }
  }, [error, onError])

  // Loading state
  if (loading) {
    return <>{loadingComponent || <DefaultLoadingComponent className={className} />}</>
  }

  // Error state
  if (error) {
    return <>{errorComponent || <DefaultErrorComponent error={error} className={className} />}</>
  }

  // Empty state (component is null)
  if (!component) {
    return <>{emptyComponent || null}</>
  }

  // Render component
  try {
    const Component = component
    return (
      <div className={`extension-card-wrapper ${className}`}>
        <Component title={title} {...componentProps} />
      </div>
    )
  } catch (renderError) {
    // Fallback for rendering errors
    const err = renderError instanceof Error ? renderError : new Error(String(renderError))
    console.error('[ExtensionCardWrapper] Render error:', err)
    return <>{errorComponent || <DefaultErrorComponent error={err} className={className} />}</>
  }
}

/**
 * HOC for creating a typed extension card component
 *
 * @example
 * ```tsx
 * interface WeatherCardProps {
 *   city: string
 *   units?: 'metric' | 'imperial'
 * }
 *
 * const WeatherCard = createExtensionCardComponent<WeatherCardProps>({
 *   extensionId: 'weather-forecast-v2',
 *   componentName: 'weather-card',
 * })
 *
 * // Usage
 * <WeatherCard city="San Francisco" units="metric" />
 * ```
 */
export function createExtensionCardComponent<TProps extends Record<string, unknown> = Record<string, unknown>>(options: {
  extensionId: string
  componentName: string
  defaultTitle?: string
  defaultClassName?: string
}) {
  const {
    extensionId,
    componentName,
    defaultTitle,
    defaultClassName,
  } = options

  return React.forwardRef<any, TProps & { title?: string; className?: string }>((props, ref) => {
    const { title, className, ...componentProps } = props

    return (
      <ExtensionCardWrapper
        extensionId={extensionId}
        componentName={componentName}
        title={(title as string | undefined) || defaultTitle}
        className={(className as string | undefined) || (defaultClassName as string | undefined) || ''}
        componentProps={componentProps}
      />
    )
  })
}

export default ExtensionCardWrapper
