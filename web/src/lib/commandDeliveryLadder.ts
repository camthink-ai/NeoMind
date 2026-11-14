/**
 * Derive an honest command-delivery ladder for the UI (1.3 iter D).
 *
 * Backend `CommandDelivery` proves only what the send path observed.
 * Adapter and extension acceptance are alternate dispatch proofs — neither
 * is a device ACK, and neither may paint ordinary green "complete".
 * Upper tiers stay `not_reached` until evidence exists; missing payload →
 * all `unknown`. Never infer tiers from `sent: true` or history status alone.
 */

/** UI / plan tier ids (stable for i18n + data attributes). */
export type CommandDeliveryTier =
  | 'adapter_received'
  | 'extension_received'
  | 'device_ack'
  | 'physical_effect'

export type TierState = 'reached' | 'not_reached' | 'unknown' | 'not_applicable'

/** Wire shape from POST /devices/:id/command/:cmd (`CommandDelivery` serde). */
export type CommandDeliveryJson =
  | 'adapter_accepted'
  | 'extension_accepted'
  | 'device_acknowledged'
  | 'physical_effect_verified'
  | { adapter_accepted: { adapters: number } }
  | { extension_accepted: null | Record<string, never> }
  | { device_acknowledged: null | Record<string, never> }
  | { physical_effect_verified: null | Record<string, never> }

export interface SendCommandDeliveryResult {
  sent: boolean
  delivery?: CommandDeliveryJson | null
  delivery_note?: string | null
}

export type ReachedKind =
  | 'adapter_accepted'
  | 'extension_accepted'
  | 'device_acknowledged'
  | 'physical_effect_verified'
  | null

export interface CommandDeliveryLadder {
  tiers: Record<CommandDeliveryTier, TierState>
  /** Which delivery variant was observed (null if missing/unparseable). */
  reachedKind: ReachedKind
  adapters?: number
  deliveryNote: string | null
  /**
   * True only when physical effect is verified — the only green "complete"
   * allowed by plan honesty. Dispatch-only tiers must not use this.
   */
  allowVerifiedSuccess: boolean
  /** True when the HTTP send succeeded (`sent`) regardless of tier. */
  dispatched: boolean
}

const TIER_ORDER: CommandDeliveryTier[] = [
  'adapter_received',
  'extension_received',
  'device_ack',
  'physical_effect',
]

export function parseReachedKind(delivery: unknown): {
  kind: ReachedKind
  adapters?: number
} {
  if (delivery == null) return { kind: null }

  if (typeof delivery === 'string') {
    switch (delivery) {
      case 'adapter_accepted':
        return { kind: 'adapter_accepted' }
      case 'extension_accepted':
        return { kind: 'extension_accepted' }
      case 'device_acknowledged':
        return { kind: 'device_acknowledged' }
      case 'physical_effect_verified':
        return { kind: 'physical_effect_verified' }
      default:
        return { kind: null }
    }
  }

  if (typeof delivery === 'object' && !Array.isArray(delivery)) {
    const o = delivery as Record<string, unknown>
    if ('adapter_accepted' in o) {
      const inner = o.adapter_accepted as { adapters?: number } | null
      const adapters =
        inner && typeof inner.adapters === 'number' ? inner.adapters : undefined
      return { kind: 'adapter_accepted', adapters }
    }
    if ('extension_accepted' in o) return { kind: 'extension_accepted' }
    if ('device_acknowledged' in o) return { kind: 'device_acknowledged' }
    if ('physical_effect_verified' in o) {
      return { kind: 'physical_effect_verified' }
    }
  }

  return { kind: null }
}

function emptyUnknown(): Record<CommandDeliveryTier, TierState> {
  return {
    adapter_received: 'unknown',
    extension_received: 'unknown',
    device_ack: 'unknown',
    physical_effect: 'unknown',
  }
}

function allNotReached(): Record<CommandDeliveryTier, TierState> {
  return {
    adapter_received: 'not_reached',
    extension_received: 'not_reached',
    device_ack: 'not_reached',
    physical_effect: 'not_reached',
  }
}

/**
 * Map a send-command response into ladder states.
 * Pass the raw `delivery` field (or full result); `sent: false` / missing
 * delivery → unknown (never invent higher tiers).
 */
export function deriveCommandDeliveryLadder(
  input: SendCommandDeliveryResult | CommandDeliveryJson | null | undefined,
): CommandDeliveryLadder {
  if (input == null) {
    return {
      tiers: emptyUnknown(),
      reachedKind: null,
      deliveryNote: null,
      allowVerifiedSuccess: false,
      dispatched: false,
    }
  }

  // Full API result
  if (typeof input === 'object' && 'sent' in input) {
    const result = input as SendCommandDeliveryResult
    if (!result.sent) {
      return {
        tiers: emptyUnknown(),
        reachedKind: null,
        deliveryNote: result.delivery_note ?? null,
        allowVerifiedSuccess: false,
        dispatched: false,
      }
    }
    const parsed = parseReachedKind(result.delivery)
    return buildFromKind(parsed.kind, parsed.adapters, result.delivery_note ?? null, true)
  }

  // Bare delivery payload
  const parsed = parseReachedKind(input)
  return buildFromKind(parsed.kind, parsed.adapters, null, true)
}

function buildFromKind(
  kind: ReachedKind,
  adapters: number | undefined,
  deliveryNote: string | null,
  dispatched: boolean,
): CommandDeliveryLadder {
  if (!kind) {
    return {
      tiers: emptyUnknown(),
      reachedKind: null,
      adapters,
      deliveryNote,
      allowVerifiedSuccess: false,
      dispatched,
    }
  }

  const tiers = allNotReached()

  switch (kind) {
    case 'adapter_accepted':
      tiers.adapter_received = 'reached'
      // Alternate dispatch path — not a failure of extension, just N/A
      tiers.extension_received = 'not_applicable'
      break
    case 'extension_accepted':
      tiers.extension_received = 'reached'
      tiers.adapter_received = 'not_applicable'
      break
    case 'device_acknowledged':
      // Dispatch path unknown when only device ACK is reported
      tiers.adapter_received = 'unknown'
      tiers.extension_received = 'unknown'
      tiers.device_ack = 'reached'
      break
    case 'physical_effect_verified':
      tiers.adapter_received = 'unknown'
      tiers.extension_received = 'unknown'
      tiers.device_ack = 'unknown'
      tiers.physical_effect = 'reached'
      break
  }

  return {
    tiers,
    reachedKind: kind,
    adapters,
    deliveryNote,
    allowVerifiedSuccess: kind === 'physical_effect_verified',
    dispatched,
  }
}

export { TIER_ORDER }
