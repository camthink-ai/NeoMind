import { useState } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { Bot, Sparkles, Shield } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Field,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { cn } from "@/lib/utils"

// Error translation helper - uses i18n for error messages
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
  // Return default error message
  return error || t("loginFailed")
}

export function LoginPage() {
  const { t } = useTranslation(['common', 'auth'])
  const { login } = useStore()
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const [rememberMe, setRememberMe] = useState(false)
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState("")

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError("")
    setIsLoading(true)

    try {
      await login(username, password, rememberMe)
    } catch (err) {
      setError(translateError(err instanceof Error ? err.message : "登录失败", t))
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center relative overflow-hidden p-4">
      {/* Animated gradient background */}
      <div className="absolute inset-0 bg-gradient-to-br from-gray-50 via-white to-gray-100 dark:from-gray-950 dark:via-black dark:to-gray-900" />
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_top,_var(--tw-gradient-stops))] from-gray-200/40 via-transparent to-transparent dark:from-gray-800/20" />

      {/* Animated floating orbs */}
      <div className="absolute top-1/4 -left-20 w-72 h-72 bg-gradient-to-br from-gray-300/30 to-transparent dark:from-gray-700/20 rounded-full blur-3xl animate-pulse-slow" />
      <div className="absolute bottom-1/4 -right-20 w-96 h-96 bg-gradient-to-tl from-gray-400/20 to-transparent dark:from-gray-600/10 rounded-full blur-3xl animate-pulse-slow delay-1000" />
      <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[600px] h-[600px] bg-gradient-to-r from-gray-200/10 to-transparent dark:from-gray-800/5 rounded-full blur-3xl" />

      <div className="w-full max-w-md relative z-10">
        {/* Login Card */}
        <div className={cn(
          "flex flex-col gap-6 rounded-3xl border border-gray-200/60 dark:border-gray-800/60",
          "bg-white/70 dark:bg-black/60 backdrop-blur-xl p-8 shadow-2xl shadow-gray-200/50 dark:shadow-black/50",
          "animate-fade-in",
        )}>
          {/* Header */}
          <div className="flex flex-col items-center gap-3 text-center">
            <div className={cn(
              "flex size-14 items-center justify-center rounded-2xl",
              "bg-gradient-to-br from-gray-900 to-gray-700 dark:from-white dark:to-gray-300",
              "text-white dark:text-gray-900 shadow-lg shadow-gray-900/20 dark:shadow-white/10",
              "animate-scale-in",
            )}>
              <Bot className="size-7" />
            </div>
            <div className="flex items-center gap-2">
              <h1 className="text-3xl font-bold bg-gradient-to-r from-gray-900 to-gray-600 dark:from-white dark:to-gray-400 bg-clip-text text-transparent">
                NeoTalk
              </h1>
              <Sparkles className="size-5 text-gray-400 dark:text-gray-600 animate-sparkle" />
            </div>
            <p className="text-gray-500 dark:text-gray-400 font-medium">
              {t('platformTagline')}
            </p>
          </div>

          {/* Security Badge */}
          <div className="flex items-center justify-center gap-2 px-4 py-2 rounded-full bg-gray-100/50 dark:bg-gray-900/50 border border-gray-200 dark:border-gray-800">
            <Shield className="size-4 text-gray-500 dark:text-gray-400" />
            <span className="text-xs text-gray-500 dark:text-gray-400">{t('secureLogin')}</span>
          </div>

          {/* Login Form */}
          <form onSubmit={handleSubmit} className="flex flex-col gap-5">
            <FieldGroup>
              <Field className="gap-2">
                <FieldLabel htmlFor="username" className="text-gray-700 dark:text-gray-300">
                  {t('username')}
                </FieldLabel>
                <Input
                  id="username"
                  type="text"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  placeholder={t('usernamePlaceholder')}
                  autoComplete="username"
                  required
                  className={cn(
                    "h-11 bg-white/80 dark:bg-gray-900/60",
                    "border-gray-300 dark:border-gray-700",
                    "text-gray-900 dark:text-gray-100 placeholder:text-gray-400 dark:placeholder:text-gray-500",
                    "transition-all duration-200",
                    "focus:border-gray-500 dark:focus:border-gray-500 focus:ring-2 focus:ring-gray-500/20",
                    "hover:border-gray-400 dark:hover:border-gray-600",
                  )}
                />
              </Field>

              <Field className="gap-2">
                <FieldLabel htmlFor="password" className="text-gray-700 dark:text-gray-300">
                  {t('password')}
                </FieldLabel>
                <Input
                  id="password"
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={t('passwordPlaceholder')}
                  autoComplete="current-password"
                  required
                  className={cn(
                    "h-11 bg-white/80 dark:bg-gray-900/60",
                    "border-gray-300 dark:border-gray-700",
                    "text-gray-900 dark:text-gray-100 placeholder:text-gray-400 dark:placeholder:text-gray-500",
                    "transition-all duration-200",
                    "focus:border-gray-500 dark:focus:border-gray-500 focus:ring-2 focus:ring-gray-500/20",
                    "hover:border-gray-400 dark:hover:border-gray-600",
                  )}
                />
              </Field>

              <label className="flex items-center gap-3 cursor-pointer group">
                <div className="relative">
                  <input
                    id="remember"
                    type="checkbox"
                    checked={rememberMe}
                    onChange={(e) => setRememberMe(e.target.checked)}
                    className="peer h-4 w-4 cursor-pointer appearance-none rounded border border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-900 checked:bg-gray-900 dark:checked:bg-white checked:border-gray-900 dark:checked:border-white transition-all"
                  />
                  <svg className="absolute left-0 top-0 w-4 h-4 pointer-events-none opacity-0 peer-checked:opacity-100 text-white dark:text-gray-900 transition-opacity" viewBox="0 0 16 16" fill="currentColor">
                    <path d="M12.207 4.793a1 1 0 010 1.414l-5 5a1 1 0 01-1.414 0l-2-2a1 1 0 011.414-1.414L6.5 9.086l4.293-4.293a1 1 0 011.414 0z"/>
                  </svg>
                </div>
                <span className="text-sm text-gray-600 dark:text-gray-400 select-none group-hover:text-gray-900 dark:group-hover:text-gray-300 transition-colors">
                  {t('rememberMe')}
                </span>
              </label>

              {/* Error Message */}
              {error && (
                <div className="flex items-center gap-2 p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800/50 text-red-600 dark:text-red-400 rounded-xl animate-shake">
                  <svg className="size-4 flex-shrink-0" viewBox="0 0 16 16" fill="currentColor">
                    <path fillRule="evenodd" d="M8 2a6 6 0 100 12A6 6 0 008 2zM7 5a1 1 0 112 0v3a1 1 0 11-2 0V5zm1 7a1 1 0 100-2 1 1 0 000 2z" clipRule="evenodd"/>
                  </svg>
                  <span className="text-sm font-medium">{error}</span>
                </div>
              )}

              <Button
                type="submit"
                disabled={isLoading || !username || !password}
                className={cn(
                  "w-full h-11 text-sm font-medium",
                  "bg-gradient-to-r from-gray-900 to-gray-700 dark:from-white dark:to-gray-200",
                  "text-white dark:text-gray-900",
                  "hover:from-gray-800 hover:to-gray-600 dark:hover:from-gray-100 dark:hover:to-gray-300",
                  "disabled:opacity-50 disabled:cursor-not-allowed",
                  "shadow-lg shadow-gray-900/20 dark:shadow-white/10",
                  "hover:shadow-xl hover:shadow-gray-900/30 dark:hover:shadow-white/20",
                  "transition-all duration-200",
                  "transform hover:scale-[1.02] active:scale-[0.98]",
                )}
              >
                {isLoading ? (
                  <span className="flex items-center justify-center gap-2">
                    <span className="h-4 w-4 border-2 border-white/30 dark:border-gray-700 border-t-white dark:border-transparent rounded-full animate-spin" />
                    {t('loggingIn')}
                  </span>
                ) : (
                  <span className="flex items-center justify-center gap-2">
                    {t('login')}
                    <svg className="size-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 7l5 5m0 0l-5 5m5-5H6" />
                    </svg>
                  </span>
                )}
              </Button>
            </FieldGroup>
          </form>
        </div>

        {/* Footer */}
        <div className="flex items-center justify-center gap-2 mt-6 text-gray-400 dark:text-gray-600">
          <div className="h-px w-8 bg-gray-300 dark:bg-gray-700" />
          <span className="text-xs">NeoTalk Edge AI Agent v1.0</span>
          <div className="h-px w-8 bg-gray-300 dark:bg-gray-700" />
        </div>
      </div>

      {/* Custom animations */}
      <style>{`
        @keyframes pulse-slow {
          0%, 100% { opacity: 0.5; transform: scale(1); }
          50% { opacity: 0.8; transform: scale(1.05); }
        }
        @keyframes scale-in {
          0% { opacity: 0; transform: scale(0.9); }
          100% { opacity: 1; transform: scale(1); }
        }
        @keyframes fade-in {
          0% { opacity: 0; transform: translateY(10px); }
          100% { opacity: 1; transform: translateY(0); }
        }
        @keyframes shake {
          0%, 100% { transform: translateX(0); }
          25% { transform: translateX(-4px); }
          75% { transform: translateX(4px); }
        }
        @keyframes sparkle {
          0%, 100% { transform: scale(1) rotate(0deg); opacity: 1; }
          50% { transform: scale(1.2) rotate(10deg); opacity: 0.8; }
        }
        .animate-pulse-slow { animation: pulse-slow 4s ease-in-out infinite; }
        .animate-scale-in { animation: scale-in 0.5s ease-out; }
        .animate-fade-in { animation: fade-in 0.6s ease-out; }
        .animate-shake { animation: shake 0.3s ease-in-out; }
        .animate-sparkle { animation: sparkle 2s ease-in-out infinite; }
        .delay-1000 { animation-delay: 1s; }
      `}</style>
    </div>
  )
}
