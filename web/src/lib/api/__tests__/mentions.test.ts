import { beforeEach, describe, expect, it, vi } from 'vitest'

const fetchAPI = vi.fn()
vi.mock('../client', () => ({
  fetchAPI: (...args: unknown[]) => fetchAPI(...args),
}))

import {
  mentionKey,
  resolveMentions,
  searchMentionOptions,
  searchMentions,
} from '../mentions'

describe('mentions API client', () => {
  beforeEach(() => {
    fetchAPI.mockReset()
  })

  it('searchMentions builds query and maps camelCase fields', async () => {
    fetchAPI.mockResolvedValueOnce({
      items: [
        {
          type: 'device',
          id: 'dev-a',
          label: '温湿度传感器',
          shortId: 'dev-a',
          hint: 'temp · mqtt',
          duplicateName: true,
        },
      ],
      total: 1,
      limit: 20,
      offset: 0,
      hasMore: false,
      nextOffset: null,
      typeErrors: { rule: 'boom' },
    })

    const result = await searchMentions({
      query: '温',
      types: ['device'],
      limit: 10,
      offset: 0,
    })

    expect(fetchAPI).toHaveBeenCalledTimes(1)
    const [path, opts] = fetchAPI.mock.calls[0]
    expect(path).toContain('/mentions/search?')
    expect(path).toContain('q=%E6%B8%A9')
    expect(path).toContain('types=device')
    expect(path).toContain('limit=10')
    expect(opts.method).toBe('GET')
    expect(result.items[0]).toMatchObject({
      type: 'device',
      id: 'dev-a',
      duplicateName: true,
      shortId: 'dev-a',
    })
    expect(result.typeErrors.rule).toBe('boom')
  })

  it('searchMentionOptions returns items only', async () => {
    fetchAPI.mockResolvedValueOnce({
      items: [{ type: 'agent', id: 'a1', label: '巡检' }],
      total: 1,
      limit: 20,
      offset: 0,
      hasMore: false,
      nextOffset: null,
      typeErrors: {},
    })
    const items = await searchMentionOptions('巡')
    expect(items).toEqual([
      expect.objectContaining({ type: 'agent', id: 'a1', label: '巡检' }),
    ])
  })

  it('resolveMentions posts references and returns ok=false on misses', async () => {
    fetchAPI.mockResolvedValueOnce({
      ok: false,
      resolutions: [
        { index: 0, type: 'device', id: 'dev-a', ok: true, detail: 'ok' },
        { index: 1, type: 'device', id: 'ghost', ok: false, detail: 'no device' },
      ],
    })
    const result = await resolveMentions([
      { type: 'device', id: 'dev-a', label: 'A' },
      { type: 'device', id: 'ghost', label: 'G' },
    ])
    expect(fetchAPI).toHaveBeenCalledWith(
      '/mentions/resolve',
      expect.objectContaining({ method: 'POST' }),
    )
    expect(result.ok).toBe(false)
    expect(result.resolutions[1].ok).toBe(false)
  })

  it('resolveMentions short-circuits empty list', async () => {
    const result = await resolveMentions([])
    expect(fetchAPI).not.toHaveBeenCalled()
    expect(result).toEqual({ resolutions: [], ok: true })
  })

  it('mentionKey is stable', () => {
    expect(mentionKey('device', 'x')).toBe('device:x')
  })

  it('passes AbortSignal through to fetchAPI', async () => {
    const ac = new AbortController()
    fetchAPI.mockResolvedValueOnce({
      items: [],
      total: 0,
      limit: 20,
      offset: 0,
      hasMore: false,
      nextOffset: null,
      typeErrors: {},
    })
    await searchMentions({ query: 'a', signal: ac.signal })
    expect(fetchAPI.mock.calls[0][1].signal).toBe(ac.signal)
  })
})
