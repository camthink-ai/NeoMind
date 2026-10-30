import { describe, expect, it } from 'vitest'
import { isToolError } from '../toolCallUtils'
import type { ToolCall } from '@/types'

const shell = (result: unknown, success?: boolean): ToolCall => ({
  id: 'call1', name: 'shell', arguments: { command: 'example' }, result, success,
})

describe('tool execution status across live events and saved history', () => {
  it.each([undefined, true, false])('reports nonzero shell exits with live success=%s', success => {
    expect(isToolError(shell({ exit_code: 7, stdout: 'partial output' }, success))).toBe(true)
    expect(isToolError(shell(JSON.stringify({ exit_code: 7, stdout: 'partial output' }), success))).toBe(true)
  })
  it('reports timeout and signal termination from saved structured results', () => {
    expect(isToolError(shell({ timed_out: true, exit_code: null }))).toBe(true)
    expect(isToolError(shell({ timed_out: false, exit_code: null }))).toBe(true)
  })
  it('does not infer failure from diagnostic words or business error fields', () => {
    expect(isToolError(shell({ exit_code: 0, stdout: 'command not found in documentation' }))).toBe(false)
    for (const result of [{ error: null }, { error: 0 }, { id: 'device1', error: 'sensor fault' }]) {
      expect(isToolError({ id: 'x', name: 'query', arguments: {}, result })).toBe(false)
    }
  })
  it('honors an explicit failed event and preserves old error envelopes', () => {
    expect(isToolError(shell({ exit_code: 0 }, false))).toBe(true)
    expect(isToolError({ id: 'x', name: 'query', arguments: {}, result: { error: 'execution failed' } })).toBe(true)
    expect(isToolError({ id: 'x', name: 'query', arguments: {}, result: { success: false, error: 'timeout', data: 'partial' } })).toBe(true)
  })
  it('uses an explicit successful event instead of interpreting a business payload', () => {
    expect(isToolError({ id: 'x', name: 'query', arguments: {}, success: true, result: { error: 'sensor fault' } })).toBe(false)
  })
})
