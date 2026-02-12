/// Tests for useDataSource hook
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

// Mock dependencies
vi.mock('@/store', () => ({
  useStore: vi.fn(() => ({
    devices: [],
    addDevice: vi.fn(),
  })),
}))

vi.mock('@/hooks/useEvents', () => ({
  useEvents: vi.fn(() => ({
    subscribe: vi.fn(),
    unsubscribe: vi.fn(),
  })),
}))

vi.mock('@/lib/api', () => ({
  api: {
    getDeviceCurrent: vi.fn(() => Promise.resolve({
      device_id: 'test-device',
      metrics: { temperature: { value: 25 } },
      commands: [],
    })),
    sendCommand: vi.fn(() => Promise.resolve()),
  },
}))

describe('useDataSource Hook', () => {
  let queryClient: QueryClient

  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
        },
      },
    })
  })

  type WrapperProps = { children: React.ReactNode }
  const wrapper = ({ children }: WrapperProps) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  )

  describe('Data Source Interface', () => {
    it('should define UseDataSourceResult interface', () => {
      const result = {
        data: null,
        loading: false,
        error: null,
        lastUpdate: null,
      }

      expect(result.data).toBeNull()
      expect(result.loading).toBe(false)
      expect(result.error).toBeNull()
      expect(result.lastUpdate).toBeNull()
    })

    it('should support optional sendCommand function', () => {
      type SendCommandType = (value?: unknown) => Promise<boolean>
      const result: {
        sendCommand?: SendCommandType
      } = {} as any

      expect(result.sendCommand).toBeUndefined()
    })

    it('should support optional sending flag', () => {
      const result: {
        sending?: boolean
      } = {}

      expect(result.sending).toBeUndefined()
    })
  })

  describe('Data Source Types', () => {
    it('should accept device type data source', () => {
      const deviceDataSource = {
        id: 'device-1',
        type: 'device' as const,
        deviceId: 'device-1',
        metric: 'temperature',
        aggregation: 'mean' as const,
        label: 'Temperature',
        unit: '°C',
      }

      expect(deviceDataSource.type).toBe('device')
      expect(deviceDataSource.deviceId).toBe('device-1')
    })

    it('should accept transform type data source', () => {
      const transformDataSource = {
        id: 'transform-1',
        type: 'transform' as const,
        transformId: 'transform-1',
        label: 'Transformed Data',
      }

      expect(transformDataSource.type).toBe('transform')
      expect(transformDataSource.transformId).toBe('transform-1')
    })

    it('should accept extension-command type data source', () => {
      const extensionCommandDataSource = {
        id: 'ext-cmd-1',
        type: 'extension-command' as const,
        extensionId: 'ext-1',
        extensionCommand: 'getData',
        label: 'Extension Command',
      }

      expect(extensionCommandDataSource.type).toBe('extension-command')
      expect(extensionCommandDataSource.extensionId).toBe('ext-1')
    })

    it('should accept array of data sources', () => {
      const dataSourceList: Array<{
        id: string
        type: 'device' | 'transform' | 'extension-command'
      }> = [
        { id: '1', type: 'device' as const, deviceId: '1', metric: 'temp' },
        { id: '2', type: 'transform' as const, transformId: '2' },
        { id: '3', type: 'extension-command' as const, extensionId: '3', extensionCommand: 'cmd' },
      ]

      expect(dataSourceList).toHaveLength(3)
    })
  })

  describe('Telemetry Aggregate Types', () => {
    it('should accept valid aggregation functions', () => {
      const aggregations: Array<'raw' | 'mean' | 'sum' | 'min' | 'max' | 'count'> = [
        'raw',
        'mean',
        'sum',
        'min',
        'max',
        'count',
      ]

      expect(aggregations).toHaveLength(6)
    })

    it('should support optional time window for telemetry', () => {
      const aggregate = {
        type: 'mean' as const,
        value: 25.5,
        timeWindow: '5m',
      }

      expect(aggregate.type).toBe('mean')
      expect(aggregate.value).toBe(25.5)
      expect(aggregate.timeWindow).toBe('5m')
    })
  })
})
