/**
 * Video Display Component
 *
 * Video player for streams and files.
 * Supports HLS (.m3u8), MP4, and other video formats.
 * Compatible with camera feeds and video sources.
 */

import { getPortalRoot } from '@/lib/portal'
import { useState, useCallback, useRef, useEffect, useMemo } from 'react'
import { createPortal } from 'react-dom'
import type Hls from 'hls.js'
import { Skeleton } from '@/components/ui/skeleton'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

// Lazy-load hls.js (~300KB) only when video playback is needed
let hlsModule: typeof import('hls.js').default | null = null
async function loadHls() {
  if (!hlsModule) {
    const mod = await import('hls.js')
    hlsModule = mod.default
  }
  return hlsModule
}
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { useDataSource } from '@/hooks/useDataSource'
import {
  Play,
  Pause,
  Volume2,
  VolumeX,
  Maximize2,
  RefreshCw,
  Webcam,
  AlertCircle,
  X,
} from 'lucide-react'
import type { DataSource } from '@/types/dashboard'
import { EmptyState } from '../shared'

// ============================================================================
// Types
// ============================================================================

export type VideoSourceType = 'file' | 'hls' | 'device-camera'

export interface VideoDisplayProps {
  dataSource?: DataSource
  src?: string
  type?: VideoSourceType

  // Display options
  size?: 'sm' | 'md' | 'lg'
  autoplay?: boolean
  muted?: boolean
  controls?: boolean
  loop?: boolean
  fit?: 'contain' | 'cover' | 'fill'

  // Styling
  rounded?: boolean
  showFullscreen?: boolean

  className?: string
}

// ============================================================================
// Video Player Component
// ============================================================================

interface VideoPlayerProps {
  src: string
  type: VideoSourceType
  autoplay: boolean
  muted: boolean
  controls: boolean
  loop: boolean
  fit: string
  onLoadingChange: (loading: boolean) => void
  onError: (error: boolean, message?: string) => void
}

// HLS configuration - optimized for WKWebView performance
const createHlsConfig = () => ({
  enableWorker: true,
  lowLatencyMode: false,  // Disabled: reduces CPU usage, not needed for dashboard

  // Buffer configuration - reduced for lower memory/CPU footprint
  maxBufferLength: 15,
  maxMaxBufferLength: 300,
  maxBufferSize: 30 * 1000 * 1000,
  maxBufferHole: 0.5,

  // Force key frame on discontinuity
  forceKeyFrameOnDiscontinuity: true,

  // Handle video integrity errors
  handleMpegTsVideoIntegrityErrors: 'skip',

  // Append error retry
  appendErrorMaxRetry: 3,
  
  // Increase retry counts for better reliability
  fragLoadPolicy: {
    default: {
      maxTimeToFirstByteMs: 10000,
      maxLoadTimeMs: 120000,
      timeoutRetry: {
        maxNumRetry: 6,
        retryDelayMs: 0,
        maxRetryDelayMs: 0,
      },
      errorRetry: {
        maxNumRetry: 6,
        retryDelayMs: 1000,
        maxRetryDelayMs: 8000,
      },
    },
  },
  manifestLoadPolicy: {
    default: {
      maxTimeToFirstByteMs: 10000,
      maxLoadTimeMs: 20000,
      timeoutRetry: {
        maxNumRetry: 4,
        retryDelayMs: 0,
        maxRetryDelayMs: 0,
      },
      errorRetry: {
        maxNumRetry: 4,
        retryDelayMs: 1000,
        maxRetryDelayMs: 8000,
      },
    },
  },
})

function VideoPlayer({
  src,
  type,
  autoplay,
  muted,
  controls,
  loop,
  fit,
  onLoadingChange,
  onError,
}: VideoPlayerProps) {
  const videoRef = useRef<HTMLVideoElement>(null)
  const hlsRef = useRef<Hls | null>(null)
  const isDestroyingRef = useRef(false)
  const recoveryAttemptsRef = useRef(0)
  const playPromiseRef = useRef<Promise<void> | null>(null)
  const [isPlaying, setIsPlaying] = useState(false)
  const [isMuted, setIsMuted] = useState(muted)
  const [currentTime, setCurrentTime] = useState(0)
  const [duration, setDuration] = useState(0)
  const [isSeeking, setIsSeeking] = useState(false)

  // Sync isMuted when muted prop changes
  useEffect(() => {
    setIsMuted(muted)
  }, [muted])

  // Cleanup function that properly handles pending play promises
  const cleanupHls = useCallback(async () => {
    const hls = hlsRef.current
    const video = videoRef.current

    if (!hls) return

    // Mark as destroying to prevent new operations
    isDestroyingRef.current = true

    // Wait for any pending play promise to settle
    if (playPromiseRef.current) {
      try {
        await playPromiseRef.current
      } catch {
        // Ignore errors from interrupted play
      }
      playPromiseRef.current = null
    }

    // Pause video before detaching to prevent AbortError
    if (video && !video.paused) {
      video.pause()
    }

    // Small delay to ensure pause is processed
    await new Promise(resolve => setTimeout(resolve, 50))

    // Now safe to destroy
    try {
      hls.destroy()
    } catch {
      // Ignore destroy errors
    }
    hlsRef.current = null
    isDestroyingRef.current = false
  }, [])

  // Initialize HLS if needed
  useEffect(() => {
    const video = videoRef.current
    if (!video || !src) return

    // Flag to track if this effect is still active
    let isCancelled = false

    const initHls = async () => {
      // Cleanup previous instance first
      await cleanupHls()

      if (isCancelled) return

      if (type === 'hls') {
        // Lazy-load hls.js only when needed
        const Hls = await loadHls()
        if (isCancelled) return

        // Always use hls.js for better compatibility
        if (Hls.isSupported()) {
          const hls = new Hls(createHlsConfig())
          hlsRef.current = hls
          recoveryAttemptsRef.current = 0

          hls.loadSource(src)
          hls.attachMedia(video)

          hls.on(Hls.Events.MANIFEST_PARSED, () => {
            if (isCancelled || isDestroyingRef.current) return
            onLoadingChange(false)
            if (autoplay && video && !video.paused) {
              playPromiseRef.current = video.play().catch((e) => {
                console.warn('[HLS] Autoplay prevented:', e)
              })
            }
          })

          hls.on(Hls.Events.ERROR, (_event, data) => {
            if (!data.fatal || isCancelled || isDestroyingRef.current) {
              return
            }

            // For bufferAppendError - need special handling
            if (data.details === 'bufferAppendError') {
              recoveryAttemptsRef.current++

              if (recoveryAttemptsRef.current <= 3) {
                return
              }

              // Max attempts reached, rebuild HLS instance
              rebuildHls()
              return
            }

            switch (data.type) {
              case Hls.ErrorTypes.NETWORK_ERROR:
                hls.startLoad()
                break
              case Hls.ErrorTypes.MEDIA_ERROR:
                hls.recoverMediaError()
                break
              default:
                onError(true, `HLS Error: ${data.details}`)
                break
            }
          })

        } else if (video.canPlayType('application/vnd.apple.mpegurl')) {
          // Fallback to native HLS only if hls.js is not supported
          video.src = src

          const handleLoadedMetadata = () => {
            if (isCancelled) return
            onLoadingChange(false)
            if (autoplay) {
              playPromiseRef.current = video.play().catch(() => {})
            }
          }

          video.addEventListener('loadedmetadata', handleLoadedMetadata)

          return () => {
            video.removeEventListener('loadedmetadata', handleLoadedMetadata)
            video.src = ''
            video.load()
          }
        } else {
          onError(true, 'HLS is not supported in this browser')
        }
      } else {
        // Regular video file
        video.src = src
      }
    }

    // Rebuild HLS instance after error
    const rebuildHls = async () => {
      if (isCancelled || isDestroyingRef.current) return

      const video = videoRef.current
      if (!video) return

      const Hls = await loadHls()
      if (!Hls || !Hls.isSupported() || isCancelled) return

      await cleanupHls()
      if (isCancelled) return

      // Don't reset recoveryAttemptsRef to prevent infinite rebuild loops
      // The counter will be reset only when src changes (via useEffect dependency)
      const newHls = new Hls(createHlsConfig())
      hlsRef.current = newHls

      newHls.loadSource(src)
      newHls.attachMedia(video)

      newHls.on(Hls.Events.MANIFEST_PARSED, () => {
        if (isCancelled || isDestroyingRef.current) return
        playPromiseRef.current = video.play().catch(() => {})
      })

      // Also handle errors on the new instance
      newHls.on(Hls.Events.ERROR, (_event, data) => {
        if (!data.fatal || isCancelled || isDestroyingRef.current) {
          return
        }

        if (data.details === 'bufferAppendError') {
          recoveryAttemptsRef.current++

          if (recoveryAttemptsRef.current > 6) {
            onError(true, `HLS Error: ${data.details} (max recovery attempts reached)`)
          }
          return
        }

        switch (data.type) {
          case Hls.ErrorTypes.NETWORK_ERROR:
            newHls.startLoad()
            break
          case Hls.ErrorTypes.MEDIA_ERROR:
            newHls.recoverMediaError()
            break
          default:
            onError(true, `HLS Error: ${data.details}`)
            break
        }
      })
    }

    initHls()

    return () => {
      isCancelled = true
      cleanupHls()
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [src, type, cleanupHls])

  // Video event handlers - simplified
  useEffect(() => {
    const video = videoRef.current
    if (!video) return

    const handleLoadStart = () => onLoadingChange(true)
    const handleCanPlay = () => onLoadingChange(false)
    const handlePlay = () => setIsPlaying(true)
    const handlePause = () => setIsPlaying(false)

    // Throttle time updates to 4fps — 60fps setState causes massive main thread pressure
    let lastTimeUpdate = 0
    const handleTimeUpdate = () => {
      if (isSeeking) return
      const now = performance.now()
      if (now - lastTimeUpdate < 250) return
      lastTimeUpdate = now
      setCurrentTime(video.currentTime)
    }

    const handleLoadedMetadata = () => setDuration(video.duration)
    const handleEnded = () => {
      // For live streams, try to reconnect
      if (type === 'hls' && hlsRef.current && !isDestroyingRef.current) {
        hlsRef.current.startLoad(-1)
        playPromiseRef.current = video.play().catch(() => {})
      }
    }

    video.addEventListener('loadstart', handleLoadStart)
    video.addEventListener('canplay', handleCanPlay)
    video.addEventListener('play', handlePlay)
    video.addEventListener('pause', handlePause)
    video.addEventListener('timeupdate', handleTimeUpdate)
    video.addEventListener('loadedmetadata', handleLoadedMetadata)
    video.addEventListener('ended', handleEnded)

    return () => {
      video.removeEventListener('loadstart', handleLoadStart)
      video.removeEventListener('canplay', handleCanPlay)
      video.removeEventListener('play', handlePlay)
      video.removeEventListener('pause', handlePause)
      video.removeEventListener('timeupdate', handleTimeUpdate)
      video.removeEventListener('loadedmetadata', handleLoadedMetadata)
      video.removeEventListener('ended', handleEnded)
    }
  }, [onLoadingChange, isSeeking, type])

  // Autoplay for non-HLS
  useEffect(() => {
    if (type !== 'hls' && autoplay && videoRef.current && !isDestroyingRef.current) {
      playPromiseRef.current = videoRef.current.play().catch(() => {})
    }
  }, [autoplay, type])

  // Handle visibility change - resume playback when tab becomes visible
  useEffect(() => {
    const handleVisibilityChange = () => {
      const video = videoRef.current
      const hls = hlsRef.current

      if (document.visibilityState === 'visible' && video && video.paused && !video.ended && !isDestroyingRef.current) {
        // For HLS live streams, jump to live edge
        if (type === 'hls' && hls?.liveSyncPosition) {
          video.currentTime = hls.liveSyncPosition
        }
        playPromiseRef.current = video.play().catch(() => {})
      }
    }

    document.addEventListener('visibilitychange', handleVisibilityChange)
    return () => document.removeEventListener('visibilitychange', handleVisibilityChange)
  }, [type])

  // HLS live stream keep-alive - periodically check and recover
  useEffect(() => {
    if (type !== 'hls') return

    const keepAliveInterval = setInterval(() => {
      const video = videoRef.current
      const hls = hlsRef.current

      if (!video || !hls || isDestroyingRef.current) return

      // Check if we're supposed to be playing but aren't
      if (!video.paused && video.readyState < 3) {
        // If we have a live sync position and we're too far behind, jump forward
        if (hls.liveSyncPosition !== null && hls.liveSyncPosition !== undefined && video.currentTime < hls.liveSyncPosition - 10) {
          video.currentTime = hls.liveSyncPosition
        }
      }
    }, 5000)

    return () => {
      clearInterval(keepAliveInterval)
    }
  }, [type])

  const togglePlay = useCallback(() => {
    if (!videoRef.current || isDestroyingRef.current) return
    if (isPlaying) {
      videoRef.current.pause()
    } else {
      playPromiseRef.current = videoRef.current.play().catch(() => {})
    }
  }, [isPlaying])

  const toggleMute = useCallback(() => {
    if (!videoRef.current) return
    videoRef.current.muted = !videoRef.current.muted
    setIsMuted(videoRef.current.muted)
  }, [])

  const handleSeek = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    if (!videoRef.current) return
    const time = parseFloat(e.target.value)
    videoRef.current.currentTime = time
    setCurrentTime(time)
  }, [])

  const formatTime = (seconds: number) => {
    const mins = Math.floor(seconds / 60)
    const secs = Math.floor(seconds % 60)
    return `${mins}:${secs.toString().padStart(2, '0')}`
  }

  return (
    <div className="relative w-full h-full bg-black flex items-center justify-center">
      <video
        ref={videoRef}
        autoPlay={autoplay}
        muted={isMuted}
        loop={loop}
        playsInline
        className={cn(
          'w-full h-full',
          fit === 'contain' && 'object-contain',
          fit === 'cover' && 'object-cover',
          fit === 'fill' && 'object-fill'
        )}
      />

      {/* Custom controls */}
      {controls && (
        <div className="absolute bottom-0 left-0 right-0 bg-gradient-to-t from-black/80 to-transparent p-3 opacity-0 hover:opacity-100 transition-opacity">
          {/* Progress bar */}
          <input
            type="range"
            min="0"
            max={duration || 100}
            value={currentTime}
            onChange={handleSeek}
            onMouseDown={() => setIsSeeking(true)}
            onMouseUp={() => setIsSeeking(false)}
            className="w-full h-1 bg-white/30 rounded-full appearance-none cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-3 [&::-webkit-slider-thumb]:h-3 [&::-webkit-slider-thumb]:bg-white [&::-webkit-slider-thumb]:rounded-full"
          />

          <div className="flex items-center justify-between mt-2">
            <div className="flex items-center gap-2">
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 text-white hover:text-white hover:bg-white/20"
                onClick={togglePlay}
              >
                {isPlaying ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4" />}
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 text-white hover:text-white hover:bg-white/20"
                onClick={toggleMute}
              >
                {isMuted ? <VolumeX className="h-4 w-4" /> : <Volume2 className="h-4 w-4" />}
              </Button>
              <span className="text-white text-xs">
                {formatTime(currentTime)} / {formatTime(duration)}
              </span>
            </div>
          </div>
        </div>
      )}

      {/* Play overlay for paused state */}
      {!isPlaying && (
        <div className="absolute inset-0 flex items-center justify-center">
          <Button
            variant="secondary"
            size="icon"
            className="h-12 w-12 rounded-full bg-white/20 hover:bg-white/30"
            onClick={togglePlay}
          >
            <Play className="h-6 w-6 text-white ml-0.5" />
          </Button>
        </div>
      )}
    </div>
  )
}

// ============================================================================
// Camera Access Component
// ============================================================================

interface CameraAccessProps {
  onStreamReady: (stream: MediaStream) => void
  onError: () => void
}

function CameraAccess({ onStreamReady, onError }: CameraAccessProps) {
  const videoRef = useRef<HTMLVideoElement>(null)
  const streamRef = useRef<MediaStream | null>(null)

  useEffect(() => {
    let cancelled = false
    navigator.mediaDevices.getUserMedia({ video: true })
      .then((stream) => {
        if (cancelled) {
          stream.getTracks().forEach(t => t.stop())
          return
        }
        streamRef.current = stream
        if (videoRef.current) {
          videoRef.current.srcObject = stream
          onStreamReady(stream)
        }
      })
      .catch(() => {
        if (!cancelled) onError()
      })
    return () => {
      cancelled = true
      if (streamRef.current) {
        streamRef.current.getTracks().forEach(t => t.stop())
        streamRef.current = null
      }
    }
  }, [onStreamReady, onError])

  return (
    <video
      ref={videoRef}
      autoPlay
      muted
      playsInline
      className="w-full h-full object-cover"
    />
  )
}

// ============================================================================
// Main Component
// ============================================================================

// Stable key for VideoPlayer - only changes when src or type changes
const getVideoPlayerKey = (src: string | undefined, type: VideoSourceType) => {
  return `video-${src || 'no-src'}-${type}`
}

export function VideoDisplay({
  dataSource,
  src: propSrc,
  type = 'file',
  size = 'md',
  autoplay = false,
  muted = true,
  controls = true,
  loop = false,
  fit = 'contain',
  rounded = true,
  showFullscreen = true,
  className,
  title,
}: VideoDisplayProps & { title?: string }) {
  const { data, loading, error } = useDataSource<string>(dataSource, {
    fallback: propSrc,
  })

  // Safely convert data to string
  const rawSrc = useMemo(() => {
    if (error) return propSrc ?? ''

    if (data === undefined || data === null) return propSrc ?? ''

    if (typeof data === 'string') return (data ?? '') || (propSrc ?? '')

    if (Array.isArray(data)) {
      const firstItem = data[0]
      if (typeof firstItem === 'string') return (firstItem ?? '') || (propSrc ?? '')
      return String(firstItem ?? propSrc ?? '')
    }

    const converted = String(data)
    return (converted ?? '') || (propSrc ?? '')
  }, [error, data, propSrc])

  const [isLoading, setIsLoading] = useState(true)
  const [hasError, setHasError] = useState(false)
  const [errorMessage, setErrorMessage] = useState<string>('')
  const [isFullscreen, setIsFullscreen] = useState(false)
  const [retryKey, setRetryKey] = useState(0)

  const sizeConfig = dashboardComponentSize[size]

  const handleRetry = useCallback(() => {
    setRetryKey(prev => prev + 1)
    setHasError(false)
    setErrorMessage('')
    setIsLoading(true)
  }, [])

  const handleVideoError = useCallback((error: boolean, message?: string) => {
    setHasError(error)
    if (message) setErrorMessage(message)
  }, [])

  // Detect video type from URL if not explicitly set
  const detectedType = type !== 'file' ? type : (() => {
    if (!rawSrc) return 'file'
    if (rawSrc.includes('.m3u8')) return 'hls'
    if (rawSrc.startsWith('camera:') || rawSrc.startsWith('device:camera')) return 'device-camera'
    return 'file'
  })()

  // No source configured
  if (!rawSrc && !dataSource) {
    return (
      <EmptyState
        size={size}
        className={className}
        icon={<Webcam />}
        message="No Video Source"
        subMessage="Configure a video URL or camera"
      />
    )
  }

  // Loading state from data source. Only replace the card before the first
  // usable source arrives; refreshes should keep the existing video surface.
  if (loading && isLoading && !rawSrc) {
    return (
      <div className={cn(dashboardCardBase, 'h-full flex items-center justify-center', sizeConfig.padding, className)}>
        <Skeleton className={cn('w-full h-full', rounded && 'rounded-lg')} />
      </div>
    )
  }

  // Error state
  if (hasError || (error && !rawSrc)) {
    return (
      <div className={cn(
        dashboardCardBase,
        'flex flex-col items-center justify-center gap-3 bg-muted-30',
        sizeConfig.padding,
        className
      )}>
        <AlertCircle className="h-10 w-10 text-muted-foreground" />
        <div className="text-center">
          <p className="text-muted-foreground text-sm font-medium">Video Load Error</p>
          <p className="text-muted-foreground text-xs mt-1">
            {errorMessage || 'Could not load video source'}
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={handleRetry}>
          <RefreshCw className="h-4 w-4 mr-1.5" />
          Retry
        </Button>
      </div>
    )
  }

  const content = (
    <>
      {/* Normal view */}
      <div className={cn(dashboardCardBase, 'relative overflow-hidden flex flex-col min-h-[200px]', className)}>
        {/* Video content */}
        <div className="flex-1 relative bg-black w-full min-h-[200px]">
          {detectedType === 'device-camera' ? (
            <CameraAccess
              key={retryKey}
              onStreamReady={() => setIsLoading(false)}
              onError={() => setHasError(true)}
            />
          ) : (
            <VideoPlayer
              key={getVideoPlayerKey(rawSrc, detectedType)}
              src={rawSrc || ''}
              type={detectedType}
              autoplay={autoplay}
              muted={muted}
              controls={controls}
              loop={loop}
              fit={fit}
              onLoadingChange={setIsLoading}
              onError={handleVideoError}
            />
          )}
        </div>

        {/* Fullscreen toggle */}
        {showFullscreen && !isFullscreen && (
          <Button
            variant="secondary"
            size="icon"
            className="absolute top-2 right-2 h-6 w-6 bg-bg-80"
            onClick={() => setIsFullscreen(true)}
          >
            <Maximize2 className="h-4 w-4" />
          </Button>
        )}

        {/* Type indicator */}
        {rawSrc && (
          <div className="absolute top-2 left-2 px-2 py-0.5 bg-bg-80 rounded text-xs text-muted-foreground">
            {detectedType === 'hls' && 'HLS'}
            {detectedType === 'device-camera' && 'Camera'}
            {detectedType === 'file' && 'Video'}
          </div>
        )}
      </div>
    </>
  )

  // Fullscreen overlay
  const fullscreenOverlay = isFullscreen && createPortal(
    <div className="fixed inset-0 z-[110] bg-black flex flex-col">
      {/* Header with close button */}
      <div className="flex items-center justify-between px-4 py-3 bg-bg-95 border-b">
        <div className="flex items-center gap-2">
          <Webcam className="h-4 w-4 text-muted-foreground" />
          <span className="text-sm font-medium">{title || 'Video'}</span>
          <span className="text-xs text-muted-foreground">
            {detectedType === 'hls' && 'HLS'}
            {detectedType === 'device-camera' && 'Camera'}
            {detectedType === 'file' && 'Video'}
          </span>
        </div>
        <Button
          variant="ghost"
          size="icon"
          className="h-8 w-8"
          onClick={() => setIsFullscreen(false)}
        >
          <X className="h-4 w-4" />
        </Button>
      </div>

      {/* Video content */}
      <div className="flex-1 relative">
        {detectedType === 'device-camera' ? (
          <CameraAccess
            key={retryKey}
            onStreamReady={() => setIsLoading(false)}
            onError={() => setHasError(true)}
          />
        ) : (
          <VideoPlayer
            key={getVideoPlayerKey(rawSrc, detectedType)}
            src={rawSrc || ''}
            type={detectedType}
            autoplay={autoplay}
            muted={muted}
            controls={controls}
            loop={loop}
            fit={fit}
            onLoadingChange={setIsLoading}
            onError={handleVideoError}
          />
        )}
      </div>
    </div>, getPortalRoot()
  )

  return (
    <>
      {/* Normal view (hidden when fullscreen to avoid dual rendering) */}
      {!isFullscreen && content}
      {fullscreenOverlay}
    </>
  )
}
