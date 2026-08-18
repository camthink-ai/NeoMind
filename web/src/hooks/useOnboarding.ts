import { useState, useEffect, useCallback } from "react"
import { fetchAPI } from "@/lib/api"

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
  const [status, setStatus] = useState<OnboardingStatus | null>(null)
  const [loading, setLoading] = useState(true)

  const fetchStatus = useCallback(async () => {
    try {
      const data = await fetchAPI<OnboardingStatus>("/onboarding/status")
      setStatus(data)
    } catch {
      // Silently fail — onboarding is not critical
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    fetchStatus()
  }, [fetchStatus])

  // Poll for status updates while dialog is relevant (incomplete steps, not dismissed).
  // Benefits the rail's onboarding rocket badge and CLI-path users.
  useEffect(() => {
    if (!status || status.dismissed) return
    if (status.steps.llm.completed && status.steps.device.completed) return
    const id = setInterval(fetchStatus, 5000)
    return () => clearInterval(id)
  }, [status, fetchStatus])

  const dismiss = useCallback(async () => {
    try {
      await fetchAPI("/onboarding/dismiss", { method: "POST" })
      setStatus((prev) => (prev ? { ...prev, dismissed: true } : prev))
    } catch {
      // Silently fail
    }
  }, [])

  const reset = useCallback(async () => {
    try {
      await fetchAPI("/onboarding/reset", { method: "POST" })
      setStatus((prev) => (prev ? { ...prev, dismissed: false } : prev))
    } catch {
      // Silently fail
    }
  }, [])

  const hasIncompleteSteps =
    status && !status.dismissed && (!status.steps.llm.completed || !status.steps.device.completed)

  return {
    status,
    loading,
    dismiss,
    reset,
    fetchStatus,
    hasIncompleteSteps: !!hasIncompleteSteps,
    allComplete: status ? status.steps.llm.completed && status.steps.device.completed : false,
  }
}
