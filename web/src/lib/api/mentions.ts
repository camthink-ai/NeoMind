/// Server-backed mention search + reference preflight (1.3 iter C).
///
/// Search hits `/mentions/search` (full accessible set, pagination, duplicate
/// flags). Resolve hits `/mentions/resolve` so the composer can mark stale
/// chips before send. AbortSignal prevents stale responses from overwriting
/// a newer query.
import { fetchAPI } from "./client"
import type { ChatReference } from "@/types"

/** Hard cap shared with the backend (contentNodes + legacy). */
export const MAX_REFERENCES = 20

export type MentionType = ChatReference["type"]

export interface MentionOption {
  type: MentionType
  id: string
  label: string
  shortId?: string
  hint?: string | null
  duplicateName?: boolean
}

export interface MentionSearchResult {
  items: MentionOption[]
  total: number
  limit: number
  offset: number
  hasMore: boolean
  nextOffset: number | null
  typeErrors: {
    device?: string
    rule?: string
    transform?: string
    agent?: string
  }
}

export interface MentionResolveItem {
  index: number
  type: string
  id: string
  ok: boolean
  detail: string
}

export interface MentionResolveResult {
  resolutions: MentionResolveItem[]
  ok: boolean
}

export interface SearchMentionParams {
  query?: string
  types?: MentionType[] | "all"
  limit?: number
  offset?: number
  signal?: AbortSignal
}

function mapItem(raw: Record<string, unknown>): MentionOption {
  return {
    type: String(raw.type) as MentionType,
    id: String(raw.id ?? ""),
    label: String(raw.label ?? raw.id ?? ""),
    shortId: raw.shortId != null ? String(raw.shortId) : undefined,
    hint: raw.hint != null ? String(raw.hint) : null,
    duplicateName: Boolean(raw.duplicateName),
  }
}

/**
 * Search devices / rules / transforms / agents on the server.
 * Chinese queries are plain substring matches (no tokenization).
 */
export async function searchMentionOptions(
  query: string,
  params: Omit<SearchMentionParams, "query"> = {},
): Promise<MentionOption[]> {
  const result = await searchMentions({ ...params, query })
  return result.items
}

/** Full search response (pagination + typeErrors). */
export async function searchMentions(
  params: SearchMentionParams = {},
): Promise<MentionSearchResult> {
  const q = params.query ?? ""
  const limit = params.limit ?? 20
  const offset = params.offset ?? 0
  const types =
    !params.types || params.types === "all"
      ? undefined
      : params.types.join(",")

  const sp = new URLSearchParams()
  sp.set("q", q)
  sp.set("limit", String(limit))
  sp.set("offset", String(offset))
  if (types) sp.set("types", types)

  const raw = await fetchAPI<{
    items: Record<string, unknown>[]
    total: number
    limit: number
    offset: number
    hasMore: boolean
    nextOffset: number | null
    typeErrors?: MentionSearchResult["typeErrors"]
  }>(`/mentions/search?${sp.toString()}`, {
    method: "GET",
    signal: params.signal,
    skipErrorToast: true,
  })

  return {
    items: (raw.items ?? []).map(mapItem),
    total: raw.total ?? 0,
    limit: raw.limit ?? limit,
    offset: raw.offset ?? offset,
    hasMore: Boolean(raw.hasMore),
    nextOffset: raw.nextOffset ?? null,
    typeErrors: raw.typeErrors ?? {},
  }
}

/** Preflight: mark missing / wrong-type references before send. */
export async function resolveMentions(
  references: ChatReference[],
  signal?: AbortSignal,
): Promise<MentionResolveResult> {
  if (references.length === 0) {
    return { resolutions: [], ok: true }
  }
  const raw = await fetchAPI<{
    resolutions: MentionResolveItem[]
    ok: boolean
  }>("/mentions/resolve", {
    method: "POST",
    body: JSON.stringify({ references }),
    signal,
    skipErrorToast: true,
  })
  return {
    resolutions: raw.resolutions ?? [],
    ok: Boolean(raw.ok),
  }
}

/** Stable key for a reference chip / resolution. */
export function mentionKey(type: string, id: string): string {
  return `${type}:${id}`
}
