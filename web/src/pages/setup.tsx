/**
 * Setup Page - Simplified 2-step initial setup
 *
 * Step 1: Create admin account + select timezone (auto-detected)
 * Step 2: Complete with quick-start guide
 *
 * LLM configuration is deferred — users configure it when they first use AI features.
 */
import { useState, useEffect } from "react"
import { useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { getApiBase, isTauriEnv } from '@/lib/api'
import { Loader2 } from "lucide-react"
import { LoadingState } from "@/components/shared/LoadingState"
import { AccountStep } from "./setup/AccountStep"
import { CompleteStep } from "./setup/CompleteStep"

type SetupStep = 'account' | 'complete'

export function SetupPage() {
  const navigate = useNavigate()
  const { login } = useStore()
  const { withErrorHandling } = useErrorHandler()
  const { t } = useTranslation(['setup'])

  const [step, setStep] = useState<SetupStep>('account')
  const [checking, setChecking] = useState(true)
  const [accountInfo, setAccountInfo] = useState<{
    username: string
    password: string
    token: string
    timezone: string
  } | null>(null)

  const getApiUrl = (path: string) => `${getApiBase()}${path}`

  // Check if setup is already completed
  useEffect(() => {
    checkSetupStatus()
  }, [])

  const checkSetupStatus = async () => {
    const result = await withErrorHandling(
      async () => {
        const maxRetries = isTauriEnv() ? 15 : 3
        const initialDelay = isTauriEnv() ? 500 : 100

        for (let i = 0; i < maxRetries; i++) {
          try {
            const response = await fetch(getApiUrl('/setup/status'), {
              signal: AbortSignal.timeout(3000),
            })
            if (!response.ok) throw new Error(`HTTP ${response.status}`)
            return await response.json() as { setup_required: boolean }
          } catch {
            if (i < maxRetries - 1) {
              await new Promise(resolve => setTimeout(resolve, initialDelay * (1 + i * 0.5)))
            } else {
              throw new Error(`Failed after ${maxRetries} retries`)
            }
          }
        }
        throw new Error('Failed to check setup status')
      },
      { operation: 'Check setup status', showToast: false }
    )

    setChecking(false)
    if (result && !result.setup_required) {
      navigate('/')
    }
  }

  const handleAccountCreated = async (username: string, password: string, token: string, timezone: string) => {
    setAccountInfo({ username, password, token, timezone })
    // Complete setup (mark as done on server)
    try {
      await fetch(getApiUrl('/setup/complete'), {
        method: 'POST',
        headers: { 'Authorization': `Bearer ${token}` },
      })
    } catch {
      // Non-blocking — the account is already created
    }
    setStep('complete')
  }

  const handleComplete = (dest = '/') => {
    if (accountInfo) {
      login(accountInfo.username, accountInfo.password, true).then(() => {
        window.location.href = dest
      }).catch(() => {
        navigate('/login')
      })
    } else {
      navigate('/login')
    }
  }

  // Loading state while checking setup status
  if (checking) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-background">
        <LoadingState size="lg" text={t('setup:loading')} />
      </div>
    )
  }

  if (step === 'account') {
    return (
      <AccountStep
        getApiUrl={getApiUrl}
        onAccountCreated={handleAccountCreated}
      />
    )
  }

  if (step === 'complete') {
    return (
      <CompleteStep
        username={accountInfo?.username || ''}
        initialTimezone={accountInfo?.timezone || 'Asia/Shanghai'}
        token={accountInfo?.token || ''}
        getApiUrl={getApiUrl}
        onComplete={handleComplete}
      />
    )
  }

  return null
}
