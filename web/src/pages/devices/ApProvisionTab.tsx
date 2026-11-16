/**
 * SoftAP Device Provisioning — first viewport is calm hand-join guidance.
 * After HTTP CT-Prov session open, shared wizard: info → caps → edit → apply.
 */

import { useState, useCallback, useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, Loader2, Wifi } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useCtProvision } from '@/hooks/useCtProvision'
import {
  listApProvisionModels,
  parseApSsid,
  softapHostForModel,
} from '@/lib/ct-prov'
import { useStore } from '@/store'
import { cn } from '@/lib/utils'
import { CtProvisionCenter, CtProvisionWizard } from './CtProvisionWizard'

interface ApProvisionTabProps {
  onComplete?: (deviceId: string) => void
}

type Phase = 'join' | 'connecting' | 'wizard' | 'failed'

export function ApProvisionTab({ onComplete }: ApProvisionTabProps) {
  const { t } = useTranslation(['common', 'devices'])
  const ct = useCtProvision('ap')
  const deviceTypes = useStore((s) => s.deviceTypes)
  const apModels = useMemo(() => listApProvisionModels(deviceTypes), [deviceTypes])

  const [phase, setPhase] = useState<Phase>('join')
  const [modelPick, setModelPick] = useState<string>('NE101')
  const [advancedOpen, setAdvancedOpen] = useState(false)

  useEffect(() => {
    if (apModels.length && !apModels.some((m) => m.model === modelPick)) {
      setModelPick(apModels[0]!.model)
      ct.setApHost(apModels[0]!.host)
    }
  }, [apModels]) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const parsed = parseApSsid(ct.apSsidInput.trim())
    if (parsed?.modelHint) {
      const hint = parsed.modelHint
      const family =
        apModels.find((m) => hint === m.model || hint.startsWith(m.model))?.model ?? hint
      setModelPick(family)
      ct.setApHost(softapHostForModel(family, deviceTypes))
      ct.setApModelHint(hint)
    }
  }, [ct.apSsidInput, apModels, deviceTypes]) // eslint-disable-line react-hooks/exhaustive-deps

  const handleConnect = useCallback(async () => {
    setPhase('connecting')
    const host = softapHostForModel(modelPick, deviceTypes)
    ct.setApHost(host)
    const ok = await ct.connectAp({
      ssid: ct.apSsidInput || undefined,
      model: modelPick,
      host,
    })
    setPhase(ok ? 'wizard' : 'failed')
  }, [ct, modelPick, deviceTypes])

  const reset = useCallback(() => {
    ct.disconnect()
    setPhase('join')
  }, [ct])

  if (phase === 'wizard' && ct.deviceInfo) {
    return (
      <CtProvisionWizard
        channel="ap"
        ct={ct}
        onComplete={onComplete}
        onReset={reset}
        failureHint={t('devices:ap.unreachableHint')}
      />
    )
  }

  if (phase === 'failed') {
    return (
      <CtProvisionCenter>
        <div className="w-16 h-16 rounded-xl bg-error-light flex items-center justify-center">
          <Wifi className="w-8 h-8 text-error" />
        </div>
        <div className="text-center max-w-sm">
          <h3 className="text-lg font-medium">{t('devices:ble.failedTitle')}</h3>
          <p className="text-sm text-muted-foreground mt-1">{ct.error}</p>
          <p className="text-xs text-muted-foreground mt-2">{t('devices:ap.unreachableHint')}</p>
        </div>
        <Button onClick={reset}>{t('common:retry')}</Button>
      </CtProvisionCenter>
    )
  }

  if (phase === 'connecting' || ct.connecting) {
    return (
      <CtProvisionCenter>
        <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
          <Loader2 className="w-8 h-8 text-primary animate-spin" />
        </div>
        <div className="text-center space-y-2">
          <h3 className="text-lg font-medium">{t('devices:ap.connectingTitle')}</h3>
          <p className="text-sm text-muted-foreground">{ct.apHost}</p>
        </div>
      </CtProvisionCenter>
    )
  }

  // —— First viewport: hand-join only ——
  return (
    <CtProvisionCenter>
      <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
        <Wifi className="w-8 h-8 text-primary" />
      </div>

      <div className="text-center max-w-sm space-y-2">
        <p className="text-xs font-medium text-muted-foreground tracking-wide">
          {t('devices:ctProv.productName')}
        </p>
        <h3 className="text-lg font-medium">{t('devices:ap.joinTitle')}</h3>
        <p className="text-sm text-muted-foreground">{t('devices:ap.joinDesc')}</p>
      </div>

      <div className="max-w-sm w-full rounded-xl border bg-card p-4 text-center space-y-2">
        <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
          {t('devices:ap.ssidPatternLabel')}
        </p>
        <p className="text-sm font-medium tracking-wide font-mono break-all">
          {t('devices:ap.ssidPatternPrimary')}
        </p>
        <p className="text-xs text-muted-foreground">{t('devices:ap.ssidPatternHint')}</p>
      </div>

      <Button onClick={handleConnect} size="lg" className="min-w-[220px]">
        <Wifi className="w-4 h-4 mr-2" />
        {t('devices:ap.joinedContinue')}
      </Button>

      <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen} className="max-w-sm w-full">
        <CollapsibleTrigger className="w-full flex items-center justify-center gap-1 text-xs text-muted-foreground hover:text-foreground">
          <ChevronDown
            className={cn('w-3.5 h-3.5 transition-transform', advancedOpen && 'rotate-180')}
          />
          {t('devices:ap.advancedOptions')}
        </CollapsibleTrigger>
        <CollapsibleContent className="pt-3 space-y-3">
          <div className="rounded-xl border bg-card p-4 space-y-3">
            <div>
              <Label>{t('devices:ap.ssidOptional')}</Label>
              <Input
                value={ct.apSsidInput}
                onChange={(e) => ct.setApSsidInput(e.target.value)}
                placeholder={t('devices:ap.ssidPlaceholder')}
              />
            </div>
            <div>
              <Label>{t('devices:ap.model')}</Label>
              <Select
                value={modelPick}
                onValueChange={(v) => {
                  setModelPick(v)
                  ct.setApHost(softapHostForModel(v, deviceTypes))
                  ct.setApModelHint(v)
                }}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {apModels.map((m) => (
                    <SelectItem key={m.model} value={m.model}>
                      {m.model} ({m.host})
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div>
              <Label>{t('devices:ap.host')}</Label>
              <Input
                value={ct.apHost}
                onChange={(e) => ct.setApHost(e.target.value)}
                placeholder="192.168.1.1"
              />
            </div>
          </div>
        </CollapsibleContent>
      </Collapsible>

      {ct.error && <p className="text-sm text-error text-center max-w-sm">{ct.error}</p>}
    </CtProvisionCenter>
  )
}
