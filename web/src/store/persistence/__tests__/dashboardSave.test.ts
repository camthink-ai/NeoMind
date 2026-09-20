/// Save-contract tests for the dashboard persistence layer — ADR 0008's
/// camelCase-domain ↔ snake_case-DTO seam. A local dashboard (UUID id)
/// must CREATE without its local id; a server dashboard (dashboard_*)
/// must UPDATE in place by id.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { ApiDashboardStorage } from '../implementations'

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

const localDashboard: Dashboard = {
  id: '550e8400-e29b-41d4-a716-446655440000', // UUID → not yet on the server
  name: '车间看板',
  layout,
  components: [],
  createdAt: 1,
  updatedAt: 2,
}

const serverDashboard: Dashboard = {
  ...localDashboard,
  id: 'dashboard_1758000000000',
}

const serverDto = {
  id: 'dashboard_1758000000000',
  name: '车间看板',
  description: null,
  layout,
  components: [],
  created_at: 1,
  updated_at: 2,
  is_default: false,
  sort_order: 0,
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('ApiDashboardStorage sync (DTO seam)', () => {
  it('creates local dashboards server-side without the local id', async () => {
    mocks.createDashboard.mockResolvedValue(serverDto)
    const storage = new ApiDashboardStorage()

    const result = await storage.sync(localDashboard)

    expect(mocks.createDashboard).toHaveBeenCalledTimes(1)
    const dto = mocks.createDashboard.mock.calls[0][0]
    expect(dto).not.toHaveProperty('id') // server assigns the id
    expect(dto.name).toBe('车间看板')
    expect(result.error).toBeNull()
    expect(result.data?.id).toBe('dashboard_1758000000000')
  })

  it('updates server dashboards in place by id', async () => {
    mocks.getDashboard.mockResolvedValue(serverDto) // exists → update path
    mocks.updateDashboard.mockResolvedValue(serverDto)
    const storage = new ApiDashboardStorage()

    const result = await storage.sync(serverDashboard)

    expect(mocks.getDashboard).toHaveBeenCalledWith('dashboard_1758000000000')
    expect(mocks.updateDashboard).toHaveBeenCalledTimes(1)
    const [id, dto] = mocks.updateDashboard.mock.calls[0]
    expect(id).toBe('dashboard_1758000000000')
    expect(dto.name).toBe('车间看板')
    expect(result.error).toBeNull()
  })
})
