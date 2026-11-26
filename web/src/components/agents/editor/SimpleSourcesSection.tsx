/**
 * Plain-language source picker — devices + extensions as chips.
 * Writes InputRequirement entries with sensible freshness defaults.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Loader2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { api } from '@/lib/api'
import type { Device, Extension, InputContract, InputRequirement } from '@/types'

export interface SimpleSourcesSectionProps {
  value: InputContract
  onChange: (next: InputContract) => void
  /** Default freshness window for newly picked sources (secs). */
  freshnessSecs?: number
  className?: string
}

const DEFAULT_FRESHNESS = 3600

function deviceReq(device: Device, freshnessSecs: number): InputRequirement {
  const id = device.device_id || device.id
  return {
    key: `device_${id}`.replace(/[^a-zA-Z0-9_]/g, '_'),
    source: { kind: 'whole_device', device_id: id },
    timeout_secs: 10,
    result_type: 'structured_json',
    freshness: { kind: 'latest_within', max_age_secs: freshnessSecs },
    on_missing: 'report_stale',
  }
}

function extensionReq(ext: Extension, freshnessSecs: number): InputRequirement {
  const op = ext.commands?.[0]?.id
  return {
    key: `ext_${ext.id}`.replace(/[^a-zA-Z0-9_]/g, '_'),
    source: { kind: 'extension_instance', id: ext.id },
    operation: op || undefined,
    auth_scope: `extension:${ext.id}:read`,
    timeout_secs: 10,
    result_type: 'structured_json',
    freshness: { kind: 'latest_within', max_age_secs: freshnessSecs },
    on_missing: 'report_stale',
  }
}

function isDeviceSelected(reqs: InputRequirement[], deviceId: string): boolean {
  return reqs.some(
    (r) =>
      (r?.source?.kind === 'whole_device' && r.source.device_id === deviceId) ||
      (r?.source?.kind === 'device_metric' && r.source.device_id === deviceId),
  )
}

function isExtSelected(reqs: InputRequirement[], extId: string): boolean {
  return reqs.some((r) => r?.source?.kind === 'extension_instance' && r.source.id === extId)
}

export function SimpleSourcesSection({
  value,
  onChange,
  freshnessSecs = DEFAULT_FRESHNESS,
  className,
}: SimpleSourcesSectionProps) {
  const { t } = useTranslation('agents')
  const [devices, setDevices] = useState<Device[]>([])
  const [extensions, setExtensions] = useState<Extension[]>([])
  const [loading, setLoading] = useState(true)
  const requirements = value?.requirements ?? []

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    void (async () => {
      try {
        const [devResp, extList] = await Promise.all([
          api.getDevices().catch(() => ({ devices: [] as Device[], count: 0 })),
          api.listExtensions().catch(() => [] as Extension[]),
        ])
        if (cancelled) return
        setDevices(devResp.devices ?? [])
        setExtensions(Array.isArray(extList) ? extList : [])
      } finally {
        if (!cancelled) setLoading(false)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])

  const toggleDevice = (device: Device) => {
    const id = device.device_id || device.id
    if (isDeviceSelected(requirements, id)) {
      onChange({
        requirements: requirements.filter(
          (r) =>
            !(
              (r.source.kind === 'whole_device' && r.source.device_id === id) ||
              (r.source.kind === 'device_metric' && r.source.device_id === id)
            ),
        ),
      })
      return
    }
    onChange({ requirements: [...requirements, deviceReq(device, freshnessSecs)] })
  }

  const toggleExtension = (ext: Extension) => {
    if (isExtSelected(requirements, ext.id)) {
      onChange({
        requirements: requirements.filter(
          (r) => !(r.source.kind === 'extension_instance' && r.source.id === ext.id),
        ),
      })
      return
    }
    onChange({ requirements: [...requirements, extensionReq(ext, freshnessSecs)] })
  }

  if (loading) {
    return (
      <div className={cn('flex items-center gap-2 text-sm text-muted-foreground', className)}>
        <Loader2 className="h-4 w-4 animate-spin" aria-label={t('editorV2.sources.loading')} />
      </div>
    )
  }

  return (
    <div className={cn('space-y-4', className)} data-section="sources-simple">
      {devices.length > 0 && (
        <div className="space-y-2">
          <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
            {t('editorV2.sources.devices')}
          </p>
          <div className="flex flex-wrap gap-2">
            {devices.map((d) => {
              const id = d.device_id || d.id
              const on = isDeviceSelected(requirements, id)
              return (
                <Button
                  key={id}
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => toggleDevice(d)}
                  className={cn(
                    on && 'border-primary bg-muted text-foreground',
                    !on && 'text-muted-foreground',
                  )}
                >
                  {d.name || id}
                </Button>
              )
            })}
          </div>
        </div>
      )}

      {extensions.length > 0 && (
        <div className="space-y-2">
          <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
            {t('editorV2.sources.extensions')}
          </p>
          <div className="flex flex-wrap gap-2">
            {extensions.map((ext) => {
              const on = isExtSelected(requirements, ext.id)
              return (
                <Button
                  key={ext.id}
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => toggleExtension(ext)}
                  className={cn(
                    on && 'border-primary bg-muted text-foreground',
                    !on && 'text-muted-foreground',
                  )}
                >
                  {ext.name || ext.id}
                </Button>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
}
