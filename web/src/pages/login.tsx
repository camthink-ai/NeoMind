import { useState } from "react"
import { useTranslation } from "react-i18next"
import { useStore } from "@/store"
import { Bot, Languages } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Field,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"

const languages = [
  { code: 'en', name: 'English' },
  { code: 'zh', name: '简体中文' },
]

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

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError("")
    setIsLoading(true)

    try {
      await login(username, password, rememberMe)
    } catch (err) {
      setError(translateError(err instanceof Error ? err.message : String(t('auth:loginFailed')), t))
    } finally {
      setIsLoading(false)
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-background p-4">
      <div className="w-full max-w-sm">
        {/* Login Card */}
        <div className="rounded-xl border bg-card p-6 shadow-sm relative">
          {/* Language Switcher */}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" className="absolute top-4 right-4 h-8 w-8">
                <Languages className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
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

          {/* Header */}
          <div className="flex flex-col items-center gap-3 text-center mb-6">
            <div className="flex size-12 items-center justify-center rounded-xl bg-primary text-primary-foreground">
              <Bot className="size-6" />
            </div>
            <h1 className="text-2xl font-bold">NeoTalk</h1>
            <p className="text-sm text-muted-foreground">
              {t('platformTagline')}
            </p>
          </div>

          {/* Login Form */}
          <form onSubmit={handleSubmit} className="flex flex-col gap-4">
            <FieldGroup>
              <Field className="gap-2">
                <FieldLabel htmlFor="username">{t('username')}</FieldLabel>
                <Input
                  id="username"
                  type="text"
                  value={username}
                  onChange={(e) => setUsername(e.target.value)}
                  placeholder={t('usernamePlaceholder')}
                  autoComplete="username"
                  required
                  className="h-10"
                />
              </Field>

              <Field className="gap-2">
                <FieldLabel htmlFor="password">{t('password')}</FieldLabel>
                <Input
                  id="password"
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  placeholder={t('passwordPlaceholder')}
                  autoComplete="current-password"
                  required
                  className="h-10"
                />
              </Field>

              <label className="flex items-center gap-2 cursor-pointer">
                <input
                  id="remember"
                  type="checkbox"
                  checked={rememberMe}
                  onChange={(e) => setRememberMe(e.target.checked)}
                  className="h-4 w-4 rounded border-gray-300"
                />
                <span className="text-sm text-muted-foreground">
                  {t('rememberMe')}
                </span>
              </label>

              {/* Error Message */}
              {error && (
                <div className="text-sm text-error bg-error/10 rounded-md p-2">
                  {error}
                </div>
              )}

              <Button
                type="submit"
                disabled={isLoading || !username || !password}
                className="w-full"
              >
                {isLoading ? t('loggingIn') : t('login')}
              </Button>
            </FieldGroup>
          </form>
        </div>

        {/* Footer */}
        <div className="text-center mt-4">
          <p className="text-xs text-muted-foreground">
            NeoTalk Edge AI Agent v1.0
          </p>
        </div>
      </div>
    </div>
  )
}
