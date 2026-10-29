/**
 * RuntimeUpgradeDialog Component
 *
 * Standalone llama.cpp runtime upgrade dialog — the builtin-LLM settings
 * card's "runtime" row opens it. Mirrors ServerUpgradeDialog's mechanics:
 * on open it force-refreshes the update channel
 * (`POST /api/builtin-llm/runtime/check`), the upgrade button kicks
 * `POST /api/builtin-llm/runtime/upgrade`, and progress comes from
 * `RuntimeUpgradeProgress` WS events (unfiltered 'all' stream) plus a 3s
 * `GET /api/builtin-llm/runtime` poll fallback. "Done" requires the poll
 * to confirm the active tag reached the target and the upgrade is no
 * longer running — events alone don't prove the respawn finished.
 *
 * Also hosts the runtime variant override (auto / cpu / cuda / vulkan /
 * jetson) — persisted via POST /api/builtin-llm/runtime/variant and
 * applied on the next spawn.
 */

import { useState, useEffect, useRef, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Progress } from '@/components/ui/progress'
import { AlertCircle, ArrowUpRight, Check, Cpu, Download, History, Loader2 } from 'lucide-react'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { useEvents } from '@/hooks/useEvents'
import { api } from '@/lib/api'
import type { BuiltinRuntimeInfo } from '@/types/llm-backend'
import type { RuntimeUpgradeProgressEvent } from '@/lib/events'
import { notifySuccess } from '@/lib/notify'
import { cn } from '@/lib/utils'
import { badgeSize } from '@/design-system/tokens/typography'

const STATUS_POLL_MS = 3000

type Stage = 'checking' | 'idle' | 'upgrading' | 'restarting' | 'done' | 'error'

export interface RuntimeUpgradeDialogProps {
  open: boolean
  onClose: () => void
  /** Latest known snapshot (the card's); refreshed internally on open/actions. */
  runtime: BuiltinRuntimeInfo | null
  /** Notify the parent to refresh its status poll cache. */
  onChanged: () => void
}

const VARIANT_CHOICES = ['auto', 'cpu', 'cuda', 'vulkan', 'jetson'] as const

function fmtBytes(n: number | null | undefined): string {
  if (!n || n <= 0) return '—'
  const mb = n / (1024 * 1024)
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb.toFixed(1)} MB`
}

export function RuntimeUpgradeDialog({ open, onClose, runtime, onChanged }: RuntimeUpgradeDialogProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const [stage, setStage] = useState<Stage>('checking')
  const [info, setInfo] = useState<BuiltinRuntimeInfo | null>(runtime)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const [downloaded, setDownloaded] = useState(0)
  const [total, setTotal] = useState<number | null>(null)
  const [targetTag, setTargetTag] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const doneRef = useRef(false)
  const [sources, setSources] = useState<{
    hf_base_url: string; hf_custom: boolean
    catalog_url: string; catalog_custom: boolean
    runtime_channel_url: string; runtime_channel_custom: boolean
    last_download_error?: string | null
  } | null>(null)

  const refresh = useCallback(async (force = false) => {
    const snap = force ? await api.checkBuiltinRuntimeUpdate() : await api.getBuiltinRuntime()
    setInfo(snap)
    return snap
  }, [])

  const refreshSources = useCallback(async () => {
    try {
      const s = await api.getLlmSources()
      setSources(s)
    } catch {
      /* non-admin or transient — the block simply stays hidden */
    }
  }, [])

  // ---- Check on open ------------------------------------------------------
  useEffect(() => {
    if (!open) return
    doneRef.current = false
    setStage('checking')
    setErrorMsg(null)
    setDownloaded(0)
    setTotal(null)
    setTargetTag(null)
    void refreshSources()
    refresh(true)
      .then(() => setStage('idle'))
      .catch((e) => {
        // 403 (non-admin) or channel unreachable — the snapshot endpoint
        // itself never fails on offline, only the force-check can.
        setErrorMsg(e instanceof Error ? e.message : String(e))
        setStage('error')
      })
  }, [open, refresh, refreshSources])

  // ---- Progress via WS events (unfiltered 'all' stream) -------------------
  useEvents({
    category: 'all',
    eventTypes: ['RuntimeUpgradeProgress'],
    enabled: open,
    onEvent: (event) => {
      if (event.type !== 'RuntimeUpgradeProgress') return
      const d = (event as RuntimeUpgradeProgressEvent).data
      if (typeof d.downloaded === 'number') setDownloaded(d.downloaded)
      if (typeof d.total === 'number' && d.total > 0) setTotal(d.total)
      if (d.target_tag) setTargetTag(d.target_tag)
      if (d.phase === 'restarting') setStage((s) => (s === 'upgrading' ? 'restarting' : s))
      if (d.phase === 'error') {
        setErrorMsg(d.error ?? t('plugins:llm.runtimeUpgradeFailed'))
        setStage('error')
        onChanged()
      }
    },
  })

  // ---- Poll fallback + done detection -------------------------------------
  useEffect(() => {
    if (!open || (stage !== 'upgrading' && stage !== 'restarting')) return
    const id = setInterval(async () => {
      try {
        const snap = await api.getBuiltinRuntime()
        setInfo(snap)
        if (!snap.upgrading && !doneRef.current) {
          doneRef.current = true
          const reached = targetTag ? snap.active.tag === targetTag : true
          if (reached) {
            setStage('done')
            notifySuccess(t('plugins:llm.runtimeUpgraded', { tag: snap.active.tag }))
            onChanged()
          } else {
            // Finished without reaching the target — surface what happened.
            setErrorMsg(
              snap.channel_error ??
                t('plugins:llm.runtimeUpgradeStalled', { active: snap.active.tag }),
            )
            setStage('error')
            onChanged()
          }
        }
      } catch {
        /* transient — the next tick retries */
      }
    }, STATUS_POLL_MS)
    return () => clearInterval(id)
  }, [open, stage, targetTag, t, onChanged])

  const startUpgrade = async () => {
    setBusy(true)
    try {
      const res = await api.upgradeBuiltinRuntime()
      if (res.started) {
        setStage('upgrading')
        setDownloaded(0)
        setTargetTag(info?.recommended?.tag ?? null)
      }
    } catch (e) {
      setErrorMsg(e instanceof Error ? e.message : String(e))
      setStage('error')
    } finally {
      setBusy(false)
    }
  }

  const doRevert = async () => {
    setBusy(true)
    try {
      const res = await api.revertBuiltinRuntime()
      notifySuccess(t('plugins:llm.runtimeReverted', { tag: res.removed_tag }))
      await refresh(false)
      onChanged()
    } catch (e) {
      setErrorMsg(e instanceof Error ? e.message : String(e))
      setStage('error')
    } finally {
      setBusy(false)
    }
  }

  const saveSource = async (kind: 'hf_base_url' | 'catalog_url' | 'runtime_channel_url', value: string) => {
    setBusy(true)
    try {
      await api.putLlmSource(kind, value)
      notifySuccess(t('plugins:llm.runtimeSourceSaved'))
      await refreshSources()
    } catch (e) {
      setErrorMsg(e instanceof Error ? e.message : String(e))
      setStage('error')
    } finally {
      setBusy(false)
    }
  }

  const setVariant = async (v: string) => {
    setBusy(true)
    try {
      await api.setBuiltinRuntimeVariant(v)
      await refresh(false)
    } catch (e) {
      setErrorMsg(e instanceof Error ? e.message : String(e))
      setStage('error')
    } finally {
      setBusy(false)
    }
  }

  const active = info?.active
  const currentTag = active?.tag ?? '—'
  const rec = info?.recommended
  const pct = total && total > 0 ? Math.min(100, (downloaded / total) * 100) : 0
  const canRevert = (info?.installed?.length ?? 0) > 0
  const effectiveVariant = info?.override_variant ?? info?.detected_variant ?? 'cpu'

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={(o) => !o && onClose()}
      title={t('plugins:llm.runtimeDialogTitle')}
      description={t('plugins:llm.runtimeDialogDesc')}
      className="z-[110]"
      footer={
        <div className="flex w-full flex-wrap items-center gap-2">
          {stage === 'idle' && (
            <>
              <Button
                size="sm"
                onClick={startUpgrade}
                disabled={busy || !info?.update_available}
              >
                {busy ? (
                  <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Download className="mr-1.5 h-3.5 w-3.5" />
                )}
                {t('plugins:llm.runtimeUpgradeTo', { tag: rec?.tag ?? '' })}
              </Button>
              <Button size="sm" variant="outline" onClick={doRevert} disabled={busy || !canRevert}>
                <History className="mr-1.5 h-3.5 w-3.5" />
                {t('plugins:llm.runtimeRevert')}
              </Button>
            </>
          )}
          {(stage === 'upgrading' || stage === 'restarting') && (
            <span className="flex items-center gap-2 text-xs text-muted-foreground">
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {stage === 'upgrading'
                ? t('plugins:llm.runtimePhaseDownloading')
                : t('plugins:llm.runtimePhaseRestarting')}
            </span>
          )}
          <Button size="sm" variant="secondary" onClick={onClose} className="ml-auto">
            {t('common:close')}
          </Button>
        </div>
      }
    >
      <div className="space-y-4">
        {/* Version line: current → recommended */}
        <div className="flex items-center gap-2 text-sm">
          <Cpu className="h-4 w-4 shrink-0 text-muted-foreground" />
          <span className="font-mono">{currentTag}</span>
          {info?.update_available && rec && (
            <>
              <ArrowUpRight className="h-3.5 w-3.5 text-muted-foreground" />
              <span className="font-mono text-primary">{rec.tag}</span>
            </>
          )}
          <span className={cn(badgeSize.small, 'rounded bg-muted px-1.5 py-0.5 text-muted-foreground')}>
            {effectiveVariant.toUpperCase()}
          </span>
        </div>

        {/* Channel state */}
        {info?.channel === 'unreachable' && (
          <p className="text-xs text-muted-foreground">{t('plugins:llm.runtimeChannelUnreachable')}</p>
        )}
        {info?.update_available && rec && (
          <p className="text-xs text-muted-foreground">
            {t('plugins:llm.runtimeUpdateAvailable', {
              tag: rec.tag,
              notes: rec.notes_zh || rec.notes_en || '',
            })}
          </p>
        )}
        {stage === 'idle' && !info?.update_available && info?.channel === 'ok' && (
          <p className="text-xs text-muted-foreground">{t('plugins:llm.runtimeUpToDate')}</p>
        )}

        {/* Progress */}
        {(stage === 'upgrading' || stage === 'restarting') && (
          <div className="space-y-1">
            <Progress value={stage === 'restarting' ? 100 : pct} />
            <p className="text-xs text-muted-foreground">
              {stage === 'restarting'
                ? t('plugins:llm.runtimePhaseRestarting')
                : `${fmtBytes(downloaded)} / ${fmtBytes(total)}`}
            </p>
          </div>
        )}

        {/* Done / error cards */}
        {stage === 'done' && (
          <div className="flex items-center gap-2 rounded-md bg-success-light px-3 py-2 text-success">
            <Check className="h-4 w-4 shrink-0" />
            <span className="text-xs">{t('plugins:llm.runtimeUpgraded', { tag: currentTag })}</span>
          </div>
        )}
        {stage === 'error' && errorMsg && (
          <div className="flex items-start gap-2 rounded-md bg-destructive/10 px-3 py-2 text-destructive">
            <AlertCircle className="h-4 w-4 shrink-0 mt-0.5" />
            <span className="text-xs leading-snug break-all">{errorMsg}</span>
          </div>
        )}

        {/* Variant override */}
        <div className="space-y-1.5">
          <p className="text-xs font-medium">{t('plugins:llm.runtimeVariantLabel')}</p>
          <div className="flex flex-wrap gap-1.5">
            {VARIANT_CHOICES.map((v) => {
              const isCurrent =
                v === 'auto' ? !info?.override_variant : info?.override_variant === v
              return (
                <button
                  key={v}
                  disabled={busy}
                  onClick={() => setVariant(v === 'auto' ? '' : v)}
                  className={cn(
                    'rounded border px-2 py-1 text-xs transition-colors disabled:opacity-50',
                    isCurrent
                      ? 'border-primary bg-primary text-primary-foreground'
                      : 'border-border bg-background text-muted-foreground hover:bg-accent',
                  )}
                >
                  {v === 'auto' ? t('plugins:llm.runtimeVariantAuto') : v.toUpperCase()}
                </button>
              )
            })}
          </div>
          <p className="text-xs text-muted-foreground">{t('plugins:llm.runtimeVariantHint')}</p>
        </div>

        {/* Download sources (mirrors) — the restricted-network escape hatch. */}
        {sources && (
          <div className="space-y-1.5 border-t border-border pt-3">
            <p className="text-xs font-medium">{t('plugins:llm.runtimeSourcesTitle')}</p>
            {sources.last_download_error && (
              <p className="text-xs text-warning break-all">
                {t('plugins:llm.runtimeLastDownloadError', { error: sources.last_download_error })}
              </p>
            )}
            <p className="text-xs text-muted-foreground">{t('plugins:llm.runtimeSourcesHint')}</p>
            {(
              [
                ['hf_base_url', 'runtimeSourceHf', sources.hf_base_url, sources.hf_custom],
                ['catalog_url', 'runtimeSourceCatalog', sources.catalog_url, sources.catalog_custom],
                ['runtime_channel_url', 'runtimeSourceChannel', sources.runtime_channel_url, sources.runtime_channel_custom],
              ] as const
            ).map(([kind, labelKey, value, custom]) => (
              <div key={kind} className="flex items-center gap-2">
                <span className="w-32 shrink-0 truncate text-xs text-muted-foreground">
                  {t(`plugins:llm.${labelKey}`)}
                </span>
                <input
                  className="h-8 min-w-0 flex-1 rounded border border-border bg-background px-2 font-mono text-xs"
                  defaultValue={value}
                  disabled={busy}
                  onBlur={(e) => {
                    if (e.target.value.trim() !== value) void saveSource(kind, e.target.value)
                  }}
                />
                {custom && (
                  <span className="shrink-0 rounded bg-warning-light px-1.5 py-0.5 text-[10px] text-warning">
                    {t('plugins:llm.runtimeSourceCustom')}
                  </span>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </UnifiedFormDialog>
  )
}
