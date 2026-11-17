/**
 * Bluetooth Device Provisioning — first viewport is scan + pick only.
 * After GATT + CT-Prov session open, shared wizard: info → caps → edit → apply.
 */

import { useState, useCallback, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Bluetooth, Loader2, Signal } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { useCtProvision } from '@/hooks/useCtProvision'
import {
  formatProvUserMessage,
  isTauriEnv,
  modelNameFromCode,
  type CtScanDevice,
} from '@/lib/ct-prov'
import { useStore } from '@/store'
import {
  CtProvisionCenter,
  CtProvisionHero,
  CtProvisionWizard,
} from './CtProvisionWizard'

interface BleProvisionTabProps {
  onComplete?: (deviceId: string) => void
  /** Called when the parent dialog is dismissed so we can drop the GATT session. */
  dismissSignal?: number
}

type Phase = 'scan' | 'connecting' | 'wizard' | 'failed'

function bleErrorCopy(
  t: (key: string, opts?: Record<string, string>) => string,
  err: unknown,
): string {
  if (err == null || err === '') return t('devices:ble.failedTitle')
  const code = formatProvUserMessage(err)
  if (code === 'wifi_init_failed') return t('devices:ctProv.errorWifiInit')
  if (code === 'wifi_associate_failed') return t('devices:ctProv.errorWifiAssociate')
  if (code === 'cellular_apn_required') return t('devices:ctProv.errorCellularApn')
  if (code === 'webhook_url_required') return t('devices:ctProv.errorWebhookUrl')
  if (code === 'required_field_missing') return t('devices:ctProv.errorRequiredField')
  if (code === 'storage_failed') return t('devices:ctProv.errorStorageFailed')
  if (code === 'request_timeout') return t('devices:ctProv.errorTimeout')
  if (code === 'device_busy') return t('devices:ctProv.errorBusy')
  if (code === 'not_connected') return t('devices:ctProv.errorNotConnected')
  if (code === 'internal_error') return t('devices:ctProv.errorInternal')
  return code
}

export function BleProvisionTab({ onComplete, dismissSignal = 0 }: BleProvisionTabProps) {
  const { t } = useTranslation(['common', 'devices'])
  const ct = useCtProvision('ble')
  const deviceTypes = useStore((s) => s.deviceTypes)

  const [phase, setPhase] = useState<Phase>('scan')
  const disconnectRef = useRef(ct.disconnect)
  disconnectRef.current = ct.disconnect

  // Drop GATT / CT-Prov session on unmount (tab switch or dialog close).
  useEffect(() => {
    return () => {
      disconnectRef.current()
    }
  }, [])

  // Parent dialog X / Escape / backdrop — bump signal so we disconnect even if
  // the tab stays mounted briefly while the dialog hides.
  useEffect(() => {
    if (dismissSignal > 0) {
      disconnectRef.current()
      setPhase('scan')
    }
  }, [dismissSignal])

  const handleScan = useCallback(async () => {
    await ct.scan()
  }, [ct])

  const handleSelectDevice = useCallback(
    async (dev: CtScanDevice) => {
      setPhase('connecting')
      const ok = await ct.selectBleDevice(dev)
      setPhase(ok ? 'wizard' : 'failed')
    },
    [ct],
  )

  const reset = useCallback(() => {
    ct.disconnect()
    setPhase('scan')
  }, [ct])

  const bleSupported = isTauriEnv() || (typeof navigator !== 'undefined' && !!navigator.bluetooth)

  if (phase === 'failed') {
    return (
      <CtProvisionCenter>
        <CtProvisionHero tone="error">
          <Bluetooth className="h-7 w-7 text-error" />
        </CtProvisionHero>
        <div className="max-w-sm space-y-1 text-center">
          <h3 className="text-base font-medium">{t('devices:ble.failedTitle')}</h3>
          <p className="text-sm text-muted-foreground">{bleErrorCopy(t, ct.error)}</p>
        </div>
        <Button onClick={reset}>{t('common:retry')}</Button>
      </CtProvisionCenter>
    )
  }

  if (phase === 'wizard' && ct.deviceInfo) {
    return (
      <CtProvisionWizard
        channel="ble"
        ct={ct}
        onComplete={onComplete}
        onReset={reset}
      />
    )
  }

  if (phase === 'connecting' || ct.connecting) {
    return (
      <CtProvisionCenter>
        <CtProvisionHero>
          <Loader2 className="h-7 w-7 animate-spin text-primary" />
        </CtProvisionHero>
        <div className="space-y-1 text-center">
          <h3 className="text-base font-medium">{t('devices:ble.connectingTitle')}</h3>
          <p className="truncate text-sm text-muted-foreground">
            {ct.selectedScan?.name || ct.selectedScan?.id}
          </p>
        </div>
        <Button variant="outline" onClick={reset}>
          {t('common:cancel')}
        </Button>
      </CtProvisionCenter>
    )
  }

  // Empty scan-in-progress (no hits yet) — full-pane spinner.
  if (ct.scanning && ct.scannedDevices.length === 0) {
    return (
      <CtProvisionCenter>
        <CtProvisionHero>
          <Loader2 className="h-7 w-7 animate-spin text-primary" />
        </CtProvisionHero>
        <div className="space-y-1 text-center">
          <h3 className="text-base font-medium">{t('devices:ble.scanning')}</h3>
          <p className="text-xs text-muted-foreground">{t('devices:ble.scanningHint')}</p>
        </div>
        <Button variant="outline" onClick={reset}>
          {t('common:cancel')}
        </Button>
      </CtProvisionCenter>
    )
  }

  // One or more devices — list while scanning continues so late advertisers appear.
  if (ct.scannedDevices.length > 0) {
    return (
      <div className="mx-auto flex h-full min-h-0 w-full max-w-3xl flex-1 flex-col self-center py-4">
        <div className="mb-3 shrink-0 space-y-0.5 text-center">
          <h3 className="text-base font-medium">{t('devices:ble.selectDevice')}</h3>
          <p className="text-xs text-muted-foreground">
            {ct.scanning
              ? t('devices:ble.scanningFound', { count: ct.scannedDevices.length })
              : t('devices:ble.foundDevices', { count: ct.scannedDevices.length })}
          </p>
        </div>
        <div className="min-h-0 w-full flex-1 space-y-2 overflow-y-auto">
          {ct.scannedDevices.map((dev) => {
            const busy = dev.discovered?.busy
            const codeName =
              dev.discovered?.modelCode != null
                ? modelNameFromCode(dev.discovered.modelCode, deviceTypes) ||
                  dev.discovered.modelCodeName
                : dev.discovered?.modelCodeName
            const label =
              dev.discovered?.displayName ||
              dev.name ||
              codeName ||
              dev.id
            return (
              <button
                key={dev.id}
                type="button"
                onClick={() => !busy && !ct.connecting && handleSelectDevice(dev)}
                disabled={!!busy || ct.connecting}
                className="flex w-full items-center gap-3 rounded-xl border border-border bg-card p-3 text-left transition-colors hover:bg-muted-30 disabled:opacity-50"
              >
                <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-primary-light">
                  <Bluetooth className="h-4 w-4 text-primary" />
                </div>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium">{label}</p>
                  <p className="text-xs text-muted-foreground">
                    {busy
                      ? t('devices:ble.busy')
                      : codeName || t('devices:ble.tabTitle')}
                    {dev.discovered?.macSuffix ? ` · ${dev.discovered.macSuffix}` : ''}
                  </p>
                </div>
                {dev.rssi != null && (
                  <div className="flex shrink-0 items-center gap-1 text-xs text-muted-foreground">
                    <Signal className="h-3 w-3" />
                    {dev.rssi} dBm
                  </div>
                )}
              </button>
            )
          })}
        </div>
        <div className="mt-auto flex shrink-0 flex-col items-center gap-2 pt-3">
          {ct.scanning ? (
            <p className="flex items-center gap-2 text-xs text-muted-foreground">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {t('devices:ble.scanningHint')}
            </p>
          ) : null}
          <div className="flex gap-2">
            <Button variant="outline" onClick={reset} disabled={ct.connecting}>
              {t('common:cancel')}
            </Button>
            <Button variant="outline" onClick={handleScan} disabled={ct.scanning || ct.connecting}>
              <Bluetooth className="mr-2 h-4 w-4" />
              {t('devices:ble.rescan')}
            </Button>
          </div>
        </div>
      </div>
    )
  }

  // Initial pre-scan: Manual/Auto-parity hero (md / text-lg / text-sm); mid-pane kept.
  return (
    <CtProvisionCenter>
      <CtProvisionHero size="md">
        <Bluetooth className="h-7 w-7 text-primary" />
      </CtProvisionHero>
      <div className="max-w-sm space-y-2 text-center">
        <h3 className="text-lg font-medium">{t('devices:ble.scanTitle')}</h3>
        <p className="text-sm text-muted-foreground">{t('devices:ble.scanDescShort')}</p>
      </div>

      {!bleSupported ? (
        <div className="max-w-sm space-y-1 text-center">
          <p className="text-sm text-warning">{t('devices:ble.notSupported')}</p>
          <p className="text-xs text-muted-foreground">{t('devices:ble.notSupportedHint')}</p>
        </div>
      ) : (
        <Button onClick={handleScan} size="lg" className="min-w-[200px] shadow-none">
          <Bluetooth className="mr-2 h-4 w-4" />
          {t('devices:ble.scanBtn')}
        </Button>
      )}

      <p className="max-w-sm text-center text-xs text-muted-foreground">
        {t('devices:ble.scanHint')}
      </p>

      {ct.error && (
        <p className="max-w-sm text-center text-sm text-error">{bleErrorCopy(t, ct.error)}</p>
      )}
    </CtProvisionCenter>
  )
}
