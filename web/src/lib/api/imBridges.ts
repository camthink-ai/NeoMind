/** imBridges api — split from api.ts 2026-09. */
import { fetchAPI } from "./client"
import type { ImBridge, ImInvite, ImInviteCreated, ImSession } from "./types"

export const imBridgesApi = {
  // ========== IM Bridges API ==========
  listImBridges: () => fetchAPI<{ bridges: ImBridge[] }>('/im-bridges'),
  // Mirrors the backend `CreateBridgeRequest` (im_bridges.rs): credential
  // fields are all optional because they are platform-specific — `bot_token`
  // + `api_base` for Telegram, `app_id` + `app_secret` + `domain` for Feishu.
  createImBridge: (req: {
    platform: string
    bot_token?: string
    api_base?: string
    app_id?: string
    app_secret?: string
    domain?: string
  }) => fetchAPI<ImBridge>('/im-bridges', { method: 'POST', body: JSON.stringify(req) }),
  deleteImBridge: (id: string) =>
    fetchAPI<{ id: string; status: string }>(`/im-bridges/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  createImInvite: (id: string) =>
    fetchAPI<ImInviteCreated>(`/im-bridges/${encodeURIComponent(id)}/invites`, { method: 'POST' }),
  listImInvites: (id: string) =>
    fetchAPI<{ invites: ImInvite[] }>(`/im-bridges/${encodeURIComponent(id)}/invites`),
  revokeImInvite: (id: string, token: string) =>
    fetchAPI(`/im-bridges/${encodeURIComponent(id)}/invites/${encodeURIComponent(token)}`, { method: 'DELETE' }),
  listImAllowlist: (id: string) =>
    fetchAPI<{ allowlist: string[] }>(`/im-bridges/${encodeURIComponent(id)}/allowlist`),
  removeImAllowed: (id: string, chat_id: string) =>
    fetchAPI(`/im-bridges/${encodeURIComponent(id)}/allowlist/${encodeURIComponent(chat_id)}`, { method: 'DELETE' }),
  listImSessions: (id: string) =>
    fetchAPI<{ sessions: ImSession[] }>(`/im-bridges/${encodeURIComponent(id)}/sessions`),
  resetImSession: (id: string, chat_id: string) =>
    fetchAPI(`/im-bridges/${encodeURIComponent(id)}/sessions/${encodeURIComponent(chat_id)}/reset`, { method: 'POST' }),

}
