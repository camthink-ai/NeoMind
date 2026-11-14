/**
 * Deterministic guard used by chat.tsx Enter→send and ChatComposer mention
 * selection: IME confirmation must never dispatch the action.
 */
import { describe, expect, it } from 'vitest'

function shouldSuppressEnterForIme(e: {
  isComposing?: boolean
  keyCode?: number
}): boolean {
  return Boolean(e.isComposing) || e.keyCode === 229
}

describe('IME Enter send/select guard', () => {
  it('suppresses composing Enter', () => {
    expect(shouldSuppressEnterForIme({ isComposing: true, keyCode: 13 })).toBe(true)
  })

  it('suppresses keyCode 229 (IME processing)', () => {
    expect(shouldSuppressEnterForIme({ isComposing: false, keyCode: 229 })).toBe(true)
  })

  it('allows a normal Enter', () => {
    expect(shouldSuppressEnterForIme({ isComposing: false, keyCode: 13 })).toBe(false)
  })
})
