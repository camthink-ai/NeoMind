/// Tests for chat message utilities — tool-call JSON stripping must remove
/// embedded CALLS without eating DATA the model quoted for the user.
import { describe, it, expect } from 'vitest'
import { cleanToolCallJson, mergeMessagesForDisplay } from '../messageUtils'
import type { Message } from '@/types'

describe('cleanToolCallJson', () => {
  it('strips fenced tool-call arrays (name + arguments)', () => {
    const content = '调用如下：\n```json\n[{"name":"shell","arguments":{"command":"ls"}}]\n```\n完成。'
    const cleaned = cleanToolCallJson(content)
    expect(cleaned).not.toContain('shell')
    expect(cleaned).toContain('完成。')
  })

  it('strips bare tool-call arrays', () => {
    const content = '好的[{"name":"echo","arguments":{"msg":"hi"}}]'
    expect(cleanToolCallJson(content)).not.toContain('echo')
  })

  it('strips bare tool-call objects (name + arguments)', () => {
    const content = '{"name":"list_devices","arguments":{"type":"sensor"}}'
    expect(cleanToolCallJson(content).trim()).toBe('')
  })

  // Regression: a fenced DATA block (device list) has "name" fields but no
  // "arguments". The old name-only pattern deleted the whole block from the
  // reply — the frontend mirror of the backend cleanup bug.
  it('preserves fenced data arrays that merely contain name fields', () => {
    const content =
      '设备列表：\n```json\n[{"id":"demo-001","name":"Demo","online":true}]\n```\n以上共 1 台。'
    const cleaned = cleanToolCallJson(content)
    expect(cleaned).toContain('demo-001')
    expect(cleaned).toContain('Demo')
    expect(cleaned).toContain('以上共 1 台。')
  })

  it('preserves data objects without arguments', () => {
    const content = '推送目标 {"name":"obs-target","url":"https://example.com/hook"} 尚未收到数据。'
    const cleaned = cleanToolCallJson(content)
    expect(cleaned).toContain('obs-target')
    expect(cleaned).toContain('尚未收到数据。')
  })

  it('leaves plain prose untouched', () => {
    const content = '设备在线，温度 23.5°C。'
    expect(cleanToolCallJson(content)).toBe(content)
  })
})

describe('mergeMessagesForDisplay contentNodes', () => {
  it('keeps and normalizes user contentNodes for history chip redraw', () => {
    const raw = [
      {
        id: 'u1',
        role: 'user',
        content: '检查设备',
        timestamp: 1_700_000_000, // seconds — also exercises ms normalize
        content_nodes: [
          { kind: 'text', text: '检查 ' },
          { kind: 'reference', type: 'device', id: 's1', label_snapshot: '传感器' },
        ],
      },
    ] as unknown as Message[]
    const merged = mergeMessagesForDisplay(raw)
    expect(merged).toHaveLength(1)
    expect(merged[0].contentNodes).toEqual([
      { kind: 'text', text: '检查 ' },
      { kind: 'reference', type: 'device', id: 's1', labelSnapshot: '传感器' },
    ])
    expect(merged[0].timestamp).toBe(1_700_000_000_000)
  })
})
