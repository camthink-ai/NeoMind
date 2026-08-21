/**
 * AccountStep - Create admin account with optional timezone selection
 *
 * Combined step 1 of setup: account creation + timezone (auto-detected)
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Lock, User, Shield, ArrowRight } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { PasswordInput } from '@/components/ui/password-input'
import { Label } from '@/components/ui/label'
import { Checkbox } from '@/components/ui/checkbox'
import { SetupBackground } from './SetupBackground'
import { SetupHeader } from './SetupHeader'
import { getBrowserTimezone, COMMON_TIMEZONE_IDS } from '@/lib/time/format'
import { notifySuccess } from '@/lib/notify'

// Mailchimp subscription function
function mcSubscribe(email: string, username?: string): Promise<{ result: string; msg: string }> {
  const base = "https://camthink.us2.list-manage.com/subscribe/post-json"
  const cb = "mc_cb_" + Date.now() + "_" + Math.random().toString(16).slice(2)

  const params = new URLSearchParams({
    u: "4ecc400d85930178fb49aa9de",
    id: "466fcc3b55",
    f_id: "00e60ae1f0",
    EMAIL: email,
    b_4ecc400d85930178fb49aa9de_466fcc3b55: "",
    c: cb,
    _: Date.now().toString(),
  })

  if (username) params.append("FNAME", username)

  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      cleanup()
      reject(new Error("Mailchimp JSONP timeout"))
    }, 8000)

    function cleanup() {
      clearTimeout(timeout)
      try { delete (window as any)[cb] } catch (_) { /* ignore */ }
      if (script && script.parentNode) script.parentNode.removeChild(script)
    }

    ;(window as any)[cb] = function (data: { result: string; msg: string }) {
      cleanup()
      resolve(data)
    }

    const script = document.createElement("script")
    script.src = base + "?" + params.toString()
    script.onerror = () => { cleanup(); reject(new Error("Mailchimp JSONP network error")) }
    document.head.appendChild(script)
  })
}

// Error translation helper
function translateError(error: string, t: (key: string, params?: Record<string, unknown>) => string): string {
  const lowerError = error.toLowerCase()
  if (lowerError.includes("password must be at least")) return t('minPasswordLength', { ns: 'validation' })
  if (lowerError.includes("username must be at least")) return t('minUsernameLength', { ns: 'validation' })
  if (lowerError.includes("password must contain")) return t('passwordComplexity')
  if (lowerError.includes("setup already completed")) return t('setupAlreadyCompleted')
  return error || t("setupFailed")
}

interface AccountStepProps {
  getApiUrl: (path: string) => string
  onAccountCreated: (username: string, password: string, token: string, timezone: string) => void
}

export function AccountStep({ getApiUrl, onAccountCreated }: AccountStepProps) {
  const { t } = useTranslation(['common', 'auth', 'setup', 'validation'])

  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState("")

  // Account form
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const [confirmPassword, setConfirmPassword] = useState("")
  const [email, setEmail] = useState("")
  const [subscribeToNewsletter, setSubscribeToNewsletter] = useState(false)

  // Timezone (auto-detected from browser, saved silently on submit; the user
  // can adjust it on the next step — no need to clutter the registration form.)
  const browserTz = getBrowserTimezone()
  const selectedTimezone = COMMON_TIMEZONE_IDS.includes(browserTz as any) ? browserTz : "Asia/Shanghai"

  // Password validation
  const getPasswordErrors = (pwd: string): string[] => {
    const errors: string[] = []
    if (pwd.length < 8) errors.push(t('minPasswordLength', { ns: 'validation' }))
    if (!pwd.match(/[a-zA-Z]/)) errors.push(t('passwordNeedsLetter', { ns: 'validation' }))
    if (!pwd.match(/[0-9]/)) errors.push(t('passwordNeedsNumber', { ns: 'validation' }))
    return errors
  }
  const passwordErrors = getPasswordErrors(password)

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError("")

    if (password !== confirmPassword) {
      setError(t('passwordsDoNotMatch', { ns: 'validation' }))
      return
    }
    const pwdErrors = getPasswordErrors(password)
    if (pwdErrors.length > 0) {
      setError(pwdErrors[0])
      return
    }

    setIsLoading(true)
    try {
      // Create admin account
      const response = await fetch(getApiUrl('/setup/initialize'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username, password, email: email || undefined }),
      })

      const data = await response.json()
      if (!response.ok) {
        throw new Error(data.message || data.error || 'Failed to create admin account')
      }

      localStorage.setItem('neomind_token', data.token)
      localStorage.setItem('neomind_user', JSON.stringify(data.user))

      // Save timezone
      try {
        await fetch(getApiUrl('/settings/timezone'), {
          method: 'PUT',
          headers: {
            'Content-Type': 'application/json',
            'Authorization': `Bearer ${data.token}`,
          },
          body: JSON.stringify({ timezone: selectedTimezone }),
        })
      } catch (tzError) {
        console.warn('Failed to save timezone, continuing:', tzError)
      }

      // Newsletter subscription (non-blocking, best-effort — failures stay
      // silent; success gets a toast so the opt-in is observably real)
      if (subscribeToNewsletter && email?.trim()) {
        mcSubscribe(email.trim(), username)
          .then((r) => {
            if (r.result === "success") notifySuccess(t("setup:subscribedToast"))
          })
          .catch(() => {})
      }

      onAccountCreated(username, password, data.token, selectedTimezone)
    } catch (err) {
      setError(translateError(err instanceof Error ? err.message : String(err), t))
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <div className="viewport-full flex flex-col bg-background relative overflow-hidden">
      <SetupBackground />

      {/* Floating header — absolute so it doesn't consume vertical flow.
          The form is short enough to fit without scrolling; a full in-flow
          bar just pushed content down and caused unnecessary scroll. */}
      <SetupHeader />

      {/* Scroll area. `min-h-full` + `items-center` is the canonical
          "center when it fits, top-align when it scrolls" pattern. `pt-20/24`
          clears the floating header so a scrolled card never hides under it.
          `safe-bottom` clears the iPhone home indicator. */}
      <main className="relative z-10 flex-1 min-h-0 overflow-y-auto safe-bottom">
        <div className="min-h-full flex items-center justify-center px-4 pt-20 pb-6 sm:px-6 sm:pt-24 sm:pb-10">
          <div className="w-full max-w-md animate-fade-in-up">
            <div
              className="backdrop-blur-xl rounded-2xl p-6 sm:p-8 border shadow-md"
              style={{
                backgroundColor: 'color-mix(in oklch, var(--background) 72%, transparent)',
                borderColor: 'color-mix(in oklch, var(--border) 55%, transparent)',
              }}
            >
              {/* Title */}
              <h2 className="text-xl sm:text-2xl font-semibold mb-1.5 text-center tracking-tight">{t('setup:createAccount')}</h2>
              <p className="text-muted-foreground text-center mb-5 text-sm px-2">{t('setup:accountDescription')}</p>

              <form onSubmit={handleSubmit} className="flex flex-col gap-4">
                {/* Username */}
                <div>
                  <Label htmlFor="username" className="text-sm">{t('auth:username')}</Label>
                  <div className="relative mt-1.5">
                    <User className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
                    <Input
                      id="username"
                      type="text"
                      value={username}
                      onChange={(e) => setUsername(e.target.value)}
                      placeholder={t('setup:usernamePlaceholder')}
                      autoComplete="username"
                      required
                      minLength={3}
                      className="pl-9 h-10 bg-bg-70 border-border scroll-mb-32"
                    />
                  </div>
                </div>

                {/* Email */}
                <div>
                  <Label htmlFor="email" className="text-sm">{t('setup:email')} <span className="text-muted-foreground">({t('optional')})</span></Label>
                  <Input
                    id="email"
                    type="email"
                    value={email}
                    onChange={(e) => setEmail(e.target.value)}
                    placeholder={t('setup:emailPlaceholder')}
                    autoComplete="email"
                    className="h-10 bg-bg-70 border-border mt-1.5 scroll-mb-32"
                  />
                  {email?.trim() && (
                    <div className="flex items-center gap-2 mt-2">
                      <Checkbox
                        id="subscribe"
                        checked={subscribeToNewsletter}
                        onCheckedChange={(checked) => setSubscribeToNewsletter(!!checked)}
                      />
                      <label htmlFor="subscribe" className="text-xs text-muted-foreground cursor-pointer leading-tight">
                        {t('setup:subscribeNewsletter')}
                      </label>
                    </div>
                  )}
                </div>

                {/* Password */}
                <div>
                  <Label htmlFor="password" className="text-sm">{t('auth:password')}</Label>
                  <div className="relative mt-1.5">
                    <Lock className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
                    <PasswordInput
                      id="password"
                      value={password}
                      onChange={(e) => setPassword(e.target.value)}
                      placeholder={t('setup:passwordPlaceholder')}
                      autoComplete="new-password"
                      required
                      minLength={8}
                      className="pl-9 h-10 bg-bg-70 border-border scroll-mb-32"
                    />
                  </div>
                </div>

                {/* Confirm Password */}
                <div>
                  <Label htmlFor="confirmPassword" className="text-sm">{t('setup:confirmPassword')}</Label>
                  <div className="relative mt-1.5">
                    <Lock className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
                    <PasswordInput
                      id="confirmPassword"
                      value={confirmPassword}
                      onChange={(e) => setConfirmPassword(e.target.value)}
                      placeholder={t('setup:confirmPasswordPlaceholder')}
                      autoComplete="new-password"
                      required
                      className="pl-9 h-10 bg-bg-70 border-border scroll-mb-32"
                    />
                  </div>
                </div>

                {/* Password Strength */}
                {password && (
                  <div className="space-y-1.5">
                    <div className="text-xs text-muted-foreground">{t('setup:passwordStrength')}</div>
                    <div className="flex gap-1">
                      {passwordErrors.length === 0 ? (
                        <>{[1,2,3,4].map(i => <div key={i} className="h-1 flex-1 rounded-full bg-success" />)}</>
                      ) : password.length >= 8 ? (
                        <>
                          <div className="h-1 flex-1 rounded-full bg-success" />
                          <div className="h-1 flex-1 rounded-full bg-warning" />
                          <div className="h-1 flex-1 rounded-full bg-border" />
                          <div className="h-1 flex-1 rounded-full bg-border" />
                        </>
                      ) : (
                        <>
                          <div className="h-1 flex-1 rounded-full bg-error" />
                          <div className="h-1 flex-1 rounded-full bg-border" />
                          <div className="h-1 flex-1 rounded-full bg-border" />
                          <div className="h-1 flex-1 rounded-full bg-border" />
                        </>
                      )}
                    </div>
                    {passwordErrors.length > 0 && (
                      <div className="text-xs text-error">{passwordErrors[0]}</div>
                    )}
                  </div>
                )}

                {/* Timezone selector moved to CompleteStep — registration form
                    stays short. The auto-detected timezone is saved silently
                    on submit (see handleSubmit above). */}

                {/* Error */}
                {error && (
                  <div className="flex items-start gap-2 text-sm text-error bg-muted rounded-md p-3">
                    <Shield className="h-4 w-4 mt-0.5 flex-shrink-0" />
                    <span>{error}</span>
                  </div>
                )}

                {/* Submit — 44px touch target on mobile, 40px on desktop */}
                <Button
                  type="submit"
                  disabled={isLoading || !username || !password || !confirmPassword || passwordErrors.length > 0}
                  className="h-11 sm:h-10 w-full mt-1"
                  size="default"
                >
                  {isLoading ? t('setup:creating') : t('setup:getStarted')}
                  {!isLoading && <ArrowRight className="ml-2 h-4 w-4" />}
                </Button>
              </form>
            </div>
          </div>
        </div>
      </main>
    </div>
  )
}
