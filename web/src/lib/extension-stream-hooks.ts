/**
 * Extension Stream React Hooks
 *
 * React hooks for interacting with extension streaming functionality.
 * Provides convenient wrappers around ExtensionStreamClient with automatic
 * cleanup, state management, and error handling.
 */

import * as React from 'react'
import { getExtensionStreamClient, closeExtensionStreamClient, type ExtensionStreamClient } from './extension-stream'
import type {
  ExtensionStreamConnectionState,
  ExtensionSessionStats,
  StreamCapability,
} from '@/types'

/**
 * Configuration for useExtensionStream hook
 */
export interface UseExtensionStreamOptions {
  /** Auto-connect on mount */
  autoConnect?: boolean
  /** Stream configuration (passed to initSession) */
  config?: Record<string, unknown>
  /** Callback when connection state changes */
  onConnectionChange?: (state: ExtensionStreamConnectionState) => void
  /** Callback when data is received */
  onData?: (data: Uint8Array, dataType: string, sequence: number) => void
  /** Callback when session is closed */
  onSessionClosed?: (stats: ExtensionSessionStats) => void
  /** Callback on error */
  onError?: (error: Error) => void
}

/**
 * Hook for extension stream management
 *
 * @example
 * ```tsx
 * function MyComponent() {
 *   const {
 *     state,
 *     capability,
 *     sessionId,
 *     isConnected,
 *     connect,
 *     disconnect,
 *     sendChunk,
 *     close
 *   } = useExtensionStream('video-processor', {
 *     autoConnect: true,
 *     config: { mode: 'realtime' },
 *     onData: (data, type) => console.log('Received:', type, data.length)
 *   })
 *
 *   return (
 *     <div>
 *       Status: {state.status}
 *       <button onClick={() => sendChunk(newData)}>Send Data</button>
 *     </div>
 *   )
 * }
 * ```
 */
export function useExtensionStream(
  extensionId: string,
  options: UseExtensionStreamOptions = {}
) {
  const {
    autoConnect = false,
    config,
    onConnectionChange,
    onData,
    onSessionClosed,
    onError,
  } = options

  const clientRef = React.useRef<ExtensionStreamClient | null>(null)

  // Initialize client
  if (!clientRef.current) {
    clientRef.current = getExtensionStreamClient(extensionId)
  }

  const [state, setState] = React.useState<ExtensionStreamConnectionState>(
    clientRef.current.getState()
  )

  // Sync connection state
  React.useEffect(() => {
    const client = clientRef.current
    if (!client) return

    const unsubscribe = client.onConnection((newState) => {
      setState(newState)
      onConnectionChange?.(newState)
    })

    return unsubscribe
  }, [extensionId, onConnectionChange])

  // Setup data handler
  React.useEffect(() => {
    const client = clientRef.current
    if (!client || !onData) return

    const unsubscribe = client.onResult(onData)
    return unsubscribe
  }, [extensionId, onData])

  // Setup session closed handler
  React.useEffect(() => {
    const client = clientRef.current
    if (!client || !onSessionClosed) return

    const unsubscribe = client.onSessionClosed(onSessionClosed)
    return unsubscribe
  }, [extensionId, onSessionClosed])

  // Setup error handler
  React.useEffect(() => {
    const client = clientRef.current
    if (!client || !onError) return

    const unsubscribe = client.onError(onError)
    return unsubscribe
  }, [extensionId, onError])

  // Auto-connect
  React.useEffect(() => {
    if (autoConnect) {
      clientRef.current?.connect(config)
    }
  }, [autoConnect])

  // Cleanup on unmount
  React.useEffect(() => {
    return () => {
      // Don't close the client completely, just disconnect
      // This allows the client to be reused
      clientRef.current?.disconnect()
    }
  }, [extensionId])

  const client = clientRef.current!

  return {
    state,
    capability: client.getCapability(),
    sessionId: client.getSessionId(),
    isConnected: client.isConnected(),

    connect: (newConfig?: Record<string, unknown>) => client.connect(newConfig || config),
    disconnect: () => client.disconnect(),
    initSession: (sessionConfig?: Record<string, unknown>) => client.initSession(sessionConfig),
    sendChunk: (data: ArrayBuffer | Uint8Array, sequence: number) => client.sendChunk(data, sequence),
    sendAck: (sequence: number) => client.sendAck(sequence),
    close: () => client.close(),
  }
}

/**
 * Hook for image analyzer extensions
 *
 * @example
 * ```tsx
 * function ImageAnalyzer() {
 *   const { analyzing, error, analyzeImage, result } = useImageAnalyzer('image-analyzer-v2')
 *
 *   return (
 *     <div>
 *       <input type="file" onChange={(e) => {
 *         const file = e.target.files?.[0]
 *         if (file) analyzeImage(file)
 *       }} />
 *       {analyzing && <p>Analyzing...</p>}
 *       {result && <img src={`data:image/jpeg;base64,${result}`} />}
 *     </div>
 *   )
 * }
 * ```
 */
export function useImageAnalyzer(extensionId: string) {
  const [analyzing, setAnalyzing] = React.useState(false)
  const [result, setResult] = React.useState<Uint8Array | null>(null)
  const [error, setError] = React.useState<Error | null>(null)

  const stream = useExtensionStream(extensionId, {
    onData: (data, dataType) => {
      if (dataType === 'image') {
        setResult(data)
        setAnalyzing(false)
      }
    },
    onError: (err) => {
      setError(err)
      setAnalyzing(false)
    },
  })

  const analyzeImage = React.useCallback(
    async (imageData: ArrayBuffer | Uint8Array | File): Promise<void> => {
      setAnalyzing(true)
      setError(null)
      setResult(null)

      try {
        let data: ArrayBuffer | Uint8Array

        if (imageData instanceof File) {
          data = await imageData.arrayBuffer()
        } else {
          data = imageData
        }

        stream.sendChunk(data, 0)
      } catch (err) {
        const error = err instanceof Error ? err : new Error(String(err))
        setError(error)
        setAnalyzing(false)
      }
    },
    [stream]
  )

  return {
    analyzing,
    result,
    error,
    analyzeImage,
    isConnected: stream.isConnected,
    connect: stream.connect,
    disconnect: stream.disconnect,
  }
}

/**
 * Hook for video processor extensions
 *
 * @example
 * ```tsx
 * function VideoProcessor() {
 *   const { processing, stats, startProcessing, stopProcessing, sendFrame } = useVideoProcessor('yolo-v2')
 *
 *   return (
 *     <div>
 *       <button onClick={() => startProcessing({ confidence: 0.5 })}>Start</button>
 *       <button onClick={stopProcessing}>Stop</button>
 *       {processing && <p>Processing... Frames: {stats?.chunks_processed}</p>}
 *     </div>
 *   )
 * }
 * ```
 */
export function useVideoProcessor(extensionId: string) {
  const [processing, setProcessing] = React.useState(false)
  const [stats, setStats] = React.useState<ExtensionSessionStats | null>(null)
  const [error, setError] = React.useState<Error | null>(null)

  const stream = useExtensionStream(extensionId, {
    onData: (data, dataType) => {
      if (dataType === 'video') {
        // Handle processed frame
        console.log('Received processed frame:', data.length)
      }
    },
    onSessionClosed: (sessionStats) => {
      setStats(sessionStats)
      setProcessing(false)
    },
    onError: (err) => {
      setError(err)
      setProcessing(false)
    },
  })

  const startProcessing = React.useCallback(
    (config?: Record<string, unknown>) => {
      stream.connect(config)
      stream.initSession(config)
      setProcessing(true)
      setError(null)
      setStats(null)
    },
    [stream]
  )

  const stopProcessing = React.useCallback(() => {
    stream.close()
    setProcessing(false)
  }, [stream])

  const sendFrame = React.useCallback(
    (frameData: ArrayBuffer | Uint8Array, sequence: number) => {
      if (!processing) {
        console.warn('[useVideoProcessor] Not processing, frame not sent')
        return
      }
      stream.sendChunk(frameData, sequence)
    },
    [stream, processing]
  )

  return {
    processing,
    stats,
    error,
    startProcessing,
    stopProcessing,
    sendFrame,
    isConnected: stream.isConnected,
    connect: stream.connect,
    disconnect: stream.disconnect,
  }
}

/**
 * Hook for sensor stream extensions
 *
 * @example
 * ```tsx
 * function SensorMonitor() {
 *   const { streaming, dataPoints, startStreaming, stopStreaming } = useSensorStream('sensor-collector')
 *
 *   return (
 *     <div>
 *       <button onClick={() => startStreaming({ interval: 100 })}>Start</button>
 *       <button onClick={stopStreaming}>Stop</button>
 *       <ul>
 *         {dataPoints.map((dp, i) => <li key={i}>{dp.value}</li>)}
 *       </ul>
 *     </div>
 *   )
 * }
 * ```
 */
export function useSensorStream(extensionId: string) {
  const [streaming, setStreaming] = React.useState(false)
  const [dataPoints, setDataPoints] = React.useState<Array<{ timestamp: number; value: number }>>([])
  const [error, setError] = React.useState<Error | null>(null)

  const stream = useExtensionStream(extensionId, {
    onData: (data, dataType) => {
      if (dataType === 'sensor') {
        try {
          // Decode sensor data (assuming JSON for now)
          const text = new TextDecoder().decode(data)
          const value = JSON.parse(text)

          setDataPoints(prev => [
            ...prev,
            { timestamp: Date.now(), value }
          ])
        } catch (err) {
          console.error('[useSensorStream] Failed to decode sensor data:', err)
        }
      }
    },
    onError: (err) => {
      setError(err)
      setStreaming(false)
    },
  })

  const startStreaming = React.useCallback(
    (config?: Record<string, unknown>) => {
      stream.connect(config)
      stream.initSession(config)
      setStreaming(true)
      setError(null)
      setDataPoints([])
    },
    [stream]
  )

  const stopStreaming = React.useCallback(() => {
    stream.close()
    setStreaming(false)
  }, [stream])

  const clearData = React.useCallback(() => {
    setDataPoints([])
  }, [])

  return {
    streaming,
    dataPoints,
    error,
    startStreaming,
    stopStreaming,
    clearData,
    isConnected: stream.isConnected,
    connect: stream.connect,
    disconnect: stream.disconnect,
  }
}

/**
 * Hook for managing extension stream capability
 *
 * @example
 * ```tsx
 * function CapabilityDisplay() {
 *   const { capability, supportedModes, hasMode } = useStreamCapability('video-processor')
 *
 *   return (
 *     <div>
 *       <h3>Capability</h3>
 *       <p>Mode: {capability?.mode}</p>
 *       <p>Data Types: {capability?.data_types.join(', ')}</p>
 *       {hasMode('stateful') && <p>✅ Supports stateful mode</p>}
 *     </div>
 *   )
 * }
 * ```
 */
export function useStreamCapability(extensionId: string): {
  capability: StreamCapability | null
  supportedModes: string[]
  supportedDataTypes: string[]
  hasMode: (mode: string) => boolean
  hasDataType: (dataType: string) => boolean
} {
  const { capability } = useExtensionStream(extensionId)

  const supportedModes = React.useMemo(
    () => capability?.supported_modes || [],
    [capability]
  )

  const supportedDataTypes = React.useMemo(
    () => capability?.data_types || [],
    [capability]
  )

  const hasMode = React.useCallback(
    (mode: string) => supportedModes.includes(mode),
    [supportedModes]
  )

  const hasDataType = React.useCallback(
    (dataType: string) => supportedDataTypes.includes(dataType),
    [supportedDataTypes]
  )

  return {
    capability,
    supportedModes,
    supportedDataTypes,
    hasMode,
    hasDataType,
  }
}

/**
 * Higher-order component that provides extension stream context
 *
 * @example
 * ```tsx
 * interface StreamContextValue {
 *   stream: ReturnType<typeof useExtensionStream>
 * }
 *
 * const MyComponent = withExtensionStream('video-processor', function MyComponent({ stream }) {
 *   return (
 *     <div>
 *       Status: {stream.state.status}
 *       <button onClick={() => stream.sendChunk(data)}>Send</button>
 *     </div>
 *   )
 * })
 * ```
 */
export function withExtensionStream<P extends Record<string, unknown>>(
  extensionId: string,
  Component: React.ComponentType<P & { stream: ReturnType<typeof useExtensionStream> }>,
  streamOptions?: UseExtensionStreamOptions
): React.ComponentType<P> {
  return function WithExtensionStreamWrapper(props: P) {
    const stream = useExtensionStream(extensionId, streamOptions)

    return <Component {...props} stream={stream} />
  }
}

/**
 * Hook for batch stream operations
 *
 * @example
 * ```tsx
 * function BatchProcessor() {
 *   const { streams, connectAll, disconnectAll } = useExtensionStreamBatch([
 *     'camera-1',
 *     'camera-2',
 *     'camera-3'
 *   ])
 *
 *   return (
 *     <div>
 *       <button onClick={connectAll}>Connect All</button>
 *       <button onClick={disconnectAll}>Disconnect All</button>
 *       {streams.map(s => (
 *         <div key={s.extensionId}>
 *           {s.extensionId}: {s.state.status}
 *         </div>
 *       ))}
 *     </div>
 *   )
 * }
 * ```
 */
export function useExtensionStreamBatch(
  extensionIds: string[],
  options?: UseExtensionStreamOptions
) {
  const streams = React.useMemo(
    () =>
      extensionIds.map(id => ({
        extensionId: id,
        ...useExtensionStream(id, options),
      })),
    [extensionIds, options]
  )

  const connectAll = React.useCallback(() => {
    streams.forEach(s => s.connect())
  }, [streams])

  const disconnectAll = React.useCallback(() => {
    streams.forEach(s => s.disconnect())
  }, [streams])

  const connectedCount = React.useMemo(
    () => streams.filter(s => s.isConnected).length,
    [streams]
  )

  return {
    streams,
    connectAll,
    disconnectAll,
    connectedCount,
    totalCount: streams.length,
    allConnected: connectedCount === streams.length,
  }
}
