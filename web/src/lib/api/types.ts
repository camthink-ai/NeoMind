/** Shared response interfaces split from api.ts 2026-09. */

/** Shared response interfaces split from api.ts 2026-09. */
// ========== IM Bridges API ==========
export interface ImBridge {
  id: string
  platform: string
  status: string
}
export interface ImInvite {
  token: string
  created_at: number
  used: boolean
  bound_chat_id: string | null
  bound_at: number | null
}
export interface ImInviteCreated {
  token: string
  deep_link: string | null
}
export interface ImSession {
  chat_id: string
  bound_agent_id: string
  neo_session_id: string
  last_active: number
  created_at: number
}

// ========== Server self-upgrade (admin, browser/server deployments) ==========
/** GET /api/system/upgrade/check */
export interface ServerUpgradeCheck {
  /** Whether web-triggered upgrade can run on this server. */
  supported: boolean
  /** "docker" | "systemd" | "unsupported" */
  deployment: string
  /** Whether the root helper units are installed (install.sh). */
  helper_available: boolean
  current_version: string
  latest_version?: string | null
  /** Release-notes markdown for `latest_version`. */
  release_notes?: string | null
  /** `latest_version` is strictly newer than `current_version`. */
  available: boolean
  /** Operator hint when upgrade cannot proceed (both languages, \n-separated). */
  notes?: string | null
}

/** Upgrade phases — mirrors `upgrade::service::phase` on the backend. */
export type ServerUpgradePhase =
  | 'idle'
  | 'checking'
  | 'downloading'
  | 'verifying'
  | 'staged'
  | 'applying'
  | 'restarting'
  | 'done'
  | 'error'

/** GET /api/system/upgrade/status */
export interface ServerUpgradeStatus {
  running: boolean
  phase: ServerUpgradePhase
  target_version?: string | null
  downloaded: number
  total: number
  error?: string | null
}

// ============================================================================
// API Methods
// ============================================================================

