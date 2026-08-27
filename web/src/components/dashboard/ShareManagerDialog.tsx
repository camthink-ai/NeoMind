/**
 * ShareManagerDialog
 *
 * Full-screen dialog for managing dashboard share links.
 * Main view: list of active share links.
 * "New Link" opens a secondary UnifiedFormDialog (z-[110]).
 */

import { useState, useCallback, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { fetchAPI } from '@/lib/api'
import { cn } from '@/lib/utils'
import { notifySuccess, notifyError } from '@/lib/notify'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import {
  FullScreenDialog,
  FullScreenDialogHeader,
  FullScreenDialogMain,
} from '@/components/automation/dialog'
import {
  Share2,
  Copy,
  Trash2,
  Clock,
  Plus,
  Link2,
} from 'lucide-react'
import { textNano, textMini } from '@/design-system/tokens/typography'
import { copyToClipboard } from '@/lib/clipboard'

// ============================================================================
// Types
// ============================================================================

interface ShareToken {
  token: string
  permissions: { allow_interactive: boolean }
  created_at: number
  expires_at: number | null
  share_url: string
}

interface ShareManagerDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  dashboardId: string | null
  dashboardName?: string
}

// ============================================================================
// Component
// ============================================================================

export function ShareManagerDialog({
  open,
  onOpenChange,
  dashboardId,
  dashboardName,
}: ShareManagerDialogProps) {
  const { t } = useTranslation('dashboardComponents')

  const [tokens, setTokens] = useState<ShareToken[]>([])
  const [createOpen, setCreateOpen] = useState(false)
  const [loading, setLoading] = useState(false)
  const [permissionInteractive, setPermissionInteractive] = useState(false)
  const [expiresHours, setExpiresHours] = useState<number | ''>('')

  // Fetch share tokens
  const fetchTokens = useCallback(async () => {
    if (!dashboardId) return
    try {
      const data = await fetchAPI<ShareToken[]>(`/dashboards/${dashboardId}/share`, {
        skipGlobalError: true,
        skipErrorToast: true,
      })
      setTokens(data || [])
    } catch {
      // silently fail
    }
  }, [dashboardId])

  // Load tokens when dialog opens
  useEffect(() => {
    if (open) fetchTokens()
  }, [open, fetchTokens])

  // Create share link
  const handleCreate = useCallback(async () => {
    if (!dashboardId) return
    setLoading(true)
    try {
      await fetchAPI(`/dashboards/${dashboardId}/share`, {
        method: 'POST',
        body: JSON.stringify({
          permissions: { allow_interactive: permissionInteractive },
          expires_in_hours: expiresHours || null,
        }),
      })
      notifySuccess(t('visualDashboard.share.linkCreated'))
      setCreateOpen(false)
      setPermissionInteractive(false)
      setExpiresHours('')
      await fetchTokens()
    } catch {
      notifyError(t('visualDashboard.share.createFailed'))
    } finally {
      setLoading(false)
    }
  }, [dashboardId, permissionInteractive, expiresHours, fetchTokens, notifySuccess, notifyError, t])

  // Revoke share link
  const handleRevoke = useCallback(async (token: string) => {
    if (!dashboardId) return
    try {
      await fetchAPI(`/dashboards/${dashboardId}/share/${token}`, { method: 'DELETE' })
      notifySuccess(t('visualDashboard.share.linkRevoked'))
      await fetchTokens()
    } catch {
      notifyError(t('visualDashboard.share.revokeFailed'))
    }
  }, [dashboardId, fetchTokens, notifySuccess, notifyError, t])

  // Copy share link
  const handleCopy = useCallback(async (token: string) => {
    const url = `${window.location.origin}/share/${token}`
    try {
      await copyToClipboard(url)
      notifySuccess(t('visualDashboard.share.linkCopied'))
    } catch {
      notifyError(t('visualDashboard.share.copyFailed'))
    }
  }, [notifySuccess, notifyError, t])

  return (
    <FullScreenDialog open={open} onOpenChange={onOpenChange}>
      <FullScreenDialogHeader
        icon={<Share2 className="w-5 h-5" />}
        iconBg="bg-success-light"
        iconColor="text-success"
        title={t('visualDashboard.share.title')}
        subtitle={dashboardName}
        onClose={() => onOpenChange(false)}
      />
      <FullScreenDialogMain className="p-4 md:p-6 lg:p-8">
        <div className="max-w-xl mx-auto">
          <div className="space-y-2">
            {tokens.map((st) => {
              const isExpired = st.expires_at ? st.expires_at * 1000 < Date.now() : false
              const expiresLabel = st.expires_at
                ? new Date(st.expires_at * 1000).toLocaleDateString()
                : t('visualDashboard.share.expires.never')

              return (
                <div
                  key={st.token}
                  className="flex items-center gap-3 p-3.5 rounded-xl border border-border hover:bg-muted-30 transition-colors"
                >
                  <div className="shrink-0 w-9 h-9 rounded-lg bg-muted-30 flex items-center justify-center">
                    <Link2 className="h-4 w-4 text-muted-foreground" />
                  </div>
                  <div className="flex-1 min-w-0 space-y-1">
                    <div className="flex items-center gap-2">
                      <Badge
                        variant={st.permissions.allow_interactive ? 'default' : 'secondary'}
                        className={`${textNano} px-1.5 py-0`}
                      >
                        {st.permissions.allow_interactive
                          ? t('visualDashboard.share.permission.interactive')
                          : t('visualDashboard.share.permission.readOnly')}
                      </Badge>
                      <span className={cn(
                        `${textMini} flex items-center gap-0.5`,
                        isExpired ? "text-error" : "text-muted-foreground"
                      )}>
                        <Clock className="h-3 w-3" />
                        {expiresLabel}
                      </span>
                    </div>
                    <div className="font-mono text-xs text-muted-foreground break-all select-all">
                      {window.location.origin}/share/{st.token}
                    </div>
                  </div>
                  <div className="flex items-center gap-0.5 shrink-0">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8"
                      onClick={() => handleCopy(st.token)}
                      aria-label="Copy link"
                    >
                      <Copy className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8 text-error"
                      onClick={() => handleRevoke(st.token)}
                      aria-label={t('visualDashboard.share.revokeConfirm')}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </div>
              )
            })}

            {/* Add Share — dashed card matching InstanceManager pattern */}
            <button
              onClick={() => setCreateOpen(true)}
              className="w-full rounded-xl border-2 border-dashed border-border p-4 text-sm text-muted-foreground hover:border-primary hover:text-primary transition-colors cursor-pointer"
            >
              <Plus className="h-5 w-5 mx-auto mb-1" />
              {t('visualDashboard.share.createLink')}
            </button>
          </div>
        </div>
      </FullScreenDialogMain>

      {/* Nested create dialog — z-[110] per design spec */}
      <UnifiedFormDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        title={t('visualDashboard.share.createLink')}
        icon={<Plus className="h-5 w-5" />}
        width="sm"
        className="z-[110]"
        onSubmit={handleCreate}
        submitLabel={loading ? t('visualDashboard.share.create') + '...' : t('visualDashboard.share.create')}
        submitDisabled={loading}
      >
        <div className="space-y-4">
          {/* Permission toggle */}
          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label className="text-sm">{t('visualDashboard.share.permission.label')}</Label>
              <p className="text-xs text-muted-foreground">
                {permissionInteractive
                  ? t('visualDashboard.share.permission.interactive')
                  : t('visualDashboard.share.permission.readOnly')}
              </p>
            </div>
            <Switch
              checked={permissionInteractive}
              onCheckedChange={setPermissionInteractive}
            />
          </div>

          {/* Expiration select */}
          <div className="space-y-1.5">
            <Label className="text-sm">{t('visualDashboard.share.expires.label')}</Label>
            <Select
              value={String(expiresHours || '')}
              onValueChange={(v) => setExpiresHours(v === 'never' ? '' : Number(v))}
            >
              <SelectTrigger className="w-full">
                <SelectValue placeholder={t('visualDashboard.share.expires.never')} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="never">{t('visualDashboard.share.expires.never')}</SelectItem>
                <SelectItem value="1">{t('visualDashboard.share.expires.1h')}</SelectItem>
                <SelectItem value="24">{t('visualDashboard.share.expires.24h')}</SelectItem>
                <SelectItem value="72">{t('visualDashboard.share.expires.72h')}</SelectItem>
                <SelectItem value="168">{t('visualDashboard.share.expires.168h')}</SelectItem>
                <SelectItem value="720">{t('visualDashboard.share.expires.720h')}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>
      </UnifiedFormDialog>
    </FullScreenDialog>
  )
}
