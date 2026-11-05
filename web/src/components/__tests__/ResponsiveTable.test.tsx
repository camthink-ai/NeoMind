/// Smoke tests for ResponsiveTable (desktop) — guards the contracts that
/// broke historically: the w-full scrollable wrapper (table adapts to
/// window width) and the shared header tokens (the tailwind-merge
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
          renderCell={(key, row) => String((row as unknown as Record<string, unknown>)[key] ?? '')}
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
          renderCell={(key, row) => String((row as unknown as Record<string, unknown>)[key] ?? '')}
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
          renderCell={(key, row) => String((row as unknown as Record<string, unknown>)[key] ?? '')}
          rowKey={(row) => (row as Row).name}
        />
      </ThemeProvider>
    )
    const table = container.querySelector('table')
    expect(table).toBeTruthy()
    expect(table!.className).toContain('min-w-full')
  })

  it('headers use readable labels', () => {
    const { container: _container } = render(
      <ThemeProvider>
        <ResponsiveTable
          columns={columns}
          data={data}
          renderCell={(key, row) => String((row as unknown as Record<string, unknown>)[key] ?? '')}
          rowKey={(row) => (row as Row).name}
        />
      </ThemeProvider>
    )
    const header = screen.getByText('Name').closest('th')
    expect(header).toBeTruthy()
    // Upgraded from the old 10px text-nano ladder for readability
    expect(header!.className).toContain('text-xs')
    expect(header!.className).not.toContain('text-nano')
  })
  it('applies CSS widths in loaded and loading headers while preserving class widths', () => {
    const props = {
      columns: [{ key: 'name', label: 'Name', width: '28%' }, { key: 'status', label: 'Status', width: 'w-24' }],
      data,
      rowKey: (row: Row) => row.name,
      renderCell: (key: string, row: Row) => row[key as keyof Row],
    }
    const { container, rerender } = render(<ResponsiveTable {...props} />)
    for (const loading of [false, true]) {
      rerender(<ResponsiveTable {...props} loading={loading} />)
      const headers = container.querySelectorAll('thead th')
      expect((headers[0] as HTMLElement).style.width).toBe('28%')
      expect(headers[1].className).toContain('w-24')
    }
  })

  it('keeps an action cell when all row actions are hidden', () => {
    const { container } = render(
      <ResponsiveTable
        columns={columns}
        data={data}
        rowKey={(row) => row.name}
        renderCell={(key, row) => row[key as keyof Row]}
        actions={[{ label: 'Edit', onClick: vi.fn(), show: (row) => row.status === 'online' }]}
      />,
    )
    const rows = container.querySelectorAll('tbody tr')
    expect(rows[0].children).toHaveLength(3)
    expect(rows[1].children).toHaveLength(3)
    expect(rows[1].lastElementChild?.querySelector('button')).toBeNull()
  })

  it('keeps the default empty table quiet and icon-free', () => {
    const { container } = render(
      <ThemeProvider>
        <ResponsiveTable
          columns={columns}
          data={[]}
          renderCell={() => null}
          rowKey={() => 'empty'}
        />
      </ThemeProvider>
    )

    expect(screen.getAllByText('No data available').length).toBeGreaterThan(0)
    expect(container.querySelector('tbody svg')).toBeNull()
  })

})
