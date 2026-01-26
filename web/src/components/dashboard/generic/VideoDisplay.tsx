/**
 * Video Display Component
 *
 * Video player for streams and files.
 * Supports RTSP, HLS, MP4, and other video formats.
 * Compatible with camera feeds and video sources.
 */

import { useState, useCallback, useRef, useEffect } from 'react'
import { Skeleton } from '@/components/ui/skeleton'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { useDataSource } from '@/hooks/useDataSource'
import {
  Play,
  Pause,
  Volume2,
  VolumeX,
  Maximize2,
  Minimize2,
  RefreshCw,
  Webcam,
  AlertCircle,
  Settings,
} from 'lucide-react'
import type { DataSource } from '@/types/dashboard'
import { EmptyState, ErrorState } from '../shared'

// ============================================================================
// Types
// ============================================================================

export type VideoSourceType = 'file' | 'stream' | 'rtsp' | 'rtmp' | 'hls' | 'webrtc' | 'device-camera'

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

  // Stream options
  refreshInterval?: number // For RTSP streams
  reconnectAttempts?: number
  reconnectDelay?: number

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
  onError: (error: boolean) => void
}

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
  const [isPlaying, setIsPlaying] = useState(false)
  const [isMuted, setIsMuted] = useState(muted)
  const [currentTime, setCurrentTime] = useState(0)
  const [duration, setDuration] = useState(0)
  const [isSeeking, setIsSeeking] = useState(false)

  // Sync isMuted when muted prop changes (e.g., after configuration update)
  useEffect(() => {
    setIsMuted(muted)
  }, [muted])

  useEffect(() => {
    const video = videoRef.current
    if (!video) return

    const handleLoadStart = () => onLoadingChange(true)
    const handleCanPlay = () => onLoadingChange(false)
    const handleError = () => onError(true)
    const handlePlay = () => setIsPlaying(true)
    const handlePause = () => setIsPlaying(false)
    const handleTimeUpdate = () => {
      if (!isSeeking) {
        setCurrentTime(video.currentTime)
      }
    }
    const handleLoadedMetadata = () => setDuration(video.duration)

    video.addEventListener('loadstart', handleLoadStart)
    video.addEventListener('canplay', handleCanPlay)
    video.addEventListener('error', handleError)
    video.addEventListener('play', handlePlay)
    video.addEventListener('pause', handlePause)
    video.addEventListener('timeupdate', handleTimeUpdate)
    video.addEventListener('loadedmetadata', handleLoadedMetadata)

    return () => {
      video.removeEventListener('loadstart', handleLoadStart)
      video.removeEventListener('canplay', handleCanPlay)
      video.removeEventListener('error', handleError)
      video.removeEventListener('play', handlePlay)
      video.removeEventListener('pause', handlePause)
      video.removeEventListener('timeupdate', handleTimeUpdate)
      video.removeEventListener('loadedmetadata', handleLoadedMetadata)
    }
  }, [onLoadingChange, onError, isSeeking])

  useEffect(() => {
    if (autoplay && videoRef.current) {
      videoRef.current.play().catch(() => {
        // Autoplay was prevented, user interaction required
      })
    }
  }, [autoplay])

  const togglePlay = useCallback(() => {
    if (!videoRef.current) return
    if (isPlaying) {
      videoRef.current.pause()
    } else {
      videoRef.current.play()
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
        src={src}
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
                className="h-7 w-7 text-white hover:text-white hover:bg-white/20"
                onClick={togglePlay}
              >
                {isPlaying ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4" />}
              </Button>
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-white hover:text-white hover:bg-white/20"
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
            className="h-12 w-12 rounded-full bg-white/20 backdrop-blur hover:bg-white/30"
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
// Stream Placeholder (for protocols browser doesn't support directly)
// ============================================================================

interface StreamPlaceholderProps {
  src: string
  streamType: 'rtsp' | 'rtmp' | 'webrtc'
  onRetry: () => void
}

function StreamPlaceholder({ src, streamType, onRetry }: StreamPlaceholderProps) {
  const streamInfo = {
    rtsp: { name: 'RTSP', desc: 'Direct RTSP playback requires a proxy or transcoder' },
    rtmp: { name: 'RTMP', desc: 'Direct RTMP playback requires a proxy or transcoder' },
    webrtc: { name: 'WebRTC', desc: 'WebRTC requires signaling server and peer connection' },
  }[streamType]

  return (
    <div className="w-full h-full bg-muted/10 flex flex-col items-center justify-center gap-4">
      <AlertCircle className="h-12 w-12 text-muted-foreground/40" />
      <div className="text-center">
        <p className="text-muted-foreground text-sm font-medium">{streamInfo.name} Stream</p>
        <p className="text-muted-foreground/50 text-xs mt-1">
          {streamInfo.desc}
        </p>
        <p className="text-xs text-muted-foreground/60 mt-2 font-mono truncate max-w-[200px]">
          {src}
        </p>
      </div>
      <Button variant="outline" size="sm" onClick={onRetry}>
        <RefreshCw className="h-3.5 w-3.5 mr-1.5" />
        Retry
      </Button>
      <div className="text-xs text-muted-foreground/50 max-w-[250px] text-center">
        Use HLS or WebSocket stream format for direct browser playback
      </div>
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

  useEffect(() => {
    navigator.mediaDevices.getUserMedia({ video: true })
      .then((stream) => {
        if (videoRef.current) {
          videoRef.current.srcObject = stream
          onStreamReady(stream)
        }
      })
      .catch(() => {
        onError()
      })
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
}: VideoDisplayProps) {
  const { data, loading, error } = useDataSource<string>(dataSource, {
    fallback: propSrc,
  })

  const rawSrc = error ? propSrc : (data ?? propSrc ?? '')
  const [isLoading, setIsLoading] = useState(true)
  const [hasError, setHasError] = useState(false)
  const [isFullscreen, setIsFullscreen] = useState(false)
  const [retryKey, setRetryKey] = useState(0)

  const sizeConfig = dashboardComponentSize[size]

  const handleRetry = useCallback(() => {
    setRetryKey(prev => prev + 1)
    setHasError(false)
    setIsLoading(true)
  }, [])

  // Detect video type from URL if not explicitly set
  const detectedType = type !== 'file' ? type : (() => {
    if (!rawSrc) return 'file'
    if (rawSrc.includes('rtsp://')) return 'rtsp'
    if (rawSrc.includes('rtmp://')) return 'rtmp'
    if (rawSrc.includes('.m3u8')) return 'hls'
    if (rawSrc.startsWith('camera:') || rawSrc.startsWith('device:camera')) return 'device-camera'
    // HTTP/HTTPS URLs (video files or streams)
    if (rawSrc.startsWith('http://') || rawSrc.startsWith('https://')) return 'file'
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

  // Loading state from data source
  if (loading && isLoading) {
    return (
      <div className={cn(dashboardCardBase, 'flex items-center justify-center', sizeConfig.padding, className)}>
        <Skeleton className={cn('w-full h-full', rounded && 'rounded-lg')} />
      </div>
    )
  }

  // Error state
  if (hasError || (error && !rawSrc)) {
    return (
      <div className={cn(
        dashboardCardBase,
        'flex flex-col items-center justify-center gap-3 bg-muted/30',
        sizeConfig.padding,
        className
      )}>
        <AlertCircle className="h-10 w-10 text-muted-foreground/40" />
        <div className="text-center">
          <p className="text-muted-foreground text-sm font-medium">Video Load Error</p>
          <p className="text-muted-foreground/50 text-xs mt-1">Could not load video source</p>
        </div>
        <Button variant="outline" size="sm" onClick={handleRetry}>
          <RefreshCw className="h-3.5 w-3.5 mr-1.5" />
          Retry
        </Button>
      </div>
    )
  }

  const content = (
    <div className={cn(dashboardCardBase, 'relative overflow-hidden', className)}>
      {/* Video content */}
      <div className={cn('relative bg-black', rounded && 'rounded-lg', isFullscreen ? 'fixed inset-0 z-50 rounded-none' : 'aspect-video')}>
        {detectedType === 'rtsp' || detectedType === 'rtmp' || detectedType === 'webrtc' ? (
          <StreamPlaceholder src={rawSrc || ''} streamType={detectedType === 'webrtc' ? 'rtsp' : detectedType} onRetry={handleRetry} />
        ) : detectedType === 'device-camera' ? (
          <CameraAccess
            key={retryKey}
            onStreamReady={() => setIsLoading(false)}
            onError={() => setHasError(true)}
          />
        ) : (
          <VideoPlayer
            key={retryKey}
            src={rawSrc || ''}
            type={detectedType}
            autoplay={autoplay}
            muted={muted}
            controls={controls}
            loop={loop}
            fit={fit}
            onLoadingChange={setIsLoading}
            onError={setHasError}
          />
        )}
      </div>

      {/* Fullscreen toggle */}
      {showFullscreen && !isFullscreen && (
        <Button
          variant="secondary"
          size="icon"
          className="absolute top-2 right-2 h-7 w-7 bg-background/80 backdrop-blur"
          onClick={() => setIsFullscreen(true)}
        >
          <Maximize2 className="h-3.5 w-3.5" />
        </Button>
      )}

      {isFullscreen && (
        <Button
          variant="secondary"
          size="icon"
          className="absolute top-4 right-4 z-50 h-9 w-9 bg-background/80 backdrop-blur"
          onClick={() => setIsFullscreen(false)}
        >
          <Minimize2 className="h-4 w-4" />
        </Button>
      )}

      {/* Type indicator */}
      {!isFullscreen && rawSrc && (
        <div className="absolute top-2 left-2 px-2 py-0.5 bg-background/80 backdrop-blur rounded text-xs text-muted-foreground">
          {detectedType === 'rtsp' && 'RTSP'}
          {detectedType === 'rtmp' && 'RTMP'}
          {detectedType === 'webrtc' && 'WebRTC'}
          {detectedType === 'hls' && 'HLS'}
          {detectedType === 'stream' && 'Stream'}
          {detectedType === 'device-camera' && 'Camera'}
          {detectedType === 'file' && 'Video'}
        </div>
      )}
    </div>
  )

  return content
}
