/**
 * CompleteStep - Setup completion screen. Success state with a single CTA —
 * setup is done, the model/device guidance lives inside the app.
 */
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, ChevronRight, Globe } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { SetupBackground } from './SetupBackground'
import { SetupHeader } from './SetupHeader'
import { getLocalizedTimezones } from '@/lib/time/format'

interface CompleteStepProps {
  username: string
  initialTimezone: string
  token: string
  getApiUrl: (path: string) => string
  onComplete: (dest?: string) => void
}

export function CompleteStep({ username, initialTimezone, token, getApiUrl, onComplete }: CompleteStepProps) {
  const { t } = useTranslation(['common', 'setup'])
  const [timezone, setTimezone] = useState(initialTimezone)
  const timezoneOptions = getLocalizedTimezones(t)

  // Save timezone silently when the user adjusts it on this screen.
  const handleTimezoneChange = async (newTz: string) => {
    setTimezone(newTz)
    try {
      await fetch(getApiUrl('/settings/timezone'), {
        method: 'PUT',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${token}`,
        },
        body: JSON.stringify({ timezone: newTz }),
      })
    } catch (e) {
      console.warn('Failed to save timezone:', e)
    }
  }

  return (
    <div className="viewport-full flex flex-col bg-background relative overflow-hidden">
      <SetupBackground />

      {/* Floating header — absolute, matches AccountStep so both setup steps
          share the same top-right language switcher without consuming flow. */}
      <SetupHeader />

      <main className="relative z-10 flex-1 min-h-0 overflow-y-auto safe-bottom">
        <div className="min-h-full flex items-center justify-center px-4 pt-20 pb-6 sm:px-6 sm:pt-24 sm:pb-10">
          <div className="w-full max-w-md text-center animate-fade-in-up">
            <div
              className="backdrop-blur-xl rounded-2xl p-6 sm:p-8 border shadow-md"
              style={{
                backgroundColor: 'color-mix(in oklch, var(--background) 72%, transparent)',
                borderColor: 'color-mix(in oklch, var(--border) 55%, transparent)',
              }}
            >
              {/* Success Icon */}
              <div className="flex justify-center mb-4 sm:mb-5">
                <div className="flex size-12 sm:size-14 items-center justify-center rounded-full bg-success-light text-success ring-1 ring-border">
                  <Check className="size-6 sm:size-7" />
                </div>
              </div>

              {/* Success Message */}
              <h2 className="text-xl sm:text-2xl font-semibold mb-2 tracking-tight">{t('setup:completeTitle')}</h2>
              <p className="text-muted-foreground mb-2 text-sm">{t('setup:completeMessage')}</p>

              {/* Account meta — username chip + inline timezone selector */}
              <div className="flex flex-wrap items-center justify-center gap-2 mb-6 sm:mb-7">
                <div className="inline-flex items-center gap-2 bg-muted-30 rounded-full px-3 py-1">
                  <span className="text-xs text-muted-foreground">{t('setup:accountCreated')}:</span>
                  <span className="text-sm font-mono font-medium">{username}</span>
                </div>
                <div className="inline-flex items-center gap-1 rounded-full bg-muted-30 pl-2.5 pr-1 py-0.5">
                  <Globe className="size-3.5 text-muted-foreground shrink-0" />
                  <Select value={timezone} onValueChange={handleTimezoneChange}>
                    <SelectTrigger className="h-7 border-0 bg-transparent hover:bg-muted px-1.5 py-0 text-xs gap-1 rounded-full shadow-none focus:ring-0 max-w-[10rem]">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent className="max-h-60">
                      {timezoneOptions.map((tz) => (
                        <SelectItem key={tz.id} value={tz.id}>{tz.name}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </div>

              {/* Single CTA — setup is done, just go in. Model/device guidance
                  lives in the app (chat empty state, LLM settings). */}
              <Button
                onClick={() => onComplete("/chat")}
                className="w-full h-11 sm:h-10 gap-2"
                size="default"
              >
                {t('setup:getStarted')}
                <ChevronRight className="h-4 w-4" />
              </Button>
            </div>
          </div>
        </div>
      </main>
    </div>
  )
}
