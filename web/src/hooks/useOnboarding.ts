import { useState, useEffect, useCallback } from "react"
import { fetchAPI, tokenManager } from "@/lib/api"
import { useStore } from "@/store"

const VIEWER_ONBOARDING_DISMISSED_KEY = "neomind_viewer_onboarding_dismissed"

function getViewerDismissed(): boolean {
  try {
    return localStorage.getItem(VIEWER_ONBOARDING_DISMISSED_KEY) === "true"
  } catch {
    return false
  }
}

function setViewerDismissed(dismissed: boolean): void {
  try {
    if (dismissed) {
      localStorage.setItem(VIEWER_ONBOARDING_DISMISSED_KEY, "true")
    } else {
      localStorage.removeItem(VIEWER_ONBOARDING_DISMISSED_KEY)
    }
  } catch {
    // Ignore storage errors (private browsing, quota, etc.)
  }
}

export interface OnboardingStatus {
  dismissed: boolean
  system_status: {
    has_llm_backend: boolean
    has_devices: boolean
    device_count: number
  }
  steps: {
    llm: { completed: boolean }
    device: { completed: boolean }
  }
}

export function useOnboarding() {
  const userRole = useStore((s) => s.user?.role)
  const isViewer = (userRole ?? tokenManager.getUser()?.role) === "viewer"
  const [viewerDismissed, setViewerDismissedState] = useState<boolean>(() => getViewerDismissed())
  const [status, setStatus] = useState<OnboardingStatus | null>(null)
  const [loading, setLoading] = useState(true)

  const fetchStatus = useCallback(async () => {
    try {
      const data = await fetchAPI<OnboardingStatus>("/onboarding/status")
      if (isViewer && getViewerDismissed()) {
        setStatus({ ...data, dismissed: true })
      } else {
        setStatus(data)
      }
    } catch {
      // Silently fail — onboarding is not critical
    } finally {
      setLoading(false)
    }
  }, [isViewer])

  useEffect(() => {
    fetchStatus()
  }, [fetchStatus])

  const effectiveStatus =
    status && isViewer && viewerDismissed ? { ...status, dismissed: true } : status

  // Poll for status updates while dialog is relevant (incomplete steps, not dismissed).
  // Benefits the rail's onboarding rocket badge and CLI-path users.
  useEffect(() => {
    if (!effectiveStatus || effectiveStatus.dismissed) return
    if (effectiveStatus.steps.llm.completed && effectiveStatus.steps.device.completed) return
    const id = setInterval(fetchStatus, 5000)
    return () => clearInterval(id)
  }, [effectiveStatus, fetchStatus])

  const dismiss = useCallback(async () => {
    if (isViewer) {
      setViewerDismissed(true)
      setViewerDismissedState(true)
      setStatus((prev) => (prev ? { ...prev, dismissed: true } : prev))
      return
    }
    try {
      await fetchAPI("/onboarding/dismiss", { method: "POST" })
      setStatus((prev) => (prev ? { ...prev, dismissed: true } : prev))
    } catch {
      // Silently fail
    }
  }, [isViewer])

  const reset = useCallback(async () => {
    if (isViewer) {
      setViewerDismissed(false)
      setViewerDismissedState(false)
      setStatus((prev) => (prev ? { ...prev, dismissed: false } : prev))
      return
    }
    try {
      await fetchAPI("/onboarding/reset", { method: "POST" })
      setStatus((prev) => (prev ? { ...prev, dismissed: false } : prev))
    } catch {
      // Silently fail
    }
  }, [isViewer])

  const hasIncompleteSteps =
    effectiveStatus &&
    !effectiveStatus.dismissed &&
    (!effectiveStatus.steps.llm.completed || !effectiveStatus.steps.device.completed)

  return {
    status: effectiveStatus,
    loading,
    dismiss,
    reset,
    fetchStatus,
    hasIncompleteSteps: !!hasIncompleteSteps,
    allComplete: effectiveStatus
      ? effectiveStatus.steps.llm.completed && effectiveStatus.steps.device.completed
      : false,
  }
}
