import { useState, useEffect } from "react"
import { useNavigate } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { Bot, Languages, Lock, User, Shield, Check, ArrowRight, ArrowLeft, Server, ChevronRight, Globe } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { BrandName, BrandLogoHorizontal } from "@/components/shared/BrandName"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { logError } from '@/lib/errors'

const languages = [
  { code: 'en', name: 'English' },
  { code: 'zh', name: '简体中文' },
]

type SetupStep = 'welcome' | 'account' | 'timezone' | 'llm' | 'complete'
type LlmProvider = 'ollama' | 'openai' | 'anthropic' | 'google' | 'xai'

interface LlmProviderInfo {
  id: LlmProvider
  name: string
  description: string
  defaultModel: string
  defaultEndpoint?: string
  needsApiKey: boolean
}

const llmProviders: LlmProviderInfo[] = [
  {
    id: 'ollama',
    name: 'Ollama',
    description: 'Local LLM runner - runs on your own machine',
    defaultModel: 'qwen3-vl:2b',
    defaultEndpoint: 'http://localhost:11434',
    needsApiKey: false,
  },
  {
    id: 'openai',
    name: 'OpenAI',
    description: 'GPT-4 and other OpenAI models',
    defaultModel: 'gpt-4o',
    needsApiKey: true,
  },
  {
    id: 'anthropic',
    name: 'Anthropic',
    description: 'Claude AI assistant',
    defaultModel: 'claude-3-5-sonnet-20241022',
    needsApiKey: true,
  },
  {
    id: 'google',
    name: 'Google AI',
    description: 'Gemini models',
    defaultModel: 'gemini-1.5-flash',
    needsApiKey: true,
  },
]

const timezoneOptions = [
  { id: 'Asia/Shanghai', name: '中国 (UTC+8)' },
  { id: 'Asia/Tokyo', name: '日本 (UTC+9)' },
  { id: 'Asia/Seoul', name: '韩国 (UTC+9)' },
  { id: 'Asia/Singapore', name: '新加坡 (UTC+8)' },
  { id: 'Europe/London', name: '伦敦 (UTC+0/+1)' },
  { id: 'Europe/Paris', name: '巴黎 (UTC+1/+2)' },
  { id: 'Europe/Berlin', name: '柏林 (UTC+1/+2)' },
  { id: 'America/New_York', name: '纽约 (UTC-5/-4)' },
  { id: 'America/Los_Angeles', name: '洛杉矶 (UTC-8/-7)' },
  { id: 'America/Chicago', name: '芝加哥 (UTC-6/-5)' },
  { id: 'Australia/Sydney', name: '悉尼 (UTC+10/+11)' },
  { id: 'UTC', name: 'UTC (UTC+0)' },
]

// Error translation helper
function translateError(error: string, t: (key: string, params?: Record<string, unknown>) => string): string {
  const lowerError = error.toLowerCase()
  if (lowerError.includes("password must be at least")) {
    return t("minPasswordLength", { ns: 'validation' })
  }
  if (lowerError.includes("username must be at least")) {
    return t("minUsernameLength", { ns: 'validation' })
  }
  if (lowerError.includes("password must contain")) {
    return t("passwordComplexity")
  }
  if (lowerError.includes("setup already completed")) {
    return t("setupAlreadyCompleted")
  }
  return error || t("setupFailed")
}

export function SetupPage() {
  const { t, i18n } = useTranslation(['common', 'auth', 'setup'])
  const navigate = useNavigate()
  const { login } = useStore()
  const { withErrorHandling } = useErrorHandler()

  const [step, setStep] = useState<SetupStep>('welcome')
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState("")

  // Account form state
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const [confirmPassword, setConfirmPassword] = useState("")
  const [email, setEmail] = useState("")

  // LLM config state
  const [selectedProvider, setSelectedProvider] = useState<LlmProvider>('ollama')
  const [llmModel, setLlmModel] = useState("qwen3-vl:2b")
  const [llmEndpoint, setLlmEndpoint] = useState("http://localhost:11434")
  const [llmApiKey, setLlmApiKey] = useState("")

  // Timezone state
  const [selectedTimezone, setSelectedTimezone] = useState("Asia/Shanghai")

  // Validate password
  const getPasswordErrors = (pwd: string): string[] => {
    const errors: string[] = []
    if (pwd.length < 8) {
      errors.push(t('minPasswordLength', { ns: 'validation' }))
    }
    if (!pwd.match(/[a-zA-Z]/)) {
      errors.push(t('passwordNeedsLetter', { ns: 'validation' }))
    }
    if (!pwd.match(/[0-9]/)) {
      errors.push(t('passwordNeedsNumber', { ns: 'validation' }))
    }
    return errors
  }

  const passwordErrors = getPasswordErrors(password)

  // Check setup status on mount
  useEffect(() => {
    checkSetupStatus()
  }, [])

  // Helper to get API base URL for current environment
  const getApiUrl = (path: string) => {
    const apiBase = (window as any).__TAURI__ ? 'http://localhost:9375/api' : '/api'
    return `${apiBase}${path}`
  }

  const checkSetupStatus = async () => {
    const result = await withErrorHandling(
      async () => {
        // Retry logic for Tauri environment where backend might be starting up
        const maxRetries = (window as any).__TAURI__ ? 15 : 3
        const initialDelay = (window as any).__TAURI__ ? 500 : 100

        for (let i = 0; i < maxRetries; i++) {
          try {
            const response = await fetch(getApiUrl('/setup/status'), {
              signal: AbortSignal.timeout(3000),
            })
            if (!response.ok) {
              throw new Error(`HTTP ${response.status}`)
            }
            return await response.json() as { setup_required: boolean }
          } catch {
            // Retry with exponential backoff
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

    if (result && !result.setup_required) {
      // Setup already completed, redirect to dashboard
      navigate('/dashboard')
    }
  }

  const handleAccountSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError("")

    // Validate passwords match
    if (password !== confirmPassword) {
      setError(t('passwordsDoNotMatch', { ns: 'validation' }))
      return
    }

    // Validate password strength
    const pwdErrors = getPasswordErrors(password)
    if (pwdErrors.length > 0) {
      setError(pwdErrors[0])
      return
    }

    setIsLoading(true)

    try {
      const response = await fetch(getApiUrl('/setup/initialize'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          username,
          password,
          email: email || undefined,
        }),
      })

      const data = await response.json()

      if (!response.ok) {
        throw new Error(data.message || data.error || 'Failed to create admin account')
      }

      // Store token for next steps
      localStorage.setItem('neomind_token', data.token)
      localStorage.setItem('neomind_user', JSON.stringify(data.user))

      // Move to timezone step
      setStep('timezone')
    } catch (err) {
      setError(translateError(err instanceof Error ? err.message : String(err), t))
    } finally {
      setIsLoading(false)
    }
  }

  const handleLlmSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError("")

    setIsLoading(true)

    try {
      // Save timezone setting first
      try {
        await fetch(getApiUrl('/settings/timezone'), {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ timezone: selectedTimezone }),
        })
      } catch (tzError) {
        logError(tzError, { operation: 'Save timezone' })
        // Continue even if timezone save fails
      }

      // Save LLM config
      await fetch(getApiUrl('/setup/llm-config'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          provider: selectedProvider,
          model: llmModel,
          endpoint: llmEndpoint || undefined,
          api_key: llmApiKey || undefined,
        }),
      })

      // Complete setup
      const response = await fetch(getApiUrl('/setup/complete'), {
        method: 'POST',
      })

      const data = await response.json()

      if (!response.ok) {
        throw new Error(data.message || 'Failed to complete setup')
      }

      // Move to complete step
      setStep('complete')
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setIsLoading(false)
    }
  }

  const handleProviderSelect = (provider: LlmProvider) => {
    setSelectedProvider(provider)
    const providerInfo = llmProviders.find(p => p.id === provider)
    if (providerInfo) {
      setLlmModel(providerInfo.defaultModel)
      if (providerInfo.defaultEndpoint) {
        setLlmEndpoint(providerInfo.defaultEndpoint)
      }
    }
  }

  const handleComplete = () => {
    // Auto-login with the created account
    login(username, password, true).then(() => {
      navigate('/dashboard')
    }).catch(() => {
      navigate('/login')
    })
  }

  // ==================== WELCOME STEP ====================
  if (step === 'welcome') {
    return (
      <div className="min-h-screen flex flex-col bg-background overflow-hidden">
        {/* Background Effects - Same as login page */}
        <div className="fixed inset-0">
          <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted/10" />
          <div className="absolute inset-0" style={{
            backgroundImage: 'radial-gradient(circle, #80808015 1px, transparent 1px)',
            backgroundSize: '32px 32px'
          }} />
          <svg className="absolute inset-0 w-full h-full opacity-[0.03]" xmlns="http://www.w3.org/2000/svg">
            <defs>
              <pattern id="network-grid" width="120" height="120" patternUnits="userSpaceOnUse">
                <circle cx="60" cy="60" r="1.5" fill="currentColor" />
                <line x1="60" y1="0" x2="60" y2="120" stroke="currentColor" strokeWidth="0.5" />
                <line x1="0" y1="60" x2="120" y2="60" stroke="currentColor" strokeWidth="0.5" />
              </pattern>
            </defs>
            <rect width="100%" height="100%" fill="url(#network-grid)" />
          </svg>
          <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] bg-primary/5 dark:bg-primary/10 rounded-full blur-3xl animate-pulse" style={{ animationDuration: '8s' }} />
          <div className="absolute top-[15%] left-[10%] w-32 h-32 bg-blue-500/5 dark:bg-blue-500/10 rounded-full blur-2xl animate-pulse" style={{ animationDuration: '6s', animationDelay: '0s' }} />
          <div className="absolute bottom-[20%] right-[15%] w-40 h-40 bg-purple-500/5 dark:bg-purple-500/10 rounded-full blur-2xl animate-pulse" style={{ animationDuration: '7s', animationDelay: '1s' }} />
          <div className="absolute top-[30%] right-[20%] w-24 h-24 bg-cyan-500/5 dark:bg-cyan-500/10 rounded-full blur-2xl animate-pulse" style={{ animationDuration: '5s', animationDelay: '2s' }} />
          <div className="absolute bottom-[30%] left-[20%] w-28 h-28 bg-indigo-500/5 dark:bg-indigo-500/10 rounded-full blur-2xl animate-pulse" style={{ animationDuration: '6s', animationDelay: '3s' }} />
        </div>

        {/* Top Header */}
        <header className="relative z-10 backdrop-blur-sm">
          <div className="flex items-center justify-between px-6 h-16">
            <div className="flex items-center gap-3">
              <BrandLogoHorizontal className="h-7" />
            </div>

            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="sm" className="gap-1.5">
                  <Languages className="h-4 w-4" />
                  {languages.find(l => l.code === i18n.language)?.name || 'Language'}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-[130px]">
                {languages.map((lang) => (
                  <DropdownMenuItem
                    key={lang.code}
                    onClick={() => i18n.changeLanguage(lang.code)}
                    className={i18n.language === lang.code ? 'bg-muted' : ''}
                  >
                    {lang.name}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </header>

        {/* Main Content */}
        <main className="relative z-10 flex-1 flex items-center justify-center px-6 py-12">
          <div className="w-full max-w-lg">
            <div className="bg-background/50 dark:bg-background/30 backdrop-blur-md rounded-xl p-8">
              {/* Welcome Icon */}
              <div className="flex justify-center mb-6">
                <div className="flex size-16 items-center justify-center rounded-full bg-primary/10 text-primary dark:bg-primary/20">
                  <Bot className="size-8" />
                </div>
              </div>

              {/* Welcome Title */}
              <h2 className="text-3xl font-semibold mb-3 text-center">{t('setup:title')}</h2>
              <p className="text-muted-foreground text-center mb-8">{t('setup:welcomeMessage')}</p>

              {/* Features List */}
              <div className="space-y-3 mb-8">
                {[
                  { icon: User, text: t('setup:featureAccount') },
                  { icon: Globe, text: t('setup:featureTimezone') },
                  { icon: Server, text: t('setup:featureLlm') },
                  { icon: Shield, text: t('setup:featureSecure') },
                ].map((feature, index) => (
                  <div key={index} className="flex items-center gap-3 text-sm">
                    <div className="flex size-8 items-center justify-center rounded-full bg-primary/5 text-primary">
                      <feature.icon className="size-4" />
                    </div>
                    <span>{feature.text}</span>
                  </div>
                ))}
              </div>

              {/* Start Button */}
              <Button
                onClick={() => setStep('account')}
                className="w-full h-11"
                size="default"
              >
                {t('setup:getStarted')}
                <ArrowRight className="ml-2 h-4 w-4" />
              </Button>
            </div>
          </div>
        </main>
      </div>
    )
  }

  // ==================== ACCOUNT STEP ====================
  if (step === 'account') {
    return (
      <div className="min-h-screen flex flex-col bg-background overflow-hidden">
        {/* Background - same as login */}
        <div className="fixed inset-0">
          <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted/10" />
          <div className="absolute inset-0" style={{
            backgroundImage: 'radial-gradient(circle, #80808015 1px, transparent 1px)',
            backgroundSize: '32px 32px'
          }} />
          <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] bg-primary/5 dark:bg-primary/10 rounded-full blur-3xl animate-pulse" style={{ animationDuration: '8s' }} />
        </div>

        {/* Header */}
        <header className="relative z-10 backdrop-blur-sm">
          <div className="flex items-center justify-between px-6 h-16">
            <div className="flex items-center gap-3">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setStep('welcome')}
                className="gap-1.5"
              >
                <ArrowLeft className="h-4 w-4" />
                {t('setup:back')}
              </Button>
              <span className="text-sm text-muted-foreground">{t('setup:step', { current: 1, total: 3 })}</span>
            </div>

            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="sm" className="gap-1.5">
                  <Languages className="h-4 w-4" />
                  {languages.find(l => l.code === i18n.language)?.name || 'Language'}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-[130px]">
                {languages.map((lang) => (
                  <DropdownMenuItem
                    key={lang.code}
                    onClick={() => i18n.changeLanguage(lang.code)}
                    className={i18n.language === lang.code ? 'bg-muted' : ''}
                  >
                    {lang.name}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </header>

        {/* Main Content */}
        <main className="relative z-10 flex-1 flex items-center justify-center px-6 py-12">
          <div className="w-full max-w-md">
            <div className="bg-background/50 dark:bg-background/30 backdrop-blur-md rounded-xl p-8">
              {/* Progress Indicator */}
              <div className="flex items-center justify-center gap-2 mb-6">
                <div className="flex size-8 items-center justify-center rounded-full bg-primary text-primary-foreground text-sm font-bold">
                  1
                </div>
                <div className="w-8 h-0.5 bg-muted" />
                <div className="flex size-8 items-center justify-center rounded-full bg-muted text-muted-foreground text-sm font-bold">
                  2
                </div>
                <div className="w-8 h-0.5 bg-muted" />
                <div className="flex size-8 items-center justify-center rounded-full bg-muted text-muted-foreground text-sm font-bold">
                  3
                </div>
              </div>

              {/* Title */}
              <h2 className="text-2xl font-semibold mb-2 text-center">{t('setup:createAccount')}</h2>
              <p className="text-muted-foreground text-center mb-6 text-sm">{t('setup:accountDescription')}</p>

              {/* Form */}
              <form onSubmit={handleAccountSubmit} className="flex flex-col gap-4">
                {/* Username Field */}
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
                      className="pl-9 h-10 bg-background/70 dark:bg-background/30 border-border/50 dark:border-border/30"
                    />
                  </div>
                </div>

                {/* Email Field (Optional) */}
                <div>
                  <Label htmlFor="email" className="text-sm">{t('setup:email')} <span className="text-muted-foreground">({t('optional')})</span></Label>
                  <Input
                    id="email"
                    type="email"
                    value={email}
                    onChange={(e) => setEmail(e.target.value)}
                    placeholder={t('setup:emailPlaceholder')}
                    autoComplete="email"
                    className="h-10 bg-background/70 dark:bg-background/30 border-border/50 dark:border-border/30 mt-1.5"
                  />
                </div>

                {/* Password Field */}
                <div>
                  <Label htmlFor="password" className="text-sm">{t('auth:password')}</Label>
                  <div className="relative mt-1.5">
                    <Lock className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
                    <Input
                      id="password"
                      type="password"
                      value={password}
                      onChange={(e) => setPassword(e.target.value)}
                      placeholder={t('setup:passwordPlaceholder')}
                      autoComplete="new-password"
                      required
                      minLength={8}
                      className="pl-9 h-10 bg-background/70 dark:bg-background/30 border-border/50 dark:border-border/30"
                    />
                  </div>
                </div>

                {/* Confirm Password Field */}
                <div>
                  <Label htmlFor="confirmPassword" className="text-sm">{t('setup:confirmPassword')}</Label>
                  <div className="relative mt-1.5">
                    <Lock className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
                    <Input
                      id="confirmPassword"
                      type="password"
                      value={confirmPassword}
                      onChange={(e) => setConfirmPassword(e.target.value)}
                      placeholder={t('setup:confirmPasswordPlaceholder')}
                      autoComplete="new-password"
                      required
                      className="pl-9 h-10 bg-background/70 dark:bg-background/30 border-border/50 dark:border-border/30"
                    />
                  </div>
                </div>

                {/* Password Strength Indicator */}
                {password && (
                  <div className="space-y-1.5">
                    <div className="text-xs text-muted-foreground">{t('setup:passwordStrength')}</div>
                    <div className="flex gap-1">
                      {passwordErrors.length === 0 ? (
                        <>
                          <div className="h-1 flex-1 rounded-full bg-green-500" />
                          <div className="h-1 flex-1 rounded-full bg-green-500" />
                          <div className="h-1 flex-1 rounded-full bg-green-500" />
                          <div className="h-1 flex-1 rounded-full bg-green-500" />
                        </>
                      ) : password.length >= 8 ? (
                        <>
                          <div className="h-1 flex-1 rounded-full bg-green-500" />
                          <div className="h-1 flex-1 rounded-full bg-yellow-500" />
                          <div className="h-1 flex-1 rounded-full bg-border" />
                          <div className="h-1 flex-1 rounded-full bg-border" />
                        </>
                      ) : (
                        <>
                          <div className="h-1 flex-1 rounded-full bg-red-500" />
                          <div className="h-1 flex-1 rounded-full bg-border" />
                          <div className="h-1 flex-1 rounded-full bg-border" />
                          <div className="h-1 flex-1 rounded-full bg-border" />
                        </>
                      )}
                    </div>
                    {passwordErrors.length > 0 && (
                      <div className="text-xs text-destructive">{passwordErrors[0]}</div>
                    )}
                  </div>
                )}

                {/* Error Message */}
                {error && (
                  <div className="flex items-start gap-2 text-sm text-destructive bg-destructive/10 rounded-md p-3">
                    <Shield className="h-4 w-4 mt-0.5 flex-shrink-0" />
                    <span>{error}</span>
                  </div>
                )}

                {/* Submit Button */}
                <Button
                  type="submit"
                  disabled={isLoading || !username || !password || !confirmPassword || passwordErrors.length > 0}
                  className="h-10 w-full mt-2"
                  size="default"
                >
                  {isLoading ? t('setup:creating') : t('setup:continue')}
                  {!isLoading && <ArrowRight className="ml-2 h-4 w-4" />}
                </Button>
              </form>
            </div>
          </div>
        </main>
      </div>
    )
  }

  // ==================== TIMEZONE STEP ====================
  if (step === 'timezone') {
    // Function to format current time in timezone
    const formatTimeInTimezone = (tz: string) => {
      try {
        const now = new Date()
        return new Intl.DateTimeFormat('zh-CN', {
          hour: '2-digit',
          minute: '2-digit',
          second: '2-digit',
          timeZone: tz,
          hour12: false,
        }).format(now)
      } catch {
        return '--:--:--'
      }
    }

    // Handle timezone continue
    const handleTimezoneContinue = async () => {
      setStep('llm')
    }

    return (
      <div className="min-h-screen flex flex-col bg-background overflow-hidden">
        {/* Background */}
        <div className="fixed inset-0">
          <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted/10" />
          <div className="absolute inset-0" style={{
            backgroundImage: 'radial-gradient(circle, #80808015 1px, transparent 1px)',
            backgroundSize: '32px 32px'
          }} />
          <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] bg-primary/5 dark:bg-primary/10 rounded-full blur-3xl animate-pulse" style={{ animationDuration: '8s' }} />
        </div>

        {/* Header */}
        <header className="relative z-10 backdrop-blur-sm">
          <div className="flex items-center justify-between px-6 h-16">
            <div className="flex items-center gap-3">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setStep('account')}
                className="gap-1.5"
              >
                <ArrowLeft className="h-4 w-4" />
                {t('setup:back')}
              </Button>
              <span className="text-sm text-muted-foreground">{t('setup:step', { current: 2, total: 3 })}</span>
            </div>

            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="sm" className="gap-1.5">
                  <Languages className="h-4 w-4" />
                  {languages.find(l => l.code === i18n.language)?.name || 'Language'}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-[130px]">
                {languages.map((lang) => (
                  <DropdownMenuItem
                    key={lang.code}
                    onClick={() => i18n.changeLanguage(lang.code)}
                    className={i18n.language === lang.code ? 'bg-muted' : ''}
                  >
                    {lang.name}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </header>

        {/* Main Content */}
        <main className="relative z-10 flex-1 flex items-center justify-center px-6 py-12">
          <div className="w-full max-w-md">
            <div className="bg-background/50 dark:bg-background/30 backdrop-blur-md rounded-xl p-8">
              {/* Progress Indicator */}
              <div className="flex items-center justify-center gap-2 mb-6">
                <div className="flex size-8 items-center justify-center rounded-full bg-primary text-primary-foreground text-sm font-bold">
                  <Check className="size-4" />
                </div>
                <div className="w-12 h-0.5 bg-primary" />
                <div className="flex size-8 items-center justify-center rounded-full bg-primary text-primary-foreground text-sm font-bold">
                  2
                </div>
                <div className="w-12 h-0.5 bg-muted" />
                <div className="flex size-8 items-center justify-center rounded-full bg-muted text-muted-foreground text-sm font-bold">
                  3
                </div>
              </div>

              {/* Title */}
              <h2 className="text-2xl font-semibold mb-2 text-center">{t('setup:timezoneConfig')}</h2>
              <p className="text-muted-foreground text-center mb-6 text-sm">{t('setup:timezoneDescription')}</p>

              {/* Timezone Selection */}
              <div className="space-y-4">
                <div>
                  <Label className="text-sm">{t('setup:selectTimezone')}</Label>
                  <div className="grid grid-cols-2 gap-2 mt-3 max-h-[180px] overflow-y-auto">
                    {timezoneOptions.map((tz) => (
                      <button
                        key={tz.id}
                        type="button"
                        onClick={() => setSelectedTimezone(tz.id)}
                        className={`
                          flex flex-col items-start gap-1 p-2 rounded-lg border text-left transition-colors
                          ${selectedTimezone === tz.id
                            ? 'border-primary bg-primary/5 dark:bg-primary/10'
                            : 'border-border hover:bg-muted/50'
                          }
                        `}
                      >
                        <span className="font-medium text-xs">{tz.name}</span>
                        <span className="text-[10px] text-muted-foreground font-mono">
                          {formatTimeInTimezone(tz.id)}
                        </span>
                      </button>
                    ))}
                  </div>
                </div>

                {/* Current Time Preview */}
                <div className="p-4 bg-muted/30 dark:bg-muted/10 rounded-lg">
                  <div className="text-center">
                    <div className="text-xs text-muted-foreground mb-1">{t('setup:currentTimeInTimezone')}</div>
                    <div className="text-2xl font-mono font-medium">
                      {formatTimeInTimezone(selectedTimezone)}
                    </div>
                    <div className="text-xs text-muted-foreground mt-1">{selectedTimezone}</div>
                  </div>
                </div>

                {/* Continue Button */}
                <Button
                  onClick={handleTimezoneContinue}
                  className="w-full h-10"
                  size="default"
                >
                  {t('setup:continue')}
                  <ArrowRight className="ml-2 h-4 w-4" />
                </Button>
              </div>
            </div>
          </div>
        </main>
      </div>
    )
  }

  // ==================== LLM CONFIG STEP ====================
  if (step === 'llm') {
    return (
      <div className="min-h-screen flex flex-col bg-background overflow-hidden">
        {/* Background */}
        <div className="fixed inset-0">
          <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted/10" />
          <div className="absolute inset-0" style={{
            backgroundImage: 'radial-gradient(circle, #80808015 1px, transparent 1px)',
            backgroundSize: '32px 32px'
          }} />
          <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] bg-primary/5 dark:bg-primary/10 rounded-full blur-3xl animate-pulse" style={{ animationDuration: '8s' }} />
        </div>

        {/* Header */}
        <header className="relative z-10 backdrop-blur-sm">
          <div className="flex items-center justify-between px-6 h-16">
            <div className="flex items-center gap-3">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setStep('timezone')}
                className="gap-1.5"
              >
                <ArrowLeft className="h-4 w-4" />
                {t('setup:back')}
              </Button>
              <span className="text-sm text-muted-foreground">{t('setup:step', { current: 3, total: 3 })}</span>
            </div>

            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="sm" className="gap-1.5">
                  <Languages className="h-4 w-4" />
                  {languages.find(l => l.code === i18n.language)?.name || 'Language'}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-[130px]">
                {languages.map((lang) => (
                  <DropdownMenuItem
                    key={lang.code}
                    onClick={() => i18n.changeLanguage(lang.code)}
                    className={i18n.language === lang.code ? 'bg-muted' : ''}
                  >
                    {lang.name}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </header>

        {/* Main Content */}
        <main className="relative z-10 flex-1 flex items-center justify-center px-6 py-12">
          <div className="w-full max-w-md">
            <div className="bg-background/50 dark:bg-background/30 backdrop-blur-md rounded-xl p-8">
              {/* Progress Indicator */}
              <div className="flex items-center justify-center gap-2 mb-6">
                <div className="flex size-8 items-center justify-center rounded-full bg-primary text-primary-foreground text-sm font-bold">
                  <Check className="size-4" />
                </div>
                <div className="w-8 h-0.5 bg-primary" />
                <div className="flex size-8 items-center justify-center rounded-full bg-primary text-primary-foreground text-sm font-bold">
                  <Check className="size-4" />
                </div>
                <div className="w-8 h-0.5 bg-primary" />
                <div className="flex size-8 items-center justify-center rounded-full bg-primary text-primary-foreground text-sm font-bold">
                  3
                </div>
              </div>

              {/* Title */}
              <h2 className="text-2xl font-semibold mb-2 text-center">{t('setup:llmConfig')}</h2>
              <p className="text-muted-foreground text-center mb-6 text-sm">{t('setup:llmDescription')}</p>

              <form onSubmit={handleLlmSubmit} className="flex flex-col gap-4">
                {/* Provider Selection */}
                <div>
                  <Label className="text-sm">{t('setup:llmProvider')}</Label>
                  <div className="grid grid-cols-2 gap-2 mt-2">
                    {llmProviders.map((provider) => (
                      <button
                        key={provider.id}
                        type="button"
                        onClick={() => handleProviderSelect(provider.id)}
                        className={`
                          flex flex-col items-start gap-1 p-3 rounded-lg border text-left transition-colors
                          ${selectedProvider === provider.id
                            ? 'border-primary bg-primary/5 dark:bg-primary/10'
                            : 'border-border hover:bg-muted/50'
                          }
                        `}
                      >
                        <span className="font-medium text-sm">{provider.name}</span>
                        <span className="text-xs text-muted-foreground line-clamp-1">{provider.description}</span>
                      </button>
                    ))}
                  </div>
                </div>

                {/* Model Name */}
                <div>
                  <Label htmlFor="model" className="text-sm">{t('setup:modelName')}</Label>
                  <Input
                    id="model"
                    type="text"
                    value={llmModel}
                    onChange={(e) => setLlmModel(e.target.value)}
                    placeholder={t('setup:modelPlaceholder')}
                    required
                    className="h-10 bg-background/70 dark:bg-background/30 border-border/50 dark:border-border/30 mt-1.5"
                  />
                </div>

                {/* Endpoint (for Ollama) */}
                {selectedProvider === 'ollama' && (
                  <div>
                    <Label htmlFor="endpoint" className="text-sm">{t('setup:endpoint')}</Label>
                    <Input
                      id="endpoint"
                      type="text"
                      value={llmEndpoint}
                      onChange={(e) => setLlmEndpoint(e.target.value)}
                      placeholder="http://localhost:11434"
                      className="h-10 bg-background/70 dark:bg-background/30 border-border/50 dark:border-border/30 mt-1.5"
                    />
                  </div>
                )}

                {/* API Key (for cloud providers) */}
                {llmProviders.find(p => p.id === selectedProvider)?.needsApiKey && (
                  <div>
                    <Label htmlFor="apiKey" className="text-sm">{t('setup:apiKey')}</Label>
                    <Input
                      id="apiKey"
                      type="password"
                      value={llmApiKey}
                      onChange={(e) => setLlmApiKey(e.target.value)}
                      placeholder={t('setup:apiKeyPlaceholder')}
                      className="h-10 bg-background/70 dark:bg-background/30 border-border/50 dark:border-border/30 mt-1.5"
                    />
                    <p className="text-xs text-muted-foreground mt-1">{t('setup:apiKeyHint')}</p>
                  </div>
                )}

                {/* Error Message */}
                {error && (
                  <div className="flex items-start gap-2 text-sm text-destructive bg-destructive/10 rounded-md p-3">
                    <Shield className="h-4 w-4 mt-0.5 flex-shrink-0" />
                    <span>{error}</span>
                  </div>
                )}

                {/* Buttons */}
                <div className="flex gap-3 mt-2">
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => setStep('account')}
                    className="flex-1 h-10"
                  >
                    {t('setup:back')}
                  </Button>
                  <Button
                    type="submit"
                    disabled={isLoading}
                    className="flex-1 h-10"
                  >
                    {isLoading ? t('setup:configuring') : t('setup:complete')}
                  </Button>
                </div>

                {/* Skip option */}
                <button
                  type="button"
                  onClick={handleComplete}
                  className="text-xs text-muted-foreground hover:text-foreground text-center w-full"
                >
                  {t('setup:skipLlm')} →
                </button>
              </form>
            </div>
          </div>
        </main>
      </div>
    )
  }

  // ==================== COMPLETE STEP ====================
  if (step === 'complete') {
    return (
      <div className="min-h-screen flex flex-col bg-background overflow-hidden">
        {/* Background with celebration */}
        <div className="fixed inset-0">
          <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted/10" />
          <div className="absolute inset-0" style={{
            backgroundImage: 'radial-gradient(circle, #80808015 1px, transparent 1px)',
            backgroundSize: '32px 32px'
          }} />
          <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] bg-green-500/10 dark:bg-green-500/20 rounded-full blur-3xl animate-pulse" style={{ animationDuration: '3s' }} />
        </div>

        {/* Main Content */}
        <main className="relative z-10 flex-1 flex items-center justify-center px-6 py-12">
          <div className="w-full max-w-md text-center">
            <div className="bg-background/50 dark:bg-background/30 backdrop-blur-md rounded-xl p-8">
              {/* Success Icon */}
              <div className="flex justify-center mb-6">
                <div className="flex size-16 items-center justify-center rounded-full bg-green-500/10 text-green-500 dark:bg-green-500/20">
                  <Check className="size-8" />
                </div>
              </div>

              {/* Success Message */}
              <h2 className="text-2xl font-semibold mb-2">{t('setup:completeTitle')}</h2>
              <p className="text-muted-foreground mb-8">{t('setup:completeMessage')}</p>

              {/* Created Account Info */}
              <div className="bg-muted/30 dark:bg-muted/10 rounded-lg p-4 mb-6 text-left">
                <div className="text-sm font-medium mb-2">{t('setup:accountCreated')}:</div>
                <div className="flex items-center gap-2">
                  <User className="h-4 w-4 text-muted-foreground" />
                  <span className="font-mono">{username}</span>
                </div>
              </div>

              {/* Continue Button */}
              <Button
                onClick={handleComplete}
                className="w-full h-10"
                size="default"
              >
                {t('setup:goToDashboard')}
                <ChevronRight className="ml-2 h-4 w-4" />
              </Button>
            </div>
          </div>
        </main>
      </div>
    )
  }

  return null
}
