/// Mention search across the four referenceable entity types.
/// Fetched fresh at mention time so the list reflects current state; the
/// server re-resolves ids again at send time (labels can go stale — that is
/// fine, ids are authoritative).
import { devicesApi } from "./devices"
import { automationApi } from "./automation"
import { agentsApi } from "./agents"
import type { ChatReference } from "@/types"

/** Hard cap shared with the backend preamble (references beyond this are
 *  truncated server-side WITH an explicit note); the composer stops adding
 *  chips at the same number so the two never disagree. */
export const MAX_REFERENCES = 20

export type MentionType = ChatReference["type"]

export interface MentionOption {
  type: MentionType
  id: string
  label: string
}

/** Options per entity type when the query is empty. */
const PER_TYPE_EMPTY = 4
/** Options per entity type for a non-empty query. */
const PER_TYPE_MATCH = 6

function matches(haystack: string, needle: string): boolean {
  if (!needle) return true
  return haystack.toLowerCase().includes(needle.toLowerCase())
}

/**
 * Search devices / rules / transforms / agents by substring (name or id).
 * Chinese queries work naturally — plain substring match, no tokenization.
 * Failures of one list never break the others (settled independently).
 */
export async function searchMentionOptions(query: string): Promise<MentionOption[]> {
  const [devices, rules, transforms, agents] = await Promise.allSettled([
    devicesApi.getDevices(),
    automationApi.listRules(),
    automationApi.listTransforms(),
    agentsApi.listAgents(),
  ])

  const cap = query ? PER_TYPE_MATCH : PER_TYPE_EMPTY
  const out: MentionOption[] = []

  if (devices.status === "fulfilled") {
    out.push(
      ...devices.value.devices
        .filter((d) => matches(d.name ?? "", query) || matches(d.id ?? "", query))
        .slice(0, cap)
        .map((d) => ({ type: "device" as const, id: d.id, label: d.name || d.id })),
    )
  }
  if (rules.status === "fulfilled") {
    out.push(
      ...rules.value.rules
        .filter((r) => matches(r.name ?? "", query) || matches(r.id ?? "", query))
        .slice(0, cap)
        .map((r) => ({ type: "rule" as const, id: r.id, label: r.name || r.id })),
    )
  }
  if (transforms.status === "fulfilled") {
    out.push(
      ...transforms.value.transforms
        .filter((t) => matches(t.name ?? "", query) || matches(t.id ?? "", query))
        .slice(0, cap)
        .map((t) => ({ type: "transform" as const, id: t.id, label: t.name || t.id })),
    )
  }
  if (agents.status === "fulfilled") {
    out.push(
      ...agents.value.agents
        .filter((a) => matches(a.name ?? "", query) || matches(a.id ?? "", query))
        .slice(0, cap)
        .map((a) => ({ type: "agent" as const, id: a.id, label: a.name || a.id })),
    )
  }
  return out
}
