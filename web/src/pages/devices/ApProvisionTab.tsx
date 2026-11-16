/**
 * SoftAP Device Provisioning — first viewport is hand-join only.
 * NeoMind does NOT join Wi‑Fi on macOS/Tauri/Web; the operator must join the
 * device SoftAP in System Settings, then Continue (after ack + reachability probe).
 */

import { useState, useCallback, useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, Loader2, Wifi } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { useCtProvision } from '@/hooks/useCtProvision'
import {
  listApProvisionModels,
  parseApSsid,
  probeSoftapHost,
  softapHostForModel,
} from '@/lib/ct-prov'
import { useStore } from '@/store'
import { cn } from '@/lib/utils'
import { CtProvisionCenter, CtProvisionWizard } from './CtProvisionWizard'

interface ApProvisionTabProps {
  onComplete?: (deviceId: string) => void
}

type Phase = 'join' | 'probing' | 'connecting' | 'wizard' | 'failed'

function ssidExampleForFamily(model: string): string {
  return `${model}-…`
}

export function ApProvisionTab({ onComplete }: ApProvisionTabProps) {
  const { t } = useTranslation(['common', 'devices'])
  const ct = useCtProvision('ap')
  const deviceTypes = useStore((s) => s.deviceTypes)
  const apModels = useMemo(() => listApProvisionModels(deviceTypes), [deviceTypes])

  const [phase, setPhase] = useState<Phase>('join')
  const [modelPick, setModelPick] = useState<string>('NE101')
  const [joinedAck, setJoinedAck] = useState(false)
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [failKind, setFailKind] = useState<'not_on_softap' | 'http' | 'protocol' | 'other'>(
    'not_on_softap',
  )

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

  const selectedHost = useMemo(() => {
    const trimmed = ct.apHost.trim()
    if (trimmed) return trimmed
    return softapHostForModel(modelPick, deviceTypes)
  }, [ct.apHost, modelPick, deviceTypes])

  const handleModelChange = useCallback(
    (v: string) => {
      setModelPick(v)
      ct.setApHost(softapHostForModel(v, deviceTypes))
      ct.setApModelHint(v)
      setJoinedAck(false)
    },
    [ct, deviceTypes],
  )

  const handleConnect = useCallback(async () => {
    if (!joinedAck) return
    const host = selectedHost
    ct.setApHost(host)
    setPhase('probing')
    setFailKind('not_on_softap')
    const probe = await probeSoftapHost(host)
    if (!probe.ok) {
      setFailKind('not_on_softap')
      ct.clearError()
      setPhase('failed')
      return
    }
    setPhase('connecting')
    const result = await ct.connectAp({
      ssid: ct.apSsidInput || undefined,
      model: modelPick,
      host,
    })
    if (!result.ok) {
      setFailKind(result.kind)
      setPhase('failed')
      return
    }
    setPhase('wizard')
  }, [ct, joinedAck, modelPick, selectedHost])

  const reset = useCallback(() => {
    ct.disconnect()
    setJoinedAck(false)
    setPhase('join')
  }, [ct])

  if (phase === 'wizard' && ct.deviceInfo) {
    return (
      <CtProvisionWizard
        channel="ap"
        ct={ct}
        onComplete={onComplete}
        onReset={reset}
        failureHint={t('devices:ap.errorNotOnSoftap')}
      />
    )
  }

  if (phase === 'failed') {
    const hint =
      failKind === 'http'
        ? t('devices:ap.errorHttp')
        : failKind === 'protocol'
          ? t('devices:ap.errorProtocol')
          : t('devices:ap.errorNotOnSoftap')
    return (
      <CtProvisionCenter>
        <div className="w-16 h-16 rounded-xl bg-error-light flex items-center justify-center">
          <Wifi className="w-8 h-8 text-error" />
        </div>
        <div className="text-center max-w-sm space-y-2">
          <h3 className="text-lg font-medium">{t('devices:ble.failedTitle')}</h3>
          <p className="text-sm text-muted-foreground">{hint}</p>
          {ct.error && (
            <p className="text-xs text-muted-foreground font-mono break-all">{ct.error}</p>
          )}
          <p className="text-xs text-muted-foreground">
            {t('devices:ap.errorHostHint', {
              host: selectedHost,
              ssid: ssidExampleForFamily(modelPick),
            })}
          </p>
        </div>
        <Button onClick={reset}>{t('common:retry')}</Button>
      </CtProvisionCenter>
    )
  }

  if (phase === 'probing' || phase === 'connecting' || ct.connecting) {
    return (
      <CtProvisionCenter>
        <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
          <Loader2 className="w-8 h-8 text-primary animate-spin" />
        </div>
        <div className="text-center space-y-2">
          <h3 className="text-lg font-medium">
            {phase === 'probing'
              ? t('devices:ap.probingTitle')
              : t('devices:ap.connectingTitle')}
          </h3>
          <p className="text-sm text-muted-foreground font-mono">{selectedHost}</p>
          <p className="text-xs text-muted-foreground max-w-sm">
            {t('devices:ap.probingHint')}
          </p>
        </div>
      </CtProvisionCenter>
    )
  }

  // —— First viewport: unmistakable hand-join ——
  return (
    <div className="min-h-[min(60vh,520px)] h-full flex flex-col px-4 py-6">
      <div className="text-center max-w-md mx-auto mb-5 space-y-2">
        <div className="w-14 h-14 mx-auto rounded-xl bg-primary-light flex items-center justify-center">
          <Wifi className="w-7 h-7 text-primary" />
        </div>
        <h3 className="text-lg font-medium">{t('devices:ap.joinTitle')}</h3>
        <p className="text-sm text-muted-foreground">{t('devices:ap.joinDescStrong')}</p>
      </div>

      <div className="max-w-md mx-auto w-full space-y-4">
        <div className="rounded-xl border bg-card p-4 space-y-3">
          <div>
            <Label>{t('devices:ap.model')}</Label>
            <Select value={modelPick} onValueChange={handleModelChange}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {apModels.map((m) => (
                  <SelectItem key={m.model} value={m.model}>
                    {m.model} · {m.host}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <ol className="space-y-2.5 text-sm list-decimal list-inside text-foreground">
            <li>{t('devices:ap.handStep1')}</li>
            <li>
              {t('devices:ap.handStep2', {
                ssid: ssidExampleForFamily(modelPick),
              })}
            </li>
            <li>{t('devices:ap.handStep3')}</li>
          </ol>

          <div className="rounded-lg bg-muted-30 px-3 py-2.5 text-xs space-y-1">
            <p>
              <span className="text-muted-foreground">{t('devices:ap.ssidPatternLabel')}: </span>
              <span className="font-mono font-medium">{ssidExampleForFamily(modelPick)}</span>
              <span className="text-muted-foreground"> · {t('devices:ap.ssidPatternHint')}</span>
            </p>
            <p>
              <span className="text-muted-foreground">{t('devices:ap.host')}: </span>
              <span className="font-mono font-medium">{selectedHost}</span>
            </p>
            <p className="text-muted-foreground">{t('devices:ap.noProgrammaticJoin')}</p>
          </div>
        </div>

        <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
          <div className="rounded-xl border bg-card">
            <CollapsibleTrigger className="w-full flex items-center justify-between p-3.5 text-left">
              <span className="text-sm font-medium">{t('devices:ap.advancedOptions')}</span>
              <ChevronDown
                className={cn(
                  'w-4 h-4 text-muted-foreground transition-transform',
                  advancedOpen && 'rotate-180',
                )}
              />
            </CollapsibleTrigger>
            <CollapsibleContent>
              <div className="px-3.5 pb-3.5 space-y-3 border-t pt-3">
                <div>
                  <Label>{t('devices:ap.ssidOptional')}</Label>
                  <Input
                    value={ct.apSsidInput}
                    onChange={(e) => ct.setApSsidInput(e.target.value)}
                    placeholder={t('devices:ap.ssidPlaceholder')}
                  />
                </div>
                <div>
                  <Label>{t('devices:ap.host')}</Label>
                  <Input
                    value={ct.apHost}
                    onChange={(e) => ct.setApHost(e.target.value)}
                    placeholder={softapHostForModel(modelPick, deviceTypes)}
                  />
                </div>
              </div>
            </CollapsibleContent>
          </div>
        </Collapsible>

        <label className="flex items-start gap-3 rounded-xl border bg-card p-3.5 cursor-pointer">
          <Checkbox
            checked={joinedAck}
            onCheckedChange={(v) => setJoinedAck(v === true)}
            className="mt-0.5"
          />
          <span className="text-sm leading-snug">
            {t('devices:ap.joinedAck', {
              ssid: ssidExampleForFamily(modelPick),
            })}
          </span>
        </label>

        <Button
          className="w-full"
          size="lg"
          onClick={() => void handleConnect()}
          disabled={!joinedAck}
        >
          <Wifi className="w-4 h-4 mr-2" />
          {t('devices:ap.joinedContinue')}
        </Button>

        {ct.error && <p className="text-sm text-error text-center">{ct.error}</p>}
      </div>
    </div>
  )
}
