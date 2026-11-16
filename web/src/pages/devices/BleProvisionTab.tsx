/**
 * Bluetooth Device Provisioning — first viewport is scan + pick only.
 * After GATT + CT-Prov session open, shared wizard: info → caps → edit → apply.
 */

import { useState, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Bluetooth, Loader2, Signal } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { useCtProvision } from '@/hooks/useCtProvision'
import {
  isTauriEnv,
  modelNameFromCode,
  type CtScanDevice,
} from '@/lib/ct-prov'
import { useStore } from '@/store'
import { CtProvisionCenter, CtProvisionWizard } from './CtProvisionWizard'

interface BleProvisionTabProps {
  onComplete?: (deviceId: string) => void
}

type Phase = 'scan' | 'connecting' | 'wizard' | 'failed'

export function BleProvisionTab({ onComplete }: BleProvisionTabProps) {
  const { t } = useTranslation(['common', 'devices'])
  const ct = useCtProvision('ble')
  const deviceTypes = useStore((s) => s.deviceTypes)

  const [phase, setPhase] = useState<Phase>('scan')

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
        <div className="w-16 h-16 rounded-xl bg-error-light flex items-center justify-center">
          <Bluetooth className="w-8 h-8 text-error" />
        </div>
        <div className="text-center max-w-sm">
          <h3 className="text-lg font-medium">{t('devices:ble.failedTitle')}</h3>
          <p className="text-sm text-muted-foreground mt-1">{ct.error}</p>
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
        <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
          <Loader2 className="w-8 h-8 text-primary animate-spin" />
        </div>
        <div className="text-center space-y-2">
          <h3 className="text-lg font-medium">{t('devices:ble.connectingTitle')}</h3>
          <p className="text-sm text-muted-foreground">
            {ct.selectedScan?.name || ct.selectedScan?.id}
          </p>
        </div>
      </CtProvisionCenter>
    )
  }

  if (ct.scanning) {
    return (
      <CtProvisionCenter>
        <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
          <Loader2 className="w-8 h-8 text-primary animate-spin" />
        </div>
        <div className="text-center space-y-2">
          <h3 className="text-lg font-medium">{t('devices:ble.scanning')}</h3>
          <p className="text-sm text-muted-foreground">{t('devices:ble.scanningHint')}</p>
        </div>
      </CtProvisionCenter>
    )
  }

  if (ct.scannedDevices.length > 0) {
    return (
      <div className="min-h-[420px] sm:min-h-[480px] flex flex-col px-4 py-6">
        <div className="text-center mb-6">
          <p className="text-xs font-medium text-muted-foreground tracking-wide mb-1">
            {t('devices:ctProv.productName')}
          </p>
          <h3 className="text-lg font-medium">{t('devices:ble.selectDevice')}</h3>
          <p className="text-sm text-muted-foreground mt-1">
            {t('devices:ble.foundDevices', { count: ct.scannedDevices.length })}
          </p>
        </div>
        <div className="space-y-2 max-w-md mx-auto w-full">
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
                onClick={() => !busy && handleSelectDevice(dev)}
                disabled={!!busy}
                className="w-full flex items-center gap-3 rounded-xl border bg-card p-3 hover:bg-muted-30 transition-colors text-left disabled:opacity-50"
              >
                <div className="w-10 h-10 rounded-lg bg-primary-light flex items-center justify-center shrink-0">
                  <Bluetooth className="w-5 h-5 text-primary" />
                </div>
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-medium truncate">{label}</p>
                  <p className="text-xs text-muted-foreground">
                    {busy ? t('devices:ble.busy') : t('devices:ctProv.productName')}
                    {dev.discovered?.macSuffix ? ` · ${dev.discovered.macSuffix}` : ''}
                  </p>
                </div>
                {dev.rssi != null && (
                  <div className="flex items-center gap-1 text-xs text-muted-foreground shrink-0">
                    <Signal className="w-3 h-3" />
                    {dev.rssi} dBm
                  </div>
                )}
              </button>
            )
          })}
        </div>
        <div className="flex justify-center mt-6">
          <Button variant="outline" onClick={handleScan} disabled={ct.scanning}>
            <Bluetooth className="w-4 h-4 mr-2" />
            {t('devices:ble.rescan')}
          </Button>
        </div>
      </div>
    )
  }

  return (
    <CtProvisionCenter>
      <div className="w-16 h-16 rounded-xl bg-primary-light flex items-center justify-center">
        <Bluetooth className="w-8 h-8 text-primary" />
      </div>
      <div className="text-center max-w-sm space-y-2">
        <p className="text-xs font-medium text-muted-foreground tracking-wide">
          {t('devices:ctProv.productName')}
        </p>
        <h3 className="text-lg font-medium">{t('devices:ble.scanTitle')}</h3>
        <p className="text-sm text-muted-foreground">{t('devices:ble.scanDescShort')}</p>
      </div>

      {!bleSupported ? (
        <div className="text-center space-y-3 max-w-sm">
          <p className="text-sm text-warning">{t('devices:ble.notSupported')}</p>
          <p className="text-xs text-muted-foreground">{t('devices:ble.notSupportedHint')}</p>
        </div>
      ) : (
        <Button onClick={handleScan} size="lg" className="min-w-[200px]">
          <Bluetooth className="w-4 h-4 mr-2" />
          {t('devices:ble.scanBtn')}
        </Button>
      )}

      <p className="text-xs text-muted-foreground text-center max-w-sm">
        {t('devices:ble.scanHint')}
      </p>

      {ct.error && <p className="text-sm text-error text-center max-w-sm">{ct.error}</p>}
    </CtProvisionCenter>
  )
}
