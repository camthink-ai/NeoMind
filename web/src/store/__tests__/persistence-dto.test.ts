/// Tests for dashboard persistence DTO conversion
import { describe, it, expect } from 'vitest'
import { toDashboardDTO } from '../persistence/types'
import type { Dashboard } from '@/types/dashboard'

function makeDashboard(components: Dashboard['components']): Dashboard {
  return {
    id: 'dashboard_test',
    name: 'Test',
    layout: { columns: 12, rows: 'auto' },
    components,
    createdAt: 1700000000000,
    updatedAt: 1700000000001,
  } as Dashboard
}

const baseComponent = {
  id: 'comp-1',
  type: 'value-card',
  position: { x: 0, y: 0, w: 4, h: 3 },
  title: 'Test Card',
}

describe('toDashboardDTO', () => {
  it('strips the runtime _saveTs stamp from a single data_source', () => {
    const dash = makeDashboard([
      {
        ...baseComponent,
        dataSource: { source: 'device', id: 'dev-1', field: 'temperature', mode: 'latest', _saveTs: 1700000000002 },
      } as unknown as Dashboard['components'][number],
    ])
    const dto = toDashboardDTO(dash)
    const ds = dto.components[0].data_source as Record<string, unknown>
    expect(ds).toBeDefined()
    expect(ds).not.toHaveProperty('_saveTs')
    expect(ds.source).toBe('device')
    expect(ds.field).toBe('temperature')
  })

  it('strips _saveTs from every entry of a data_source list', () => {
    const dash = makeDashboard([
      {
        ...baseComponent,
        dataSource: [
          { source: 'device', id: 'dev-1', field: 'temperature', mode: 'latest', _saveTs: 1 },
          { source: 'device', id: 'dev-1', field: 'humidity', mode: 'latest' },
        ],
      } as unknown as Dashboard['components'][number],
    ])
    const dto = toDashboardDTO(dash)
    const list = dto.components[0].data_source as Record<string, unknown>[]
    expect(list).toHaveLength(2)
    for (const ds of list) {
      expect(ds).not.toHaveProperty('_saveTs')
    }
    expect(list[1].field).toBe('humidity')
  })

  it('passes through a data_source without runtime fields unchanged', () => {
    const raw = { source: 'system', id: 'neomind', field: 'free_memory', mode: 'latest' }
    const dash = makeDashboard([{ ...baseComponent, dataSource: { ...raw } } as Dashboard['components'][number]])
    const dto = toDashboardDTO(dash)
    expect(dto.components[0].data_source).toEqual(raw)
  })

  it('keeps data_source absent when the component has none', () => {
    const dash = makeDashboard([{ ...baseComponent } as Dashboard['components'][number]])
    const dto = toDashboardDTO(dash)
    expect(dto.components[0].data_source).toBeUndefined()
  })
})
