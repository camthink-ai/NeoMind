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
import { notifySuccess } from '@/lib/notify'
import { api } from '@/lib/api'
import type { UpdateInfo, UpdateProgress } from '@/store/slices/updateSlice'

/** Normalize version strings for reliable comparison */
const normalizeVersion = (v: string) => v.trim().replace(/^v/, '')

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
  const lastProgressUpdateRef = useRef(0)

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

    // Skip when not running in Tauri desktop
    if (!(window as any).__TAURI_INTERNALS__) return

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
    // Browser (non-Tauri) = server deployment: check the server's release
    // state via the admin API instead of the desktop OTA plugin. Populates
    // the same updateInfo slice that drives the About badge; no auto-open
    // dialog — the About page's server-upgrade dialog opens on demand.
    if (!(window as any).__TAURI_INTERNALS__) {
      try {
        setUpdateStatus('checking')
        setError(null)

        // Post-upgrade marker (written by ServerUpgradeDialog right before
        // its reload): toast once, skip the immediate re-check.
        const pendingVersion = localStorage.getItem('neomind_installed_version')
        if (pendingVersion) {
          localStorage.removeItem('neomind_installed_version')
          notifySuccess(t('settings:updateApplied'), t('settings:newVersionAvailable'))
          setUpdateStatus('up-to-date')
          onUpToDateRef.current?.()
          return
        }

        const check = await api.checkServerUpgrade()
        if (check.available && check.latest_version) {
          setUpdateInfo({
            available: true,
            version: check.latest_version,
            body: check.release_notes ?? undefined,
          })
          setUpdateStatus('available')
          onUpdateAvailableRef.current?.({
            available: true,
            version: check.latest_version,
            body: check.release_notes ?? undefined,
          })
        } else {
          setUpdateInfo({ available: false })
          setUpdateStatus('up-to-date')
          onUpToDateRef.current?.()
        }
      } catch (error) {
        console.error('Failed to check server upgrades:', error)
        const errorMessage = error instanceof Error ? error.message : String(error)
        setError(errorMessage)
        setUpdateStatus('error')
        onErrorRef.current?.(errorMessage)
      }
      return
    }

    try {
      setUpdateStatus('checking')
      setError(null)

      // Handle post-update restart: detect if an update was just applied
      // by comparing the stored target version with the current app version.
      // If detected, show success toast and skip server check to prevent
      // the update dialog from re-appearing on the new version.
      const pendingVersion = localStorage.getItem('neomind_installed_version')
      if (pendingVersion) {
        localStorage.removeItem('neomind_installed_version')

        notifySuccess(
          t('settings:updateApplied'),
          t('settings:newVersionAvailable')
        )

        // Skip server check — we just updated, no need to re-check immediately.
        // Force-close any persisted/lingering dialog: after a successful OTA
        // the "New Version Available" dialog must NOT re-appear.
        setUpdateStatus('up-to-date')
        setUpdateDialogOpen(false)
        onUpToDateRef.current?.()
        return
      }

      const info = await invoke<UpdateInfo>('check_update')

      if (info.available) {
        setUpdateInfo(info)
        setUpdateStatus('available')

        if (showNotification) {
          await showUpdateNotification(info)
        }

        setUpdateDialogOpen(true)
        onUpdateAvailableRef.current?.(info)
      } else {
        setUpdateStatus('up-to-date')
        setUpdateDialogOpen(false)
        onUpToDateRef.current?.()
      }
    } catch (error) {
      console.error('Failed to check for updates:', error)
      const errorMessage = error instanceof Error ? error.message : String(error)
      setError(errorMessage)
      setUpdateStatus('error')
      onErrorRef.current?.(errorMessage)
    }
  }, [setUpdateStatus, setUpdateInfo, setError, setUpdateDialogOpen, showNotification, showUpdateNotification, t])

  /**
   * Download and install the available update
   */
  const downloadAndInstall = useCallback(async () => {
    if (!(window as any).__TAURI_INTERNALS__) return

    try {
      setUpdateStatus('downloading')
      setError(null)

      // Pre-write the "installed" marker BEFORE invoking download_and_install.
      //
      // Tauri's updater may trigger a process restart or webview reload the
      // moment download_and_install resolves (platform-dependent: macOS can
      // replace the .app bundle mid-flight, Windows NSIS may auto-restart).
      // Writing the marker AFTER the await is a race — if the process dies
      // between invoke() resolving and localStorage.setItem() running, the
      // next launch finds no marker, falls through to check_update, and the
      // dialog pops up again on the version we just installed.
      //
      // We already know the target version from check_update; persist it now
      // and clear it on error so a failed install doesn't poison the next
      // launch.
      const latestInfo = useAppStore.getState().updateInfo
      if (latestInfo?.version) {
        localStorage.setItem('neomind_installed_version', latestInfo.version)
      }

      try {
        await invoke('download_and_install')
      } catch (invokeError) {
        // Install failed — clear the pre-written marker so next launch
        // doesn't skip a legitimate update check.
        localStorage.removeItem('neomind_installed_version')
        throw invokeError
      }

      setUpdateStatus('done')
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
    if (!(window as any).__TAURI_INTERNALS__) return 'unknown'

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
    if (!(window as any).__TAURI_INTERNALS__) return

    try {
      await invoke('relaunch_app')
    } catch (error) {
      console.error('Failed to relaunch app:', error)
      throw error
    }
  }, [])

  // Set up update progress listener (only in Tauri desktop)
  useEffect(() => {
    // Skip when not running in Tauri desktop
    if (!(window as any).__TAURI_INTERNALS__) return

    const setupListener = async () => {
      try {
        const unlisten = await listen<UpdateProgress>('update-progress', (event) => {
          // Throttle to ~200ms intervals to prevent UI flickering
          const now = Date.now()
          if (now - lastProgressUpdateRef.current < 200 && event.payload.progress < 100) {
            return
          }
          lastProgressUpdateRef.current = now
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

  // Auto-check on mount — use a ref-based stable callback to avoid
  // resetting the interval when checkUpdate is recreated (e.g. on language change)
  const checkUpdateRef = useRef(checkUpdate)
  checkUpdateRef.current = checkUpdate

  useEffect(() => {
    if (autoCheck) {
      checkUpdateRef.current()

      intervalRef.current = setInterval(() => {
        checkUpdateRef.current()
      }, checkInterval)

      return () => {
        clearInterval(intervalRef.current)
      }
    }
  }, [autoCheck, checkInterval])

  return {
    checkUpdate,
    downloadAndInstall,
    getAppVersion,
    relaunchApp,
  }
}

export default useUpdateCheck
