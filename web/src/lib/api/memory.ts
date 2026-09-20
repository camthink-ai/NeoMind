/** memory api — split from api.ts 2026-09. */
import type { MemoryEntry } from '@/types'
import { fetchAPI } from "./client"

export const memoryApi = {
  // ========== Memory API ==========
  getShortTermMemory: (limit?: number) =>
    fetchAPI<{ memories: Array<MemoryEntry> }>(
      `/memory/short-term${limit ? `?limit=${limit}` : ''}`
    ),
  addShortTermMemory: (content: string, importance?: number) =>
    fetchAPI<{ memory: MemoryEntry; message: string }>('/memory/short-term', {
      method: 'POST',
      body: JSON.stringify({ content, importance }),
    }),
  getMidTermMemory: (limit?: number) =>
    fetchAPI<{ memories: Array<MemoryEntry> }>(
      `/memory/mid-term${limit ? `?limit=${limit}` : ''}`
    ),
  searchMemory: (query: string, limit?: number) =>
    fetchAPI<{ memories: Array<MemoryEntry> }>(
      `/memory/search${limit ? `?limit=${limit}` : ''}`,
      {
        method: 'POST',
        body: JSON.stringify({ query }),
      }
    ),
  consolidateMemory: () =>
    fetchAPI<{ consolidated_count: number; message: string }>('/memory/consolidate', {
      method: 'POST',
    }),

}
