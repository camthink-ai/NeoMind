/**
 * UpdateAvailableButton — top-right quick affordance while an update is
 * available (next to the theme/language/alerts cluster).
 *
 * The 24h auto-check (or a manual About-page check) populates the shared
 * updateInfo slice in BOTH environments; this button makes that state
 * reachable from any page instead of only Settings → About. Click opens the
 * environment's dialog: the desktop OTA dialog in Tauri, the server
 * self-upgrade dialog in a browser session.
 */

import { useTranslation } from 'react-i18next'
import { ArrowUpCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { isTauriEnv } from '@/lib/api'
import { useAppStore } from '@/store'

export function UpdateAvailableButton() {
  const { t } = useTranslation(['common', 'settings'])
  const { updateInfo, setUpdateDialogOpen, setServerUpgradeDialogOpen } = useAppStore()

  if (!updateInfo?.available) return null

  const title = updateInfo.version
    ? `${t('settings:updateNow')} · v${updateInfo.version}`
    : t('settings:updateNow')

  return (
    <Button
      variant="ghost"
      size="icon-sm"
      aria-label={title}
      title={title}
      className="relative shrink-0 text-info hover:text-info no-press-scale"
      onClick={() => {
        if (isTauriEnv()) setUpdateDialogOpen(true)
        else setServerUpgradeDialogOpen(true)
      }}
    >
      <ArrowUpCircle className="h-4 w-4" />
      {/* Pulse dot — the icon itself is subtle; the dot carries the "new"
          signal at a glance without a full badge's visual weight. */}
      <span className="absolute -right-0.5 -top-0.5 flex h-2 w-2">
        <span className="absolute inline-flex h-full w-full rounded-full bg-info opacity-75 animate-ping" />
        <span className="relative inline-flex h-2 w-2 rounded-full bg-info" />
      </span>
    </Button>
  )
}
