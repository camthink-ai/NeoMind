import type { LucideIcon } from 'lucide-react'
import { Send, MessageSquare } from 'lucide-react'

/**
 * Field definition for an IM platform's configuration form.
 * Driven purely by data — `PlatformConfigForm` renders these generically,
 * so adding a platform never touches the form code.
 */
export interface ImPlatformField {
  /** Payload key sent to the backend, e.g. 'bot_token' / 'api_base'. */
  name: string
  /** i18n key for the field label, e.g. 'settings:im.botToken'. */
  labelKey: string
  type: 'text' | 'password'
  required: boolean
  /** Optional i18n key for the input placeholder. */
  placeholderKey?: string
  /** Optional i18n key for help text below the field. */
  helpKey?: string
}

/**
 * A selectable IM platform in the add-flow. This array is the single
 * extension seam: to wire a new platform (once its backend lands), add
 * one entry here with `available: true` and its field definitions — the
 * picker + config form pick it up automatically.
 *
 * Brand glyphs are intentionally avoided (DESIGN_SPEC: no logos); each
 * platform uses a generic lucide icon on a tinted `iconBg` background.
 */
export interface ImPlatformDef {
  /** Backend platform string, e.g. 'telegram'. */
  id: string
  /** i18n key for the display name, e.g. 'common:im.platforms.telegram'. */
  nameKey: string
  /** i18n key for the one-line card description. */
  descriptionKey: string
  icon: LucideIcon
  /** Tint classes for the icon container, e.g. 'bg-info-light text-info'. */
  iconBg: string
  /** true = selectable in the picker now; false = hidden until wired. */
  available: boolean
  fields: ImPlatformField[]
}

export const IM_PLATFORMS: ImPlatformDef[] = [
  {
    id: 'telegram',
    nameKey: 'common:im.platforms.telegram',
    descriptionKey: 'settings:im.telegramCardDesc',
    icon: Send,
    iconBg: 'bg-info-light text-info',
    available: true,
    fields: [
      {
        name: 'bot_token',
        labelKey: 'settings:im.botToken',
        type: 'password',
        required: true,
        placeholderKey: 'settings:im.botTokenPlaceholder',
        helpKey: 'settings:im.botTokenHelp',
      },
      {
        name: 'api_base',
        labelKey: 'settings:im.apiBase',
        type: 'text',
        required: false,
        placeholderKey: 'settings:im.apiBasePlaceholder',
        helpKey: 'settings:im.apiBaseHelp',
      },
    ],
  },
  {
    id: 'feishu',
    nameKey: 'common:im.platforms.feishu',
    descriptionKey: 'settings:im.feishuCardDesc',
    icon: MessageSquare,
    iconBg: 'bg-success-light text-success',
    available: true,
    fields: [
      {
        name: 'app_id',
        labelKey: 'settings:im.appId',
        type: 'text',
        required: true,
        placeholderKey: 'settings:im.appIdPlaceholder',
        helpKey: 'settings:im.appIdHelp',
      },
      {
        name: 'app_secret',
        labelKey: 'settings:im.appSecret',
        type: 'password',
        required: true,
        placeholderKey: 'settings:im.appSecretPlaceholder',
        helpKey: 'settings:im.appSecretHelp',
      },
      {
        name: 'domain',
        labelKey: 'settings:im.domain',
        type: 'text',
        required: false,
        placeholderKey: 'settings:im.domainPlaceholder',
        helpKey: 'settings:im.domainHelp',
      },
    ],
  },
  // Future platforms (Discord / Slack / DingTalk / ...): add an
  // entry here with `available: false` until the backend is wired, then
  // flip to true. The picker only renders available platforms, so
  // coming-soon platforms stay invisible by design (per product spec:
  // no stack of grey disabled cards).
]

/** Look up a platform definition by its backend id. */
export function getPlatformDef(id: string): ImPlatformDef | undefined {
  return IM_PLATFORMS.find(p => p.id === id)
}
