/// Regression tests (2026-11-16): the "dead server-born dashboard" bug.
///
/// A dashboard with a SERVER-issued id (dashboard_<uuid>) whose record no
/// longer exists server-side (deleted from another client / recreated data
/// dir) used to be resurrected by the load-merge as "localOnly" forever.
/// Every edit then PUT the dead id (the 404 the UI surfaced:
/// "Dashboard 'dashboard_…' not found (by id or name)") and every background
/// sync cloned a duplicate server-side — the recreate result carried a fresh
/// id that the sync layer discarded.
///
/// Contract after the fix:
///   1. load() drops server-format ids absent from a successful server list.
///   2. sync() on an absent server id re-creates WITHOUT the dead id in the
///      DTO (no update-PUT against the dead id).
///   3. HybridStorage adopts the re-issued id into the local cache (dead id
///      removed, server copy saved).
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ApiDashboardStorage, HybridDashboardStorage } from '../implementations'

const mocks = vi.hoisted(() => ({
  createDashboard: vi.fn(),
  updateDashboard: vi.fn(),
  getDashboard: vi.fn(),
  getDashboards: vi.fn(),
}))

vi.mock('@/lib/api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api')>()),
  api: { ...mocks },
}))

import type { Dashboard } from '@/types/dashboard'

const layout = {
  columns: 12,
  rows: 'auto' as const,
  breakpoints: { lg: 12, md: 10, sm: 6, xs: 4 },
}

const DEAD_ID = 'dashboard_a46195d5-2d1c-4b26-9630-c1dbc6785646'
const NEW_ID = 'dashboard_9f1c2b3d-1111-2222-3333-444455556666'

const deadDashboard: Dashboard = {
  id: DEAD_ID,
  name: '社区组件看板',
  layout,
  components: [],
  createdAt: 1,
  updatedAt: Date.now(),
}

const newServerDto = {
  id: NEW_ID,
  name: '社区组件看板',
  description: null,
  layout,
  components: [],
  created_at: 2,
  updated_at: 3,
  is_default: false,
  sort_order: 0,
}

const otherServerDto = {
  id: 'dashboard_1758000000000',
  name: '另一块板',
  description: null,
  layout,
  components: [],
  created_at: 1,
  updated_at: 2,
  is_default: false,
  sort_order: 0,
}

function readCache(): Dashboard[] {
  const raw = localStorage.getItem('neomind_dashboards')
  return raw ? (JSON.parse(raw) as Dashboard[]) : []
}

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  localStorage.removeItem('neomind_local_to_server_id')
})

describe('dead server-born dashboard recovery', () => {
  it('load() drops a server-format dashboard the server no longer has', async () => {
    localStorage.setItem('neomind_dashboards', JSON.stringify([deadDashboard]))
    mocks.getDashboards.mockResolvedValue({ dashboards: [otherServerDto], count: 1 })

    const storage = new HybridDashboardStorage({ cacheEnabled: false })
    const result = await storage.load()

    const ids = (result.data ?? []).map((d: Dashboard) => d.id)
    expect(
      ids.includes(DEAD_ID),
      `dead dashboard must NOT survive a successful server load, got: ${JSON.stringify(ids)}`,
    ).toBe(false)
    expect(ids).toContain('dashboard_1758000000000')
  })

  it('load() keeps genuinely local dashboards (no dashboard_ prefix) after the drop fix', async () => {
    const localOnly: Dashboard = { ...deadDashboard, id: '550e8400-e29b-41d4-a716-446655440000' }
    localStorage.setItem('neomind_dashboards', JSON.stringify([localOnly]))
    mocks.getDashboards.mockResolvedValue({ dashboards: [otherServerDto], count: 1 })

    const storage = new HybridDashboardStorage({ cacheEnabled: false })
    const result = await storage.load()

    const ids = (result.data ?? []).map((d: Dashboard) => d.id)
    expect(ids).toContain('550e8400-e29b-41d4-a716-446655440000')
  })

  it('sync() re-creates an absent server dashboard without the dead id (no PUT)', async () => {
    mocks.getDashboard.mockRejectedValue(new Error("Dashboard 'x' not found (by id or name)"))
    mocks.createDashboard.mockResolvedValue(newServerDto)
    const storage = new ApiDashboardStorage()

    const result = await storage.sync(deadDashboard)

    expect(mocks.updateDashboard).not.toHaveBeenCalled()
    expect(mocks.createDashboard).toHaveBeenCalledTimes(1)
    const dto = mocks.createDashboard.mock.calls[0][0]
    expect(dto).not.toHaveProperty('id', DEAD_ID)
    expect(result.data?.id).toBe(NEW_ID)
  })

  it('HybridStorage sync adopts the re-issued id into the cache (dead id removed)', async () => {
    localStorage.setItem('neomind_dashboards', JSON.stringify([deadDashboard]))
    mocks.getDashboard.mockRejectedValue(new Error('not found'))
    mocks.createDashboard.mockResolvedValue(newServerDto)
    const storage = new HybridDashboardStorage({ cacheEnabled: false })

    await storage.sync(deadDashboard)

    const cachedIds = readCache().map((d) => d.id)
    expect(cachedIds, `dead id must leave the cache, got: ${JSON.stringify(cachedIds)}`).not.toContain(DEAD_ID)
    expect(cachedIds).toContain(NEW_ID)
  })
})
