/**
 * Feature flag for the M3 AgentSpec-v2 editor modules.
 *
 * Default ON for the clean-slate release. Opt out via:
 * - URL `?editor=v1`
 * - localStorage `neomind.agentEditorV2` = `0`
 * - Vite env `VITE_AGENT_EDITOR_V2=false`
 *
 * Explicit opt-in still works: `?editor=v2`, localStorage `1`, or env `true`.
 */
export const AGENT_EDITOR_V2_STORAGE_KEY = 'neomind.agentEditorV2'

export function isAgentEditorV2Enabled(
  search: string = typeof window !== 'undefined' ? window.location.search : '',
): boolean {
  if (import.meta.env.VITE_AGENT_EDITOR_V2 === 'false') return false
  if (import.meta.env.VITE_AGENT_EDITOR_V2 === 'true') return true
  try {
    if (typeof localStorage !== 'undefined') {
      const stored = localStorage.getItem(AGENT_EDITOR_V2_STORAGE_KEY)
      if (stored === '0') return false
      if (stored === '1') return true
    }
  } catch {
    // private mode / blocked storage
  }
  const params = new URLSearchParams(search.startsWith('?') ? search : `?${search}`)
  const editor = params.get('editor')
  if (editor === 'v1') return false
  if (editor === 'v2') return true
  // M3 default: v2 editor is the product surface.
  return true
}
