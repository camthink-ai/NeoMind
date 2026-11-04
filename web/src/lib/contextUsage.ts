export interface ContextTokenUsage {
  promptTokens: number
  systemPromptTokens?: number
  toolTokens?: number
  maxContextTokens?: number
}

const positive = (value?: number) => value != null && Number.isFinite(value) && value > 0

/** A context window is one request, not the sum of a ReAct transcript. */
export function contextUsageSnapshot(measured: ContextTokenUsage | null | undefined,
  configuredMax: number | undefined, messageCount: number) {
  const max = positive(measured?.maxContextTokens) ? measured!.maxContextTokens!
    : positive(configuredMax) ? configuredMax! : 8192
  const known = measured != null && Number.isFinite(measured.promptTokens) && measured.promptTokens >= 0
  // The browser does not know which stored messages the server compacted.
  const pending = !known
  const used = known ? measured!.promptTokens : 0
  const rawSystem = known ? measured?.systemPromptTokens : undefined
  const rawTools = known ? measured?.toolTokens : undefined
  const validPart = (value?: number) => value == null || (Number.isFinite(value) && value >= 0 && value <= used)
  const consistent = validPart(rawSystem) && validPart(rawTools)
    && (rawSystem == null || rawTools == null || rawSystem + rawTools <= used)
  // These are estimates from prompt templates, not provider-measured parts.
  // Do not show an impossible decomposition of a measured total.
  const system = consistent ? rawSystem : undefined
  const tools = consistent ? rawTools : undefined
  const history = system != null && tools != null ? Math.max(0, used - system - tools) : undefined
  return { used, max, system, tools, history, estimated: !known, pending, messageCount: known ? undefined : messageCount }
}
