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
import { Download } from 'lucide-react'
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
      className="shrink-0 text-success hover:text-success no-press-scale"
      onClick={() => {
        if (isTauriEnv()) setUpdateDialogOpen(true)
        else setServerUpgradeDialogOpen(true)
      }}
    >
      <Download className="h-4 w-4" />
    </Button>
  )
}
