import { describe, expect, it } from 'vitest'
import { getSessionDisplayTitle } from '../sessionTitle'
import type { ChatSession } from '@/types'

function session(partial: Partial<ChatSession>): ChatSession {
  return {
    sessionId: 's1',
    id: 's1',
    createdAt: 0,
    ...partial,
  }
}

describe('getSessionDisplayTitle', () => {
  it('prefers a real title over preview', () => {
    expect(getSessionDisplayTitle(
      session({ title: 'Device check', preview: 'hello' }),
      'New Chat',
    )).toBe('Device check')
  })

  it('ignores placeholder titles and falls back to preview', () => {
    expect(getSessionDisplayTitle(
      session({ title: '新对话', preview: '帮我看看设备' }),
      '新对话',
    )).toBe('帮我看看设备')
    expect(getSessionDisplayTitle(
      session({ title: 'New Chat', preview: 'check devices' }),
      'New Chat',
    )).toBe('check devices')
  })

  it('truncates long previews', () => {
    const preview = 'a'.repeat(40)
    expect(getSessionDisplayTitle(session({ preview }), 'New Chat')).toBe(`${'a'.repeat(30)}…`)
  })

  it('uses fallback when empty', () => {
    expect(getSessionDisplayTitle(undefined, 'New Chat')).toBe('New Chat')
    expect(getSessionDisplayTitle(session({}), 'New Chat')).toBe('New Chat')
  })
})
