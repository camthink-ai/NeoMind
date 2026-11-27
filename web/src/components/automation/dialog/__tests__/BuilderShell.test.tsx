import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { BuilderShell } from '../BuilderShell'

// Mock hooks that might cause issues in jsdom
vi.mock('@/hooks/useMobile', () => ({ useIsMobile: () => false, useSafeAreaInsets: () => ({ top: 0, bottom: 0 }) }))
vi.mock('@/hooks/useBodyScrollLock', () => ({ useBodyScrollLock: () => {} }))

describe('BuilderShell', () => {
  const baseProps = {
    open: true,
    onOpenChange: () => {},
    accent: 'indigo' as const,
    title: '新建规则',
    icon: <span>R</span>,
  }

  it('renders title and all four slots', () => {
    render(
      <BuilderShell
        {...baseProps}
        config={<div>CONFIG_RAIL</div>}
        workspace={<div>WORKSPACE_CANVAS</div>}
        footer={
          <>
            <button>取消</button>
            <button>保存</button>
          </>
        }
      />
    )
    expect(screen.getByText('新建规则')).toBeInTheDocument()
    expect(screen.getByText('CONFIG_RAIL')).toBeInTheDocument()
    expect(screen.getByText('WORKSPACE_CANVAS')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '取消' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '保存' })).toBeInTheDocument()
  })

  it('renders the status indicator in the header when provided', () => {
    render(
      <BuilderShell
        {...baseProps}
        statusIndicator={<span data-testid="status">已启用</span>}
        config={<div>c</div>}
        workspace={<div>w</div>}
        footer={<button>f</button>}
      />
    )
    expect(screen.getByTestId('status')).toBeInTheDocument()
  })
})