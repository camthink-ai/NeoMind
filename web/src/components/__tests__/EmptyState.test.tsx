import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { Inbox } from 'lucide-react'
import { EmptyState, EmptyStateInline } from '@/components/shared/EmptyState'

describe('EmptyState', () => {
  it('renders a supplied visual without a decorative tile', () => {
    const { container } = render(
      <EmptyState
        icon={<Inbox />}
        title="Nothing here"
        description="Add an item to begin"
        action={{ label: 'Add item', onClick: () => {} }}
      />,
    )

    expect(screen.getByText('Nothing here')).toBeInTheDocument()
    const iconContainer = container.querySelector('svg')?.parentElement
    expect(iconContainer?.className).not.toContain('bg-primary-light')
    expect(iconContainer?.className).not.toContain('border')
  })

  it('keeps inline empty rows text-only', () => {
    const { container } = render(
      <table>
        <tbody>
          <EmptyStateInline title="No results" />
        </tbody>
      </table>,
    )

    expect(screen.getByText('No results')).toBeInTheDocument()
    expect(container.querySelector('svg')).toBeNull()
  })
})
