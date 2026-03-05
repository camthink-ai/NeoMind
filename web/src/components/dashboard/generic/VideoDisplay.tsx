/**
 * Video Display Component
 *
 * Video player for streams and files.
 * Supports HLS (.m3u8), MP4, and other video formats.
 * Compatible with camera feeds and video sources.
 */

import { useState, useCallback, useRef, useEffect, useMemo } from 'react'
import { createPortal } from 'react-dom'
import Hls from 'hls.js'
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
  RefreshCw,
  Webcam,
  AlertCircle,
  X,
} from 'lucide-react'
import type { DataSource } from '@/types/dashboard'
import { EmptyState, ErrorState } from '../shared'

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

// HLS optimized configuration
const createHlsConfig = () => ({
  enableWorker: true,
  enableSoftwareAES: true,
  // Buffer configuration
  maxBufferLength: 30,
  maxMaxBufferLength: 600,
  maxBufferSize: 60 * 1000 * 1000,
  backBufferLength: 90,
  maxBufferHole: 0.5,
  // Stall detection and recovery
  detectStallWithCurrentTimeMs: 1250,
  highBufferWatchdogPeriod: 3,
  nudgeOffset: 0.1,
  nudgeMaxRetry: 5,
  nudgeOnVideoHole: true,
  skipBufferHolePadding: 0.1,
  // Live stream configuration
  liveDurationInfinity: true,
  liveBackBufferLength: 0,
  liveSyncDurationCount: 3,
  liveMaxLatencyDurationCount: 10,
  liveSyncOnStallIncrease: 1,
  maxLiveSyncPlaybackRate: 1.5,
  // Low-Latency HLS
  lowLatencyMode: true,
  // ABR configuration
  abrEwmaDefaultEstimate: 500000,
  maxStarvationDelay: 4,
  maxLoadingDelay: 4,
  // Fragment loading policy with retry
  fragLoadPolicy: {
    default: {
      maxTimeToFirstByteMs: 10000,
      maxLoadTimeMs: 120000,
      timeoutRetry: {
        maxNumRetry: 4,
        retryDelayMs: 0,
        maxRetryDelayMs: 0,
      },
      errorRetry: {
        maxNumRetry: 6,
        retryDelayMs: 1000,
        maxRetryDelayMs: 8000,
        backoff: 'linear' as const,
      },
    },
  },
  // Manifest loading policy
  manifestLoadPolicy: {
    default: {
      maxTimeToFirstByteMs: 10000,
      maxLoadTimeMs: 20000,
      timeoutRetry: {
        maxNumRetry: 3,
        retryDelayMs: 0,
        maxRetryDelayMs: 0,
      },
      errorRetry: {
        maxNumRetry: 3,
        retryDelayMs: 1000,
        maxRetryDelayMs: 5000,
        backoff: 'linear' as const,
      },
    },
  },
  // Force key frame on discontinuity
  forceKeyFrameOnDiscontinuity: true,
  // Append error retry
  appendErrorMaxRetry: 5,
  // macOS VTDecompressionOutputCallback error handling
  // Skip corrupted video data to prevent decoder errors
  handleMpegTsVideoIntegrityErrors: 'skip' as const,
  // Stretch short video tracks to match audio
  stretchShortVideoTrack: true,
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
  const [isPlaying, setIsPlaying] = useState(false)
  const [isMuted, setIsMuted] = useState(muted)
  const [currentTime, setCurrentTime] = useState(0)
  const [duration, setDuration] = useState(0)
  const [isSeeking, setIsSeeking] = useState(false)

  // Error recovery state
  const retryCountRef = useRef(0)
  const decodeErrorCountRef = useRef(0)
  const lastDecodeErrorTimeRef = useRef(0)
  const isRecoveringRef = useRef(false)
  const maxRetries = 6
  const maxDecodeErrors = 5
  const decodeErrorCooldown = 3000 // 3 seconds cooldown

  // Native HLS recovery state (for macOS/Safari)
  const nativeHlsRetryRef = useRef(0)
  const nativeHlsRecoveringRef = useRef(false)

  // Stall detection and heartbeat
  const stallDetectionRef = useRef<{
    lastTime: number
    lastCurrentTime: number
    stallCount: number
    heartbeatInterval: ReturnType<typeof setInterval> | null
  }>({
    lastTime: 0,
    lastCurrentTime: 0,
    stallCount: 0,
    heartbeatInterval: null,
  })

  // Sync isMuted when muted prop changes
  useEffect(() => {
    setIsMuted(muted)
  }, [muted])

  // Initialize HLS if needed
  useEffect(() => {
    const video = videoRef.current
    if (!video || !src) return

    // Cleanup previous HLS instance
    if (hlsRef.current) {
      hlsRef.current.destroy()
      hlsRef.current = null
    }

    // Reset retry count when source changes
    retryCountRef.current = 0
    decodeErrorCountRef.current = 0
    lastDecodeErrorTimeRef.current = 0
    isRecoveringRef.current = false
    nativeHlsRetryRef.current = 0
    nativeHlsRecoveringRef.current = false

    if (type === 'hls') {
      // Detect Safari browser - Safari has native HLS support
      const isSafari = /^((?!chrome|android).)*safari/i.test(navigator.userAgent)
      const isMacOS = /Mac|iPod|iPhone|iPad/.test(navigator.platform)
      const hasNativeHLS = video.canPlayType('application/vnd.apple.mpegurl')

      // Native HLS recovery function for macOS/Safari
      const recoverNativeHLS = (errorType: number) => {
        if (nativeHlsRecoveringRef.current) {
          console.log('Native HLS already recovering, skipping...')
          return
        }

        nativeHlsRecoveringRef.current = true
        nativeHlsRetryRef.current++

        const maxNativeRetries = 5
        const currentPos = video.currentTime

        console.log(`Native HLS recovery attempt ${nativeHlsRetryRef.current}/${maxNativeRetries}`)

        if (nativeHlsRetryRef.current > maxNativeRetries) {
          console.error('Native HLS max retries exceeded, falling back to hls.js')
          // Fall back to hls.js
          if (Hls.isSupported()) {
            video.src = ''
            video.load()
            const hls = new Hls(createHlsConfig())
            hlsRef.current = hls
            hls.loadSource(src)
            hls.attachMedia(video)
            hls.on(Hls.Events.MANIFEST_PARSED, () => {
              video.currentTime = currentPos
              video.play().catch(() => {})
            })
            hls.on(Hls.Events.ERROR, (_event, data) => {
              if (data.fatal) {
                console.error('hls.js fallback error:', data.details)
                onError(true, `HLS Error: ${data.details}`)
              }
            })
          } else {
            onError(true, 'HLS playback failed and hls.js is not available')
          }
          return
        }

        // Recovery strategy based on error type
        if (errorType === 3) { // MEDIA_ERR_DECODE
          // For decode errors, try to skip ahead
          const skipAmount = nativeHlsRetryRef.current * 2 // Progressive skip
          const newPos = currentPos + skipAmount

          console.log(`Native HLS decode error, seeking from ${currentPos.toFixed(2)} to ${newPos.toFixed(2)}`)

          // Clear and reload
          video.src = ''
          video.load()

          setTimeout(() => {
            video.src = src
            video.currentTime = newPos
            video.play().catch(() => {})
            setTimeout(() => {
              nativeHlsRecoveringRef.current = false
            }, 2000)
          }, 500)
        } else {
          // For other errors, simple reload
          video.load()
          video.play().catch(() => {})
          setTimeout(() => {
            nativeHlsRecoveringRef.current = false
          }, 2000)
        }
      }

      // Use native HLS for Safari/macOS
      if (hasNativeHLS || isSafari || isMacOS) {
        console.log('Using native HLS support (Safari/macOS)')
        video.src = src

        video.addEventListener('loadedmetadata', () => {
          onLoadingChange(false)
          nativeHlsRetryRef.current = 0 // Reset on successful load
          if (autoplay) {
            video.play().catch(() => {})
          }
        })

        // Add error handling for native HLS
        video.addEventListener('error', () => {
          const error = video.error
          if (error) {
            console.error('Native HLS error:', error.code, error.message)
            if (error.code === 3) { // MEDIA_ERR_DECODE
              recoverNativeHLS(3)
            } else if (error.code === 2) { // MEDIA_ERR_NETWORK
              console.log('Native HLS network error, retrying...')
              video.load()
              video.play().catch(() => {})
            }
          }
        })

        // Add stalled handling for native HLS
        video.addEventListener('stalled', () => {
          console.log('Native HLS stalled, checking playback...')
          if (!video.paused && video.readyState < 3) {
            video.currentTime = video.currentTime + 0.1
          }
        })
      }
      // Use hls.js for other browsers (Chrome, Firefox, etc.)
      else if (Hls.isSupported()) {
        console.log('Using hls.js for HLS playback')
        const hls = new Hls(createHlsConfig())
        hlsRef.current = hls

        hls.loadSource(src)
        hls.attachMedia(video)

        hls.on(Hls.Events.MANIFEST_PARSED, () => {
          onLoadingChange(false)
          retryCountRef.current = 0 // Reset retry count on successful manifest
          stallDetectionRef.current.stallCount = 0
          if (autoplay) {
            video.play().catch(() => {})
          }
        })

        // Track fragment changes for live stream health monitoring
        hls.on(Hls.Events.FRAG_CHANGED, (_event, data) => {
          // Reset stall count when fragment changes successfully
          stallDetectionRef.current.stallCount = 0
          // Update last activity time
          stallDetectionRef.current.lastTime = Date.now()
        })

        // Handle fragment load emergency aborted
        hls.on(Hls.Events.FRAG_LOAD_EMERGENCY_ABORTED, () => {
          console.warn('HLS fragment load emergency aborted, retrying...')
          hls.startLoad()
        })

        // Handle all HLS errors
        hls.on(Hls.Events.ERROR, (_event, data) => {
          console.warn('HLS error:', data.type, data.details, data.fatal ? '(fatal)' : '(non-fatal)')

          // Handle buffer stalled specifically (non-fatal)
          if (data.details === Hls.ErrorDetails.BUFFER_STALLED_ERROR) {
            console.warn('HLS buffer stalled, attempting recovery...')
            // Try to nudge playback forward
            if (video && !video.paused && video.currentTime > 0) {
              const nudgeAmount = 0.1
              video.currentTime = video.currentTime + nudgeAmount
            }
            return
          }

          // Handle buffer append error (non-fatal but needs attention)
          if (data.details === Hls.ErrorDetails.BUFFER_APPEND_ERROR) {
            console.warn('HLS buffer append error, attempting recovery...')
            // Try to seek forward to skip the problematic segment
            if (video && !video.paused) {
              const currentPos = video.currentTime
              video.currentTime = currentPos + 1
            }
            return
          }

          // Handle non-fatal errors
          if (!data.fatal) {
            // Log specific non-fatal errors
            if (data.details === Hls.ErrorDetails.FRAG_LOAD_ERROR) {
              console.warn('HLS fragment load error:', data.frag?.url)
            } else if (data.details === Hls.ErrorDetails.KEY_LOAD_ERROR) {
              console.warn('HLS key load error')
            } else if (data.details === Hls.ErrorDetails.MANIFEST_LOAD_ERROR) {
              console.error('HLS manifest load error')
            }
            return
          }

          // Handle fatal errors
          switch (data.type) {
            case Hls.ErrorTypes.NETWORK_ERROR:
              console.error('HLS network error:', data.details)
              // Check if we can retry
              if (retryCountRef.current < maxRetries) {
                retryCountRef.current++
                console.log(`HLS retry attempt ${retryCountRef.current}/${maxRetries}`)
                // Wait a bit before retrying
                setTimeout(() => {
                  if (hlsRef.current) {
                    hlsRef.current.startLoad()
                  }
                }, 1000 * retryCountRef.current) // Exponential backoff
              } else {
                console.error('HLS max retries exceeded')
                onError(true, `Network Error: ${data.details}`)
              }
              break
            case Hls.ErrorTypes.MEDIA_ERROR:
              console.error('HLS media error:', data.details)
              // Try to recover media error
              const recovered = hls.recoverMediaError()
              console.log('HLS recoverMediaError result:', recovered)
              
              // If recovery didn't work, try to rebuild HLS instance
              setTimeout(() => {
                if (videoRef.current && videoRef.current.error) {
                  console.log('Media error still present, rebuilding HLS instance...')
                  const currentPos = videoRef.current.currentTime
                  hls.destroy()
                  const newHls = new Hls(createHlsConfig())
                  hlsRef.current = newHls
                  newHls.loadSource(src)
                  newHls.attachMedia(videoRef.current)
                  videoRef.current.currentTime = currentPos + 2
                  videoRef.current.play().catch(() => {})
                }
              }, 2000)
              break
            default:
              console.error('HLS fatal error:', data.details)
              onError(true, `HLS Error: ${data.details}`)
              break
          }
        })
      } else {
        onError(true, 'HLS is not supported in this browser')
      }
    } else {
      // Regular video file
      video.src = src
    }

    return () => {
      if (hlsRef.current) {
        hlsRef.current.destroy()
        hlsRef.current = null
      }
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [src, type]) // Only recreate HLS when src or type changes, not onLoadingChange/onError

  // Video event handlers
  useEffect(() => {
    const video = videoRef.current
    if (!video) return

    const handleLoadStart = () => onLoadingChange(true)
    const handleCanPlay = () => {
      onLoadingChange(false)
      // Reset decode error count on successful playback
      decodeErrorCountRef.current = 0
      isRecoveringRef.current = false
    }
    const handleError = () => {
      const error = video.error
      if (!error) {
        onError(true, 'Unknown video error')
        return
      }

      // Error code 3 = MEDIA_ERR_DECODE (decoding error)
      // This often happens on macOS with VTDecompressionOutputCallback errors
      if (error.code === 3) {
        const now = Date.now()
        
        // Check if we're in cooldown period
        if (now - lastDecodeErrorTimeRef.current < decodeErrorCooldown) {
          console.warn('Decode error in cooldown period, skipping recovery')
          return
        }
        
        // Check if already recovering
        if (isRecoveringRef.current) {
          console.warn('Already recovering from decode error, skipping')
          return
        }
        
        console.error('Video decode error:', error.message)
        lastDecodeErrorTimeRef.current = now
        decodeErrorCountRef.current++
        
        console.log(`Decode error count: ${decodeErrorCountRef.current}/${maxDecodeErrors}`)

        if (decodeErrorCountRef.current <= maxDecodeErrors) {
          isRecoveringRef.current = true
          
          // For macOS VTDecompressionOutputCallback errors, skip more aggressively
          const skipAmount = decodeErrorCountRef.current <= 2 ? 2 : 5
          const currentPos = video.currentTime
          const newPos = currentPos + skipAmount
          
          console.log(`Attempting recovery by seeking from ${currentPos.toFixed(2)} to ${newPos.toFixed(2)}`)
          
          // Define recovery function
          const performRecovery = () => {
            // For HLS, use startLoad to reload from new position
            if (type === 'hls' && hlsRef.current) {
              hlsRef.current.startLoad(newPos)
            }
            video.currentTime = newPos
            
            // Wait for seek to complete, then play
            const handleSeeked = () => {
              video.removeEventListener('seeked', handleSeeked)
              console.log('Seek completed, resuming playback')
              video.play().catch(() => {})
              setTimeout(() => {
                isRecoveringRef.current = false
              }, 1000)
            }
            
            video.addEventListener('seeked', handleSeeked)
            
            // Fallback timeout in case seeked event doesn't fire
            setTimeout(() => {
              video.removeEventListener('seeked', handleSeeked)
              if (isRecoveringRef.current) {
                console.log('Seek timeout, forcing playback')
                video.play().catch(() => {})
                isRecoveringRef.current = false
              }
            }, 3000)
          }
          
          performRecovery()
          return
        }
        
        // If we've exceeded simple retries, try full recovery with decoder reset
        if (decodeErrorCountRef.current === maxDecodeErrors + 1) {
          console.log('Attempting full decoder reset and HLS instance recovery')
          isRecoveringRef.current = true
          
          // Store current position before reset
          const currentPos = video.currentTime
          
          // Step 1: Clear video source to reset decoder
          video.src = ''
          video.load()
          
          // Step 2: Destroy and recreate HLS instance
          if (hlsRef.current) {
            hlsRef.current.destroy()
            hlsRef.current = null
          }
          
          // Step 3: Wait and reinitialize
          setTimeout(() => {
            if (videoRef.current && type === 'hls') {
              const newHls = new Hls(createHlsConfig())
              hlsRef.current = newHls
              newHls.loadSource(src)
              newHls.attachMedia(videoRef.current)
              
              // Jump to a position slightly ahead
              videoRef.current.currentTime = currentPos + 5
              videoRef.current.play().catch(() => {})
              
              setTimeout(() => {
                isRecoveringRef.current = false
              }, 3000)
            }
          }, 1500)
          return
        }
        
        // All recovery attempts failed
        console.error('All decode error recovery attempts failed')
        onError(true, `Decode Error: Video format may not be supported on this device`)
        return
      }

      // Other error types
      const message = error ? `Error ${error.code}: ${error.message}` : 'Unknown video error'
      onError(true, message)
    }
    const handlePlay = () => {
      setIsPlaying(true)
      // Start heartbeat on play
      if (!stallDetectionRef.current.heartbeatInterval) {
        stallDetectionRef.current.heartbeatInterval = setInterval(() => {
          const v = videoRef.current
          if (!v || v.paused) return

          const now = Date.now()
          const currentTime = v.currentTime
          const lastTime = stallDetectionRef.current.lastTime
          const lastCurrentTime = stallDetectionRef.current.lastCurrentTime

          // Check if playback is stuck (time not advancing but should be playing)
          if (lastTime > 0 && now - lastTime > 3000 && Math.abs(currentTime - lastCurrentTime) < 0.1) {
            stallDetectionRef.current.stallCount++
            console.warn(`Playback stall detected (${stallDetectionRef.current.stallCount}), attempting recovery...`)

            if (stallDetectionRef.current.stallCount <= 5) {
              // Try nudge forward
              v.currentTime = currentTime + 0.5
              v.play().catch(() => {})
            } else if (stallDetectionRef.current.stallCount <= 10) {
              // Try reload from current position
              if (hlsRef.current) {
                hlsRef.current.startLoad(currentTime)
              }
            } else {
              // Full recovery
              console.error('Persistent stall, attempting full recovery')
              if (hlsRef.current) {
                hlsRef.current.destroy()
                hlsRef.current = null
              }
              const newHls = new Hls(createHlsConfig())
              hlsRef.current = newHls
              newHls.loadSource(src)
              newHls.attachMedia(v)
              v.play().catch(() => {})
              stallDetectionRef.current.stallCount = 0
            }
          }

          stallDetectionRef.current.lastTime = now
          stallDetectionRef.current.lastCurrentTime = currentTime
        }, 3000)
      }
    }
    const handlePause = () => {
      setIsPlaying(false)
      // Stop heartbeat on pause
      if (stallDetectionRef.current.heartbeatInterval) {
        clearInterval(stallDetectionRef.current.heartbeatInterval)
        stallDetectionRef.current.heartbeatInterval = null
      }
    }
    const handleTimeUpdate = () => {
      if (!isSeeking) {
        setCurrentTime(video.currentTime)
      }
    }
    const handleLoadedMetadata = () => setDuration(video.duration)

    // Handle waiting event (buffering)
    const handleWaiting = () => {
      console.log('Video waiting for buffer...')
      // If waiting too long, try to recover
      setTimeout(() => {
        const v = videoRef.current
        if (v && v.paused && !v.ended && v.readyState < 3) {
          console.warn('Long wait detected, attempting recovery')
          if (hlsRef.current) {
            hlsRef.current.startLoad()
          }
        }
      }, 5000)
    }

    // Handle stalled event
    const handleStalled = () => {
      console.warn('Video stalled, checking playback state...')
      // Try to resume if should be playing
      if (!video.paused && video.readyState < 3) {
        // Wait a bit to see if it recovers naturally
        setTimeout(() => {
          if (videoRef.current && !videoRef.current.paused && videoRef.current.readyState < 3) {
            console.warn('Still stalled after wait, attempting recovery')
            if (hlsRef.current) {
              hlsRef.current.startLoad()
            }
            videoRef.current.play().catch(() => {})
          }
        }, 2000)
      }
    }

    // Handle ended event (for live streams, this shouldn't happen)
    const handleEnded = () => {
      console.log('Video ended')
      // For live streams, try to reconnect
      if (type === 'hls' && hlsRef.current) {
        console.log('Live stream ended, attempting to reconnect...')
        setTimeout(() => {
          if (hlsRef.current) {
            hlsRef.current.startLoad(-1)
          }
          video.play().catch(() => {})
        }, 2000)
      }
    }

    video.addEventListener('loadstart', handleLoadStart)
    video.addEventListener('canplay', handleCanPlay)
    video.addEventListener('error', handleError)
    video.addEventListener('play', handlePlay)
    video.addEventListener('pause', handlePause)
    video.addEventListener('timeupdate', handleTimeUpdate)
    video.addEventListener('loadedmetadata', handleLoadedMetadata)
    video.addEventListener('waiting', handleWaiting)
    video.addEventListener('stalled', handleStalled)
    video.addEventListener('ended', handleEnded)

    return () => {
      video.removeEventListener('loadstart', handleLoadStart)
      video.removeEventListener('canplay', handleCanPlay)
      video.removeEventListener('error', handleError)
      video.removeEventListener('play', handlePlay)
      video.removeEventListener('pause', handlePause)
      video.removeEventListener('timeupdate', handleTimeUpdate)
      video.removeEventListener('loadedmetadata', handleLoadedMetadata)
      video.removeEventListener('waiting', handleWaiting)
      video.removeEventListener('stalled', handleStalled)
      video.removeEventListener('ended', handleEnded)
      // Cleanup heartbeat interval
      if (stallDetectionRef.current.heartbeatInterval) {
        clearInterval(stallDetectionRef.current.heartbeatInterval)
        stallDetectionRef.current.heartbeatInterval = null
      }
    }
  }, [onLoadingChange, onError, isSeeking, src, type])

  // Autoplay for non-HLS
  useEffect(() => {
    if (type !== 'hls' && autoplay && videoRef.current) {
      videoRef.current.play().catch(() => {
        // Autoplay was prevented, user interaction required
      })
    }
  }, [autoplay, type])

  // Handle visibility change - resume playback when tab becomes visible
  useEffect(() => {
    const handleVisibilityChange = () => {
      const video = videoRef.current
      const hls = hlsRef.current

      if (document.visibilityState === 'visible') {
        console.log('Tab became visible, checking playback state...')

        if (video && video.paused && !video.ended) {
          // Video was paused while tab was hidden, try to resume
          console.log('Resuming playback after tab visibility change')

          // For HLS live streams, jump to live edge
          if (type === 'hls' && hls && hls.liveSyncPosition !== undefined && hls.liveSyncPosition !== null) {
            console.log('Jumping to live sync position:', hls.liveSyncPosition)
            video.currentTime = hls.liveSyncPosition
          }

          video.play().catch((e) => {
            console.warn('Could not auto-resume playback:', e)
          })
        }

        // Reset stall detection
        stallDetectionRef.current.stallCount = 0
        stallDetectionRef.current.lastTime = 0
        stallDetectionRef.current.lastCurrentTime = 0
      }
    }

    document.addEventListener('visibilitychange', handleVisibilityChange)

    return () => {
      document.removeEventListener('visibilitychange', handleVisibilityChange)
    }
  }, [type])

  // HLS live stream keep-alive - periodically check and recover
  useEffect(() => {
    if (type !== 'hls') return

    const keepAliveInterval = setInterval(() => {
      const video = videoRef.current
      const hls = hlsRef.current

      if (!video || !hls) return

      // Check if we're supposed to be playing but aren't
      if (!video.paused && video.readyState < 3) {
        console.warn('HLS live stream buffering, checking...')

        // If we have a live sync position and we're too far behind, jump forward
        if (hls.liveSyncPosition !== null && hls.liveSyncPosition !== undefined && video.currentTime < hls.liveSyncPosition - 10) {
          console.log('Too far behind live edge, jumping to:', hls.liveSyncPosition)
          video.currentTime = hls.liveSyncPosition
        }
      }
    }, 5000)

    return () => {
      clearInterval(keepAliveInterval)
    }
  }, [type])

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
}: VideoDisplayProps) {
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
          <p className="text-muted-foreground/50 text-xs mt-1">
            {errorMessage || 'Could not load video source'}
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={handleRetry}>
          <RefreshCw className="h-3.5 w-3.5 mr-1.5" />
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
            className="absolute top-2 right-2 h-7 w-7 bg-background/80 backdrop-blur"
            onClick={() => setIsFullscreen(true)}
          >
            <Maximize2 className="h-3.5 w-3.5" />
          </Button>
        )}

        {/* Type indicator */}
        {rawSrc && (
          <div className="absolute top-2 left-2 px-2 py-0.5 bg-background/80 backdrop-blur rounded text-xs text-muted-foreground">
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
    <div className="fixed inset-0 z-[9999] bg-black flex flex-col">
      {/* Header with close button */}
      <div className="flex items-center justify-between px-4 py-3 bg-background/95 border-b">
        <div className="flex items-center gap-2">
          <Webcam className="h-4 w-4 text-muted-foreground" />
          <span className="text-sm font-medium">Video</span>
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
    </div>,
    document.body
  )

  return (
    <>
      {content}
      {fullscreenOverlay}
    </>
  )
}