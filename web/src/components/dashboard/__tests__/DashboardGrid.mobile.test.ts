/**
 * Tests for the mobile (xs) layout derivation in DashboardGrid.
 *
 * Mobile uses a single full-width column (masonry-style, HA-pattern). Every
 * card becomes w=1 and stacks vertically; authored height is preserved but
 * floored by a per-type minimum so charts and content cards keep enough
 * vertical room.
 */
import { describe, it, expect } from 'vitest'
import { buildMobileLayout, MOBILE_COLS } from '../DashboardGrid.layout'

const mk = (id: string, type: string, w = 4, h = 2) => ({
  id,
  type,
  position: { w, h },
})

describe('buildMobileLayout', () => {
  // ── Width: always full row ─────────────────────────────────────────────
  it('forces every card to the mobile column count (full row)', () => {
    const out = buildMobileLayout([
      mk('a', 'value-card', 2, 1),    // small card
      mk('b', 'line-chart', 6, 4),    // large card
      mk('c', 'map-display', 8, 6),   // very wide card
    ])
    expect(out.every((i) => i.w === MOBILE_COLS)).toBe(true)
    expect(MOBILE_COLS).toBe(1)
  })

  it('classifies extension / unknown types as full-row too', () => {
    const out = buildMobileLayout([mk('ext', 'totally-custom-widget', 7, 3)])
    expect(out[0].w).toBe(MOBILE_COLS)
  })

  // ── Height floor ──────────────────────────────────────────────────────
  it('bumps a short value-card up to the per-type mobile min (h=1 → h=2)', () => {
    // h=1 fits at desktop ~190px wide but squishes at full-width mobile when
    // the card is in a stacked column. Floor raises it.
    const out = buildMobileLayout([mk('v1', 'value-card', 2, 1)])
    expect(out[0].h).toBe(2)
  })

  it('does not shrink a tall card below its authored height', () => {
    const out = buildMobileLayout([mk('v1', 'value-card', 2, 4)])
    expect(out[0].h).toBe(4)
  })

  it('applies a taller floor to charts so axes have room', () => {
    const out = buildMobileLayout([mk('c1', 'line-chart', 6, 2)])
    expect(out[0].h).toBe(3)
  })

  it('honors markdown-display taller floor for text content', () => {
    const out = buildMobileLayout([mk('m1', 'markdown-display', 4, 1)])
    expect(out[0].h).toBeGreaterThanOrEqual(3)
  })

  it('floors height at 1 for unknown types with missing position', () => {
    const out = buildMobileLayout([{ id: 'x', type: 'mystery-ext', position: {} }])
    expect(out[0].h).toBeGreaterThanOrEqual(1)
  })

  // ── Packing invariants ────────────────────────────────────────────────
  it('returns x/y = 0 for every item so RGL vertical compactor stacks them', () => {
    const out = buildMobileLayout([
      mk('a', 'value-card', 2, 2),
      mk('b', 'line-chart', 6, 4),
      mk('c', 'map-display', 6, 4),
    ])
    for (const item of out) {
      expect(item.x).toBe(0)
      expect(item.y).toBe(0)
    }
  })

  it('preserves every item id in source order', () => {
    const src = [
      mk('temp', 'value-card', 2, 2),
      mk('chart', 'line-chart', 6, 4),
      mk('led', 'led-indicator', 2, 2),
      mk('map', 'map-display', 6, 4),
    ]
    const out = buildMobileLayout(src)
    expect(out.map((i) => i.i)).toEqual(['temp', 'chart', 'led', 'map'])
  })
})
