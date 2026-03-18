/**
 * useUpdateCheck Hook
 *
 * Hook for checking and managing application updates.
 * Automatically checks for updates on mount and provides
 * manual check functionality with system notifications.
 */

import { useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useTranslation } from 'react-i18next'
import { useAppStore } from '@/store'
import type { UpdateInfo, UpdateProgress } from '@/store/slices/updateSlice'

const UPDATE_CHECK_INTERVAL = 24 * 60 * 60 * 1000 // 24 hours

export interface UpdateCheckOptions {
  /** Auto-check on mount (default: true) */
  autoCheck?: boolean
  /** Interval for auto-checking in ms (default: 24 hours) */
  checkInterval?: number
  /** Show system notification when update is available (default: true) */
  showNotification?: boolean
  /** Callback when update is available */
  onUpdateAvailable?: (info: UpdateInfo) => void
  /** Callback when already up to date */
  onUpToDate?: () => void
  /** Callback on error */
  onError?: (error: string) => void
}

export interface UseUpdateCheckReturn {
  /** Check for updates manually */
  checkUpdate: () => Promise<void>
  /** Download and install available update */
  downloadAndInstall: () => Promise<void>
  /** Get current app version */
  getAppVersion: () => Promise<string>
  /** Relaunch the application */
  relaunchApp: () => Promise<void>
}

/**
 * Hook for managing application updates
 */
export function useUpdateCheck(options: UpdateCheckOptions = {}): UseUpdateCheckReturn {
  const {
    autoCheck = true,
    checkInterval = UPDATE_CHECK_INTERVAL,
    showNotification = true,
    onUpdateAvailable,
    onUpToDate,
    onError,
  } = options

  const { t } = useTranslation(['common', 'settings'])
  const {
    updateStatus,
    updateInfo,
    downloadProgress,
    setUpdateStatus,
    setUpdateInfo,
    setDownloadProgress,
    setError,
    setUpdateDialogOpen
  } = useAppStore()

  const intervalRef = useRef<ReturnType<typeof setInterval>>()
  const unlistenRef = useRef<(() => void) | null>(null)
  const lastNotificationVersion = useRef<string | null>(null)

  // Use refs to store the latest callbacks without triggering re-renders
  const onUpdateAvailableRef = useRef(onUpdateAvailable)
  const onUpToDateRef = useRef(onUpToDate)
  const onErrorRef = useRef(onError)

  // Keep refs in sync with latest props
  useEffect(() => {
    onUpdateAvailableRef.current = onUpdateAvailable
    onUpToDateRef.current = onUpToDate
    onErrorRef.current = onError
  }, [onUpdateAvailable, onUpToDate, onError])

  /**
   * Show system notification for available update
   */
  const showUpdateNotification = useCallback(async (info: UpdateInfo) => {
    // Don't show notification if we've already notified about this version
    if (lastNotificationVersion.current === (info.version ?? null)) {
      return
    }

    try {
      await invoke('show_update_notification', {
        title: t('settings:newVersionAvailable'),
        body: info.version
          ? t('settings:updateAvailableWithVersion', { version: info.version })
          : t('settings:updateAvailableDesc'),
      })
      lastNotificationVersion.current = info.version ?? null
    } catch (error) {
      console.error('Failed to show update notification:', error)
    }
  }, [t])

  /**
   * Check for available updates
   */
  const checkUpdate = useCallback(async () => {
    console.log('[Update] checkUpdate called')
    try {
      console.log('[Update] Setting status to checking')
      setUpdateStatus('checking')
      setError(null)

      console.log('[Update] Invoking check_update command...')
      const info = await invoke<UpdateInfo>('check_update')
      console.log('[Update] Got response from backend:', info)

      if (info.available) {
        setUpdateInfo(info)
        setUpdateStatus('available')

        // Show system notification
        if (showNotification) {
          await showUpdateNotification(info)
        }

        // Open update dialog
        setUpdateDialogOpen(true)

        // Use ref to get latest callback without including it in dependencies
        onUpdateAvailableRef.current?.(info)
      } else {
        setUpdateStatus('up-to-date')
        onUpToDateRef.current?.()
      }
    } catch (error) {
      console.error('Failed to check for updates:', error)
      const errorMessage = error instanceof Error ? error.message : String(error)
      setError(errorMessage)
      setUpdateStatus('error')
      onErrorRef.current?.(errorMessage)
    }
  }, [setUpdateStatus, setUpdateInfo, setError, setUpdateDialogOpen, showNotification, showUpdateNotification])

  /**
   * Download and install the available update
   */
  const downloadAndInstall = useCallback(async () => {
    try {
      setUpdateStatus('downloading')
      setError(null)

      await invoke('download_and_install')

      setUpdateStatus('installing')
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error)
      setError(errorMessage)
      setUpdateStatus('error')
      onErrorRef.current?.(errorMessage)
      throw error
    }
  }, [setUpdateStatus, setError])

  /**
   * Get the current app version
   */
  const getAppVersion = useCallback(async (): Promise<string> => {
    try {
      return await invoke<string>('get_app_version')
    } catch (error) {
      console.error('Failed to get app version:', error)
      return 'unknown'
    }
  }, [])

  /**
   * Relaunch the application
   */
  const relaunchApp = useCallback(async () => {
    try {
      await invoke('relaunch_app')
    } catch (error) {
      console.error('Failed to relaunch app:', error)
      throw error
    }
  }, [])

  // Set up update progress listener
  useEffect(() => {
    const setupListener = async () => {
      try {
        const unlisten = await listen<UpdateProgress>('update-progress', (event) => {
          setDownloadProgress(event.payload)
        })
        unlistenRef.current = unlisten
      } catch (error) {
        console.error('Failed to set up update progress listener:', error)
      }
    }

    setupListener()

    return () => {
      unlistenRef.current?.()
    }
  }, [setDownloadProgress])

  // Auto-check on mount
  useEffect(() => {
    if (autoCheck) {
      // Check on mount
      checkUpdate()

      // Set up interval for periodic checks
      intervalRef.current = setInterval(() => {
        checkUpdate()
      }, checkInterval)

      return () => {
        clearInterval(intervalRef.current)
      }
    }
  }, [autoCheck, checkInterval, checkUpdate])

  return {
    checkUpdate,
    downloadAndInstall,
    getAppVersion,
    relaunchApp,
  }
}

export default useUpdateCheck
