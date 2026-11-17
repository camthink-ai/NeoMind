/**
 * SoftAP Device Provisioning — first viewport is hand-join only.
 * NeoMind does NOT join Wi‑Fi on macOS/Tauri/Web; the operator must join the
 * device SoftAP in System Settings, then Continue (after ack + reachability probe).
 */

import { useState, useCallback, useEffect, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, Loader2, Wifi } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { Field, FieldLabel } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
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
import {
  CtProvisionCenter,
  CtProvisionHero,
  CtProvisionWizard,
} from './CtProvisionWizard'

interface ApProvisionTabProps {
  onComplete?: (deviceId: string) => void
  /** Parent dialog dismiss counter — tear down SoftAP session on close. */
  dismissSignal?: number
}

type Phase = 'join' | 'probing' | 'connecting' | 'wizard' | 'failed'

function ssidExampleForFamily(model: string): string {
  return `${model}-…`
}

export function ApProvisionTab({ onComplete, dismissSignal = 0 }: ApProvisionTabProps) {
  const { t } = useTranslation(['common', 'devices'])
  const ct = useCtProvision('ap')
  const deviceTypes = useStore((s) => s.deviceTypes)
  const apModels = useMemo(() => listApProvisionModels(deviceTypes), [deviceTypes])

  const [phase, setPhase] = useState<Phase>('join')
  const [modelPick, setModelPick] = useState<string>('NE101')
  const [joinedAck, setJoinedAck] = useState(false)
  const [howOpen, setHowOpen] = useState(false)
  const [advancedOpen, setAdvancedOpen] = useState(false)
  const [failKind, setFailKind] = useState<'not_on_softap' | 'http' | 'protocol' | 'other'>(
    'not_on_softap',
  )
  const disconnectRef = useRef(ct.disconnect)
  disconnectRef.current = ct.disconnect

  useEffect(() => {
    return () => {
      disconnectRef.current()
    }
  }, [])

  useEffect(() => {
    if (dismissSignal > 0) {
      disconnectRef.current()
      setJoinedAck(false)
      setPhase('join')
    }
  }, [dismissSignal])

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
        <CtProvisionHero tone="error">
          <Wifi className="h-8 w-8 text-error" />
        </CtProvisionHero>
        <div className="max-w-sm space-y-2 text-center">
          <h3 className="text-lg font-medium">{t('devices:ble.failedTitle')}</h3>
          <p className="text-sm text-muted-foreground">{hint}</p>
          {ct.error && (
            <p className="break-all font-mono text-xs text-muted-foreground">{ct.error}</p>
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
        <CtProvisionHero>
          <Loader2 className="h-8 w-8 animate-spin text-primary" />
        </CtProvisionHero>
        <div className="space-y-2 text-center">
          <h3 className="text-lg font-medium">
            {phase === 'probing'
              ? t('devices:ap.probingTitle')
              : t('devices:ap.connectingTitle')}
          </h3>
          <p className="font-mono text-sm text-muted-foreground">{selectedHost}</p>
        </div>
        <Button variant="outline" onClick={reset}>
          {t('common:cancel')}
        </Button>
      </CtProvisionCenter>
    )
  }

  // —— First viewport: unmistakable hand-join ——
  // Mirror BLE scan-list shell: Main is overflow-hidden for sticky wizard
  // actions, so this pane must own overflow-y-auto (min-h-0 flex chain).
  // Flat hierarchy (no stacked cards/shadows on ack) — progressive disclosure
  // for steps + advanced overrides.
  return (
    <div className="mx-auto flex h-full min-h-0 w-full max-w-md flex-1 flex-col self-center py-6">
      <div className="mb-5 shrink-0 space-y-2 text-center">
        <CtProvisionHero size="md">
          <Wifi className="h-7 w-7 text-primary" />
        </CtProvisionHero>
        <h3 className="text-lg font-medium">{t('devices:ap.joinTitle')}</h3>
        <p className="text-sm text-muted-foreground">{t('devices:ap.joinDescStrong')}</p>
      </div>

      <div className="min-h-0 w-full flex-1 space-y-5 overflow-y-auto pb-2">
        <Field>
          <FieldLabel>{t('devices:ap.model')}</FieldLabel>
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
        </Field>

        <p className="text-xs text-muted-foreground">
          <span className="font-mono font-medium text-foreground">
            {ssidExampleForFamily(modelPick)}
          </span>
          <span> · </span>
          <span className="font-mono">{selectedHost}</span>
        </p>

        <Collapsible open={howOpen} onOpenChange={setHowOpen}>
          <CollapsibleTrigger className="flex w-full items-center justify-between py-1 text-left">
            <span className="text-sm font-medium">{t('devices:ap.prepTitle')}</span>
            <ChevronDown
              className={cn(
                'h-4 w-4 text-muted-foreground transition-transform',
                howOpen && 'rotate-180',
              )}
            />
          </CollapsibleTrigger>
          <CollapsibleContent>
            <ol className="mt-2 list-inside list-decimal space-y-2 text-sm text-muted-foreground">
              <li>{t('devices:ap.handStep1')}</li>
              <li>
                {t('devices:ap.handStep2', {
                  ssid: ssidExampleForFamily(modelPick),
                })}
              </li>
              <li>{t('devices:ap.handStep3')}</li>
            </ol>
            <p className="mt-2 text-xs text-muted-foreground">
              {t('devices:ap.ssidPatternHint')}
            </p>
          </CollapsibleContent>
        </Collapsible>

        <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
          <CollapsibleTrigger className="flex w-full items-center justify-between py-1 text-left">
            <span className="text-sm font-medium">{t('devices:ap.advancedOptions')}</span>
            <ChevronDown
              className={cn(
                'h-4 w-4 text-muted-foreground transition-transform',
                advancedOpen && 'rotate-180',
              )}
            />
          </CollapsibleTrigger>
          <CollapsibleContent>
            <div className="mt-2 space-y-3">
              <Field>
                <FieldLabel>{t('devices:ap.ssidOptional')}</FieldLabel>
                <Input
                  value={ct.apSsidInput}
                  onChange={(e) => ct.setApSsidInput(e.target.value)}
                  placeholder={t('devices:ap.ssidPlaceholder')}
                />
              </Field>
              <Field>
                <FieldLabel>{t('devices:ap.host')}</FieldLabel>
                <Input
                  value={ct.apHost}
                  onChange={(e) => ct.setApHost(e.target.value)}
                  placeholder={softapHostForModel(modelPick, deviceTypes)}
                />
              </Field>
              <p className="text-xs text-muted-foreground">{t('devices:ap.noProgrammaticJoin')}</p>
            </div>
          </CollapsibleContent>
        </Collapsible>
      </div>

      <div className="mt-auto shrink-0 space-y-3 border-t border-border bg-background pt-3">
        {/* Flat ack — no card/shadow; sticky footer border is the only divider. */}
        <label className="flex cursor-pointer items-start gap-3 py-0.5">
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
          <Wifi className="mr-2 h-4 w-4" />
          {t('devices:ap.joinedContinue')}
        </Button>

        {ct.error && <p className="text-center text-sm text-error">{ct.error}</p>}
      </div>
    </div>
  )
}
