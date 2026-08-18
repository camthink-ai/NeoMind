/// Smoke tests for ResponsiveTable (desktop) — guards the contracts that
/// broke historically: the w-full scrollable wrapper (table adapts to
/// window width) and the text-nano header tokens (the tailwind-merge
/// regression dropped sizes app-wide).
import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { ResponsiveTable } from '@/components/shared'
import { ThemeProvider } from '@/components/ui/theme'

vi.mock('@/hooks/useMobile', () => ({
  useIsMobile: () => false,
  useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
}))

interface Row { name: string; status: string }

const columns = [
  { key: 'name' as const, label: 'Name' },
  { key: 'status' as const, label: 'Status' },
]
const data: Row[] = [
  { name: 'sensor-1', status: 'online' },
  { name: 'sensor-2', status: 'offline' },
]

describe('ResponsiveTable (desktop)', () => {
  it('renders column headers and row cells', () => {
    render(
      <ThemeProvider>
        <ResponsiveTable
          columns={columns}
          data={data}
          renderCell={(key, row) => String((row as Record<string, unknown>)[key] ?? '')}
          rowKey={(row) => (row as Row).name}
        />
      </ThemeProvider>
    )
    expect(screen.getByText('Name')).toBeInTheDocument()
    expect(screen.getAllByText('Status').length).toBeGreaterThan(0)
    expect(screen.getAllByText('sensor-1').length).toBeGreaterThan(0)
    expect(screen.getAllByText('sensor-2').length).toBeGreaterThan(0)
  })

  it('wrapper is w-full + overflow-x-auto (adapts to window width)', () => {
    const { container } = render(
      <ThemeProvider>
        <ResponsiveTable
          columns={columns}
          data={data}
          renderCell={(key, row) => String((row as Record<string, unknown>)[key] ?? '')}
          rowKey={(row) => (row as Row).name}
        />
      </ThemeProvider>
    )
    const wrapper = container.querySelector('.overflow-x-auto')
    expect(wrapper).toBeTruthy()
    expect(wrapper!.className).toContain('w-full')
  })

  it('table itself is min-w-full', () => {
    const { container } = render(
      <ThemeProvider>
        <ResponsiveTable
          columns={columns}
          data={data}
          renderCell={(key, row) => String((row as Record<string, unknown>)[key] ?? '')}
          rowKey={(row) => (row as Row).name}
        />
      </ThemeProvider>
    )
    const table = container.querySelector('table')
    expect(table).toBeTruthy()
    expect(table!.className).toContain('min-w-full')
  })

  it('headers carry the text-nano token (10px ladder)', () => {
    const { container } = render(
      <ThemeProvider>
        <ResponsiveTable
          columns={columns}
          data={data}
          renderCell={(key, row) => String((row as Record<string, unknown>)[key] ?? '')}
          rowKey={(row) => (row as Row).name}
        />
      </ThemeProvider>
    )
    const header = screen.getByText('Name').closest('th')
    expect(header).toBeTruthy()
    expect(header!.className).toContain('text-nano')
  })
})
