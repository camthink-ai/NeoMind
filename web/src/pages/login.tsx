import { useState, useEffect } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { Bot, Languages, Lock, User, Shield } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { BrandName, BrandLogoHorizontal } from "@/components/shared/BrandName"

const languages = [
  { code: 'en', name: 'English' },
  { code: 'zh', name: '简体中文' },
]

// LocalStorage keys for remembering credentials
const CREDENTIALS_KEY = 'neomind_remembered_credentials'

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
  const { t, i18n } = useTranslation(['common', 'auth'])
  const { login } = useStore()
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const [rememberMe, setRememberMe] = useState(false)
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState("")

  // Load saved credentials on mount
  useEffect(() => {
    try {
      const saved = localStorage.getItem(CREDENTIALS_KEY)
      if (saved) {
        const credentials = JSON.parse(saved)
        if (credentials.username) {
          setUsername(credentials.username)
        }
        if (credentials.password) {
          setPassword(credentials.password)
        }
        if (credentials.rememberMe !== undefined) {
          setRememberMe(credentials.rememberMe)
        }
      }
    } catch {
      // Ignore parsing errors
    }
  }, [])

  // Save credentials when rememberMe changes
  useEffect(() => {
    if (rememberMe && username) {
      try {
        localStorage.setItem(CREDENTIALS_KEY, JSON.stringify({
          username,
          password: '', // Only save password when logging in successfully
          rememberMe: true,
        }))
      } catch {
        // Ignore localStorage errors
      }
    } else if (!rememberMe) {
      // Clear saved credentials when unchecked
      try {
        localStorage.removeItem(CREDENTIALS_KEY)
      } catch {
        // Ignore localStorage errors
      }
    }
  }, [rememberMe, username])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError("")
    setIsLoading(true)

    try {
      await login(username, password, rememberMe)

      // Save credentials if remember me is checked
      if (rememberMe) {
        try {
          localStorage.setItem(CREDENTIALS_KEY, JSON.stringify({
            username,
            password,
            rememberMe: true,
          }))
        } catch {
          // Ignore localStorage errors
        }
      } else {
        // Clear saved credentials
        try {
          localStorage.removeItem(CREDENTIALS_KEY)
        } catch {
          // Ignore localStorage errors
        }
      }
    } catch (err) {
      setError(translateError(err instanceof Error ? err.message : String(t('auth:loginFailed')), t))
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <div className="min-h-screen flex flex-col bg-background overflow-hidden">
      {/* Background Effects - AI Network Theme */}
      <div className="fixed inset-0">
        {/* Base gradient */}
        <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted/10" />

        {/* Dot grid pattern - representing distributed nodes/edge devices */}
        <div className="absolute inset-0" style={{
          backgroundImage: 'radial-gradient(circle, #80808015 1px, transparent 1px)',
          backgroundSize: '32px 32px'
        }} />

        {/* Network connection lines - subtle tech feel */}
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

        {/* Central AI glow - representing intelligence hub */}
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] bg-primary/5 dark:bg-primary/10 rounded-full blur-3xl animate-pulse" style={{ animationDuration: '8s' }} />

        {/* Edge node glows - representing distributed edge computing */}
        <div className="absolute top-[15%] left-[10%] w-32 h-32 bg-blue-500/5 dark:bg-blue-500/10 rounded-full blur-2xl animate-pulse" style={{ animationDuration: '6s', animationDelay: '0s' }} />
        <div className="absolute bottom-[20%] right-[15%] w-40 h-40 bg-purple-500/5 dark:bg-purple-500/10 rounded-full blur-2xl animate-pulse" style={{ animationDuration: '7s', animationDelay: '1s' }} />
        <div className="absolute top-[30%] right-[20%] w-24 h-24 bg-cyan-500/5 dark:bg-cyan-500/10 rounded-full blur-2xl animate-pulse" style={{ animationDuration: '5s', animationDelay: '2s' }} />
        <div className="absolute bottom-[30%] left-[20%] w-28 h-28 bg-indigo-500/5 dark:bg-indigo-500/10 rounded-full blur-2xl animate-pulse" style={{ animationDuration: '6s', animationDelay: '3s' }} />

        {/* Diagonal accent lines - tech aesthetic */}
        <div className="absolute inset-0 opacity-[0.02]">
          <div className="absolute top-0 left-1/4 w-px h-full bg-gradient-to-b from-transparent via-primary to-transparent" />
          <div className="absolute top-0 right-1/4 w-px h-full bg-gradient-to-b from-transparent via-blue-500 to-transparent" />
          <div className="absolute top-1/4 left-0 w-full h-px bg-gradient-to-r from-transparent via-purple-500 to-transparent" />
          <div className="absolute bottom-1/4 left-0 w-full h-px bg-gradient-to-r from-transparent via-cyan-500 to-transparent" />
        </div>

        {/* Corner accents */}
        <div className="absolute top-8 left-8 w-16 h-16 border-l border-t border-primary/10 rounded-tl-lg" />
        <div className="absolute top-8 right-8 w-16 h-16 border-r border-t border-blue-500/10 rounded-tr-lg" />
        <div className="absolute bottom-8 left-8 w-16 h-16 border-l border-b border-purple-500/10 rounded-bl-lg" />
        <div className="absolute bottom-8 right-8 w-16 h-16 border-r border-b border-cyan-500/10 rounded-br-lg" />
      </div>

      {/* Top Header */}
      <header className="relative z-10 backdrop-blur-sm">
        <div className="flex items-center justify-between px-6 h-16">
          {/* Left - Logo & Name */}
          <div className="flex items-center gap-3">
            <BrandLogoHorizontal className="h-7" />
          </div>

          {/* Right - Language Switcher */}
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

      {/* Main Content - Centered Login Form */}
      <main className="relative z-10 flex-1 flex items-center justify-center px-6 py-12">
        <div className="w-full max-w-md">
          {/* Login Card */}
          <div className="bg-background/50 dark:bg-background/30 backdrop-blur-md rounded-xl p-8">
            {/* Login Title */}
            <h2 className="text-3xl font-semibold mb-8 text-center">{t('auth:login')}</h2>

            {/* Login Form */}
            <form onSubmit={handleSubmit} className="flex flex-col gap-5">
              {/* Username Field */}
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
                  className="pl-9 h-11 bg-background/70 dark:bg-background/30 border-border/50 dark:border-border/30 focus:bg-background dark:focus:bg-background/50 focus:border-primary/50 transition-colors"
                />
              </div>

              {/* Password Field */}
              <div className="relative">
                <Lock className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
                <Input
                  id="password"
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={t('auth:password')}
                  autoComplete="current-password"
                  required
                  className="pl-9 h-11 bg-background/70 dark:bg-background/30 border-border/50 dark:border-border/30 focus:bg-background dark:focus:bg-background/50 focus:border-primary/50 transition-colors"
                />
              </div>

              {/* Remember Me */}
              <label className="flex items-center gap-2 cursor-pointer group">
                <div className="relative flex items-center">
                  <input
                    id="remember"
                    type="checkbox"
                    checked={rememberMe}
                    onChange={(e) => setRememberMe(e.target.checked)}
                    className="peer appearance-none h-4 w-4 rounded border border-border bg-background/70 dark:bg-background/30 transition-all cursor-pointer checked:bg-primary checked:border-primary dark:checked:bg-foreground/90 dark:checked:border-foreground/90"
                  />
                  <svg className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 w-3 h-3 text-white dark:text-background pointer-events-none opacity-0 peer-checked:opacity-100 transition-opacity" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth="3">
                    <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                  </svg>
                </div>
                <span className="text-sm text-muted-foreground group-hover:text-foreground transition-colors leading-none">
                  {t('auth:rememberMe')}
                </span>
              </label>

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
                disabled={isLoading || !username || !password}
                className="h-11 w-full"
                size="default"
              >
                {isLoading ? t('auth:loggingIn') : t('auth:login')}
              </Button>
            </form>

            {/* Footer */}
            <div className="text-center mt-6 pt-6">
              <p className="text-xs text-muted-foreground/70 dark:text-muted-foreground/50">
                <BrandName /> Edge AI Agent v1.0
              </p>
            </div>
          </div>
        </div>
      </main>
    </div>
  )
}
