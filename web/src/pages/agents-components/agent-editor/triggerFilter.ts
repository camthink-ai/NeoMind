// A reactive agent fires on data changes, and the backend expresses those as an
// `event_filter` JSON string. Two groups exist (design 001 §5.2.2): `any` fires
// on the first matching source — what the editor has always written — and `all`
// fires only once every listed source has reported inside a window. The
// envelope lives here so the form, the load path and the validation share one
// definition instead of each re-deriving it from JSON.

export type TriggerMode = 'any' | 'all'

export interface TriggerSource {
  type: string
  id: string
  name: string
  field?: string
}

export interface TriggerFilter {
  mode: TriggerMode
  sources: TriggerSource[]
  /**
   * Aggregation window for an `all` group, in seconds. `null` means the stored
   * filter carries none — which the backend reads as "one single event must
   * satisfy every source", i.e. an agent that effectively never fires.
   */
  withinSecs: number | null
}

/** Why a filter cannot be saved. The caller renders it in the user's language. */
export type TriggerFilterIssue = 'window_required' | 'sources_required'

const EMPTY: TriggerFilter = { mode: 'any', sources: [], withinSecs: null }

export function parseTriggerFilter(raw: string | undefined | null): TriggerFilter {
  if (!raw) return EMPTY

  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return EMPTY
  }
  if (!parsed || typeof parsed !== 'object') return EMPTY

  const obj = parsed as Record<string, unknown>
  const asSources = (v: unknown): TriggerSource[] =>
    Array.isArray(v) ? (v as TriggerSource[]) : []

  const allSources = asSources(obj.all)
  if (allSources.length > 0) {
    const window = obj.within_secs
    return {
      mode: 'all',
      sources: allSources,
      withinSecs: typeof window === 'number' ? window : null,
    }
  }

  // `any` is the canonical name; `sources` is what the editor wrote before M2,
  // and what the backend still aliases to `any`.
  const anySources = asSources(obj.any)
  const sources = anySources.length > 0 ? anySources : asSources(obj.sources)
  if (sources.length === 0) return EMPTY

  return { mode: 'any', sources, withinSecs: null }
}

export function buildTriggerFilter(filter: TriggerFilter): string {
  if (filter.mode === 'all') {
    return JSON.stringify({
      all: filter.sources,
      // Omitted rather than written as null so the stored shape stays exactly
      // what the backend's parser recognises.
      ...(filter.withinSecs !== null ? { within_secs: filter.withinSecs } : {}),
    })
  }
  return JSON.stringify({ any: filter.sources })
}

/**
 * Why this filter must not be saved, or `null` when it is fine.
 *
 * Two shapes silently never fire, so neither may leave the editor:
 *   - an `all` group with no window — the sources would have to arrive in one
 *     single event;
 *   - an event agent with no sources at all *and* nothing bound — the backend
 *     has nothing to match on. Bound resources are a legitimate fallback, so
 *     the caller has to say whether there are any.
 */
export function reasonTriggerFilterInvalid(
  filter: TriggerFilter,
  context: { hasBoundResources: boolean },
): TriggerFilterIssue | null {
  if (filter.sources.length === 0 && !context.hasBoundResources) {
    return 'sources_required'
  }
  if (filter.mode === 'all' && filter.withinSecs === null) {
    return 'window_required'
  }
  return null
}
