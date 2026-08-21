import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { useNavigate } from "react-router-dom"
import { useStore } from "@/store"
import { Languages, Lock, User, Shield, ArrowLeft, Server, Globe, ChevronRight, Check, KeyRound, Eye, EyeOff } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Input } from "@/components/ui/input"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { BrandLogoHorizontal } from "@/components/shared/BrandName"
import { HoneycombBackground } from "@/components/shared/HoneycombBackground"
import { LoadingState } from "@/components/shared/LoadingState"
import { forceViewportReset } from "@/hooks/useVisualViewport"
import { textNano } from '@/design-system/tokens/typography'
import { tokenManager, getApiBase, getApiKey, setApiBase, clearApiKey, setApiKey } from "@/lib/api"
import { handleWindowDragMouseDown } from "@/lib/windowDrag"
import { INSTANCE_CACHE_KEY, CURRENT_INSTANCE_KEY, PENDING_SWITCH_KEY } from "@/lib/instance-constants"
import { decryptApiKey } from "@/store/slices/instanceSlice"

const languages = [
  { code: 'en', name: 'English' },
  { code: 'zh', name: '简体中文' },
]

// LocalStorage keys for remembering credentials
const CREDENTIALS_KEY = 'neomind_remembered_credentials'

interface CachedInstance {
  id: string
  name: string
  url: string
  api_key?: string
  encrypted_key?: string
  is_local: boolean
  last_status: string
}

function getCachedInstances(): CachedInstance[] {
  try {
    const raw = localStorage.getItem(INSTANCE_CACHE_KEY)
    return raw ? JSON.parse(raw) : []
  } catch {
    return []
  }
}

// Error translation helper
function translateError(error: string, t: (key: string, params?: Record<string, unknown>) => string): string {
  const lowerError = error.toLowerCase()
  if (lowerError.includes("invalid username or password") || lowerError.includes("invalid credentials")) {
    return t("invalidCredentials")
  }
  if (lowerError.includes("user not found")) {
    return t("userNotFound")
  }
  if (lowerError.includes("user disabled") || lowerError.includes("account is disabled")) {
    return t("accountDisabled")
  }
  if (lowerError.includes("password must be at least")) {
    return t("minPasswordLength", { ns: 'validation' })
  }
  if (lowerError.includes("username must be at least")) {
    return t("minUsernameLength", { ns: 'validation' })
  }
  if (lowerError.includes("user already exists")) {
    return t("userAlreadyExists")
  }
  if (lowerError.includes("unauthorized")) {
    return t("authFailed")
  }
  return error || t("loginFailed")
}

export function LoginPage() {
  const { t, i18n } = useTranslation(['common', 'auth', 'instances'])
  const { login, checkAuthStatus } = useStore()
  const navigate = useNavigate()
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const [showPassword, setShowPassword] = useState(false)
  const [rememberMe, setRememberMe] = useState(false)
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState("")
  const [isFirstSetup, setIsFirstSetup] = useState<boolean | null>(null)
  const [checkingAuth, setCheckingAuth] = useState(true)
  const [hasLoadedCredentials, setHasLoadedCredentials] = useState(false)
  const [showInstancePicker, setShowInstancePicker] = useState(false)

  const cachedInstances = getCachedInstances()
  const [instanceStatuses, setInstanceStatuses] = useState<Record<string, string>>({})

  // Live health-check each backend when the picker opens, so the list shows
  // online/offline status (like InstanceManagerDialog on the home page).
  useEffect(() => {
    if (!showInstancePicker) return
    const instances = getCachedInstances()
    setInstanceStatuses(Object.fromEntries(instances.map(i => [i.id, 'checking'])))
    instances.forEach(async (inst) => {
      const base = inst.is_local ? 'http://localhost:9375' : inst.url.replace(/\/+$/, '')
      try {
        const res = await fetch(`${base}/api/setup/status`, { signal: AbortSignal.timeout(4000) })
        setInstanceStatuses(prev => ({ ...prev, [inst.id]: res.ok ? 'online' : 'offline' }))
      } catch {
        setInstanceStatuses(prev => ({ ...prev, [inst.id]: 'offline' }))
      }
    })
  }, [showInstancePicker])
  const apiBase = getApiBase()
  const isRemote = !!(apiBase && apiBase !== '/api' && !apiBase.includes('localhost') && !apiBase.includes('127.0.0.1'))

  // Handle instance switch — use encrypted_key from backend
  const handleInstanceSwitch = async (instance: CachedInstance) => {
    const fullKey = instance.encrypted_key ? decryptApiKey(instance.encrypted_key) : ''
    useStore.setState({
      switchingState: 'switching',
      switchingError: null,
      currentInstanceId: instance.id,
    })
    // Pre-validate the target backend BEFORE touching localStorage or reloading.
    // If it's down/non-existent, error + revert INSTANTLY (no reload, no 30s
    // StartupLoading wait) — the user stays on the current instance and sees
    // the error/revert overlay. Only reload if the backend is actually
    // reachable. A dead backend (e.g. connection refused) fails in <100ms.
    try {
      const base = instance.is_local ? 'http://localhost:9375' : instance.url.replace(/\/+$/, '')
      const res = await fetch(`${base}/api/setup/status`, { signal: AbortSignal.timeout(3000) })
      if (!res.ok) {
        useStore.setState({ switchingState: 'error', switchingError: 'unreachable' })
        return
      }
    } catch {
      useStore.setState({ switchingState: 'error', switchingError: 'unreachable' })
      return
    }
    // Only close the picker + reload once the backend is confirmed reachable.
    // If pre-validation failed above, the picker stays open UNDER the error
    // overlay, so "Return" brings the user back to the picker (not the login form).
    setShowInstancePicker(false)
    localStorage.setItem(CURRENT_INSTANCE_KEY, instance.id)
    localStorage.setItem(PENDING_SWITCH_KEY, JSON.stringify({
      targetId: instance.id,
      previousId: 'local-default',
      apiUrl: instance.is_local ? '' : `${instance.url}/api`,
      apiKey: instance.is_local ? '' : fullKey,
    }))
    window.location.reload()
  }

  const handleBackToLocal = () => {
    localStorage.setItem(CURRENT_INSTANCE_KEY, 'local-default')
    setApiBase('')
    clearApiKey()
    window.location.reload()
  }

  // Check if already authenticated on mount
  useEffect(() => {
    const checkExistingAuth = async () => {
      const token = tokenManager.getToken()
      const apiKey = getApiKey()
      if (token) {
        try {
          await checkAuthStatus()
          navigate('/', { replace: true })
          return
        } catch {
          tokenManager.clearToken()
        }
      } else if (apiKey) {
        checkAuthStatus()
        navigate('/', { replace: true })
        return
      }
      setCheckingAuth(false)
    }
    checkExistingAuth()
  }, [checkAuthStatus, navigate])

  // Load saved credentials on mount
  useEffect(() => {
    if (checkingAuth) return

    const checkSetupStatus = async () => {
      const apiBase = getApiBase()
      try {
        const response = await fetch(`${apiBase}/setup/status`, {
          signal: AbortSignal.timeout(5000),
        })
        if (response.ok) {
          const data = await response.json() as { setup_required: boolean }
          setIsFirstSetup(data.setup_required)
          if (data.setup_required) {
            navigate('/setup', { replace: true })
          }
        } else {
          setIsFirstSetup(false)
        }
      } catch {
        setIsFirstSetup(false)
      }
    }
    checkSetupStatus()

    try {
      const saved = localStorage.getItem(CREDENTIALS_KEY)
      if (saved) {
        const credentials = JSON.parse(saved)
        if (credentials.username) setUsername(credentials.username)
        if (credentials.password) setPassword(credentials.password)
        if (credentials.rememberMe !== undefined) setRememberMe(credentials.rememberMe)
        setHasLoadedCredentials(true)
      }
    } catch { /* ignore */ }

    const localToken = localStorage.getItem('neomind_token')
    if (localToken) {
      ;(async () => {
        try {
          await checkAuthStatus()
          navigate('/', { replace: true })
        } catch {
          tokenManager.clearToken()
        }
      })()
    }
  }, [checkingAuth, navigate])

  // Save credentials when rememberMe changes
  useEffect(() => {
    if (!hasLoadedCredentials) return
    try {
      if (rememberMe) {
        const saved = localStorage.getItem(CREDENTIALS_KEY)
        const existing = saved ? JSON.parse(saved) : {}
        localStorage.setItem(CREDENTIALS_KEY, JSON.stringify({
          username: username || existing.username,
          password: existing.password || '',
          rememberMe: true,
        }))
      } else {
        localStorage.removeItem(CREDENTIALS_KEY)
      }
    } catch { /* ignore */ }
  }, [rememberMe, username, hasLoadedCredentials])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError("")
    setIsLoading(true)

    try {
      await login(username, password, rememberMe)
      if (rememberMe) {
        try { localStorage.setItem(CREDENTIALS_KEY, JSON.stringify({ username, password, rememberMe: true })) } catch { /* ignore */ }
      } else {
        try { localStorage.removeItem(CREDENTIALS_KEY) } catch { /* ignore */ }
      }
      setHasLoadedCredentials(true)
      forceViewportReset()
      navigate('/', { replace: true })
    } catch (err) {
      setError(translateError(err instanceof Error ? err.message : String(t('auth:loginFailed')), t))
      forceViewportReset()
    } finally {
      setIsLoading(false)
    }
  }

  const handleBackdropClick = () => {
    forceViewportReset()
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur()
  }

  // Show loading while checking authentication
  if (checkingAuth) {
    return (
      <div className="flex flex-col bg-background relative overflow-hidden viewport-full items-center justify-center">
        <LoadingState size="lg" />
      </div>
    )
  }

  // Full-screen instance picker
  if (showInstancePicker) {
    return (
      <div className="flex flex-col bg-popover viewport-full">
        {/* Header */}
        <header
          className="flex items-center gap-3 px-4 sm:px-6 py-3.5 border-b border-border"
          style={{ paddingTop: "calc(0.875rem + env(safe-area-inset-top, 0px) + var(--titlebar-inset, 0px))" }}
        >
          <Button variant="ghost" size="sm" onClick={() => setShowInstancePicker(false)}>
            <ArrowLeft className="h-4 w-4 mr-1" />
            {t('common:back')}
          </Button>
          <h2 className="text-lg font-semibold">{t('instances:selectBackend')}</h2>
        </header>

        {/* Instance List */}
        <div className="flex-1 overflow-auto px-4 py-4">
          <div className="max-w-lg mx-auto space-y-3">
            {cachedInstances.map((inst) => {
              const isCurrent = inst.id === localStorage.getItem(CURRENT_INSTANCE_KEY)
              const hasApiKey = !!(inst.encrypted_key || inst.api_key)
              return (
                <button
                  key={inst.id}
                  onClick={() => handleInstanceSwitch(inst)}
                  className={`w-full flex items-center gap-4 p-4 rounded-xl border transition-colors text-left ${
                    isCurrent ? 'border-primary' : 'border-border hover:border-primary'
                  }`}
                >
                  <div className={`flex-shrink-0 w-10 h-10 rounded-full flex items-center justify-center ${isCurrent ? 'bg-primary-light' : 'bg-muted'}`}>
                    {inst.is_local ? <Globe className="h-5 w-5 text-primary" /> : <Server className={`h-5 w-5 ${isCurrent ? 'text-primary' : 'text-accent-cyan'}`} />}
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="font-medium truncate flex items-center gap-2">
                      {instanceStatuses[inst.id] && (
                        <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                          instanceStatuses[inst.id] === 'online' ? 'bg-success'
                          : instanceStatuses[inst.id] === 'offline' ? 'bg-destructive'
                          : 'bg-muted-foreground'
                        }`} />
                      )}
                      {inst.is_local ? t('instances:localBackend') : inst.name}
                      {isCurrent && (
                        <span className={`inline-flex items-center gap-0.5 ${textNano} font-medium px-1.5 py-0.5 rounded-full bg-primary-light text-primary`}>
                          <Check className="h-3 w-3" />
                          {t('instances:current')}
                        </span>
                      )}
                    </div>
                    <div className="text-xs text-muted-foreground truncate flex items-center gap-2 mt-0.5">
                      <span>{inst.is_local ? 'localhost:9375' : inst.url.replace(/^https?:\/\//, '')}</span>
                      <span className={`inline-flex items-center gap-1 ${textNano} px-1.5 py-0.5 rounded bg-muted`}>
                        <KeyRound className="h-3 w-3" />
                        {inst.is_local ? t('instances:authUserLogin') : hasApiKey ? t('instances:authApiKey') : t('instances:authUserLogin')}
                      </span>
                    </div>
                  </div>
                  <ChevronRight className="h-4 w-4 text-muted-foreground flex-shrink-0" />
                </button>
              )
            })}
            {cachedInstances.length === 0 && (
              <p className="text-center text-muted-foreground py-12">{t('instances:noInstances')}</p>
            )}
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="flex flex-col bg-background relative overflow-hidden viewport-full">
      {/* Background Effects */}
      <div className="fixed inset-0">
        {/* Base gradient */}
        <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted" />
        {/* Honeycomb mesh — shared component. Pointy-top tight tiling,
            brand-orange, ripple-from-center breathe. Mask fades edges. */}
        <HoneycombBackground />
        {/* One soft brand glow — restrained, just enough warmth to avoid
            feeling flat. Brand orange ties to the logo. */}
        <div
          className="absolute top-[45%] left-1/2 -translate-x-1/2 w-[42rem] h-[42rem] rounded-full blur-3xl"
          style={{ background: 'color-mix(in oklch, var(--accent-orange) 8%, transparent)' }}
        />
      </div>

      {/* Top Header — doubles as the Tauri window drag region (the shell's
          TopBar has the same contract; overlay titlebar means nothing native
          is draggable here). Interactive elements are skipped by the helper. */}
      <header className="absolute top-0 left-0 right-0 z-50 safe-top">
        <div
          className="flex items-center justify-between px-4 sm:px-6 h-14 sm:h-16"
          onMouseDown={handleWindowDragMouseDown}
        >
          <div className="flex items-center gap-2 sm:gap-3">
            <BrandLogoHorizontal className="h-6 sm:h-7" />
          </div>
          <div className="flex items-center gap-1">
            {/* Backend switcher — always opens instance picker */}
            {cachedInstances.length > 0 ? (() => {
              const currentId = localStorage.getItem(CURRENT_INSTANCE_KEY)
              const current = cachedInstances.find(i => i.id === currentId)
              const label = current
                ? (current.is_local ? t('instances:localBackend') : current.name)
                : t('instances:localBackend')
              return (
                <Button variant="ghost" size="sm" className="gap-1 px-2 sm:px-3" onClick={() => setShowInstancePicker(true)}>
                  <Server className="h-4 w-4" />
                  <span className="hidden sm:inline">{label}</span>
                </Button>
              )
            })() : null}

            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="ghost" size="sm" className="gap-1 px-2 sm:px-3">
                  <Languages className="h-4 w-4" />
                  <span>{languages.find(l => l.code === i18n.language)?.name || 'Language'}</span>
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="min-w-[130px]">
                {languages.map((lang) => (
                  <DropdownMenuItem
                    key={lang.code}
                    onClick={() => i18n.changeLanguage(lang.code)}
                    className={i18n.language === lang.code ? 'bg-primary-light text-primary font-medium' : ''}
                  >
                    {lang.name}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
      </header>

      {/* Main Content */}
      <main
        className="flex-1 px-4 sm:px-6 safe-bottom flex items-center justify-center min-h-0 overflow-y-auto"
        onClick={(e) => {
          if ((e.target as HTMLElement).closest('form, button, a')) return
          handleBackdropClick()
        }}
      >
        <div className="w-full max-w-md">
          <div
            className="backdrop-blur-xl rounded-2xl p-6 sm:p-8 border shadow-md animate-fade-in-up"
            style={{
              backgroundColor: 'color-mix(in oklch, var(--background) 72%, transparent)',
              borderColor: 'color-mix(in oklch, var(--border) 55%, transparent)',
            }}
          >
            <h2 className="text-2xl sm:text-3xl font-semibold mb-4 sm:mb-6 text-center">{t('auth:login')}</h2>
            <form onSubmit={handleSubmit} className="flex flex-col gap-4 sm:gap-5">
              <div className="relative">
                <User className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
                <Input
                  id="username"
                  type="text"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  placeholder={t('auth:username')}
                  autoComplete="username"
                  required
                  className="pl-9 h-11 bg-bg-70 border-border focus:bg-background dark:focus:bg-bg-50 focus:border-primary transition-colors text-base scroll-mb-32"
                />
              </div>
              <div className="relative">
                <Lock className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
                <Input
                  id="password"
                  type={showPassword ? "text" : "password"}
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={t('auth:password')}
                  autoComplete="current-password"
                  required
                  className="pl-9 pr-10 h-11 bg-bg-70 border-border focus:bg-background dark:focus:bg-bg-50 focus:border-primary transition-colors text-base scroll-mb-32"
                />
                <button
                  type="button"
                  onClick={() => setShowPassword((s) => !s)}
                  className="absolute right-2 top-1/2 -translate-y-1/2 h-7 w-7 flex items-center justify-center rounded-md text-muted-foreground hover:text-foreground hover:bg-muted transition-colors"
                  aria-label={showPassword ? t('auth:hidePassword') : t('auth:showPassword')}
                  title={showPassword ? t('auth:hidePassword') : t('auth:showPassword')}
                  tabIndex={-1}
                >
                  {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                </button>
              </div>
              <label className="flex items-center gap-2 cursor-pointer group">
                <Checkbox
                  id="remember"
                  checked={rememberMe}
                  onCheckedChange={(checked) => setRememberMe(!!checked)}
                />
                <span className="text-sm text-muted-foreground group-hover:text-foreground transition-colors leading-none">
                  {t('auth:rememberMe')}
                </span>
              </label>
              {error && (
                <div className="flex items-start gap-2 text-sm text-error bg-muted rounded-md p-3">
                  <Shield className="h-4 w-4 mt-0.5 flex-shrink-0" />
                  <span>{error}</span>
                </div>
              )}
              <Button
                type="submit"
                disabled={isLoading || !username || !password}
                className="h-11 w-full"
                size="default"
              >
                {isLoading ? t('auth:loggingIn') : t('auth:login')}
              </Button>
            </form>
          </div>
        </div>
        <footer className="hidden sm:block absolute left-0 right-0 z-10 text-center bottom-6">
          <p className="text-xs text-muted-foreground">
            © CamThink {new Date().getFullYear()}
          </p>
        </footer>
      </main>
    </div>
  )
}
