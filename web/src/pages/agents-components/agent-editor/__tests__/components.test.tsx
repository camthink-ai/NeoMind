/// Rendering tests for the agent-editor subcomponents extracted from the
/// AgentEditorFullScreen monolith: the schedule/recommendation/list cards,
/// selected-resource item interactions, and the resource selection dialog.
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { ScheduleCard } from '../ScheduleCard'
import { RecommendationCard } from '../RecommendationCard'
import { ResourceListItem } from '../ResourceListItem'
import { SelectedResourceItem } from '../SelectedResourceItem'
import { ResourceSelectionDialog } from '../ResourceSelectionDialog'
import type {
  AvailableResource,
  ResourceRecommendation,
  SelectedResource,
} from '../types'

const metric = (name: string) => ({ name, display_name: name, source: 'device' as const })

const availableRes: AvailableResource = {
  id: 'dev-1',
  name: '温度计',
  type: 'device',
  metrics: [metric('temperature'), metric('humidity')],
  commands: [{ name: 'reboot', display_name: '重启', source: 'device' }],
}

const selectedRes: SelectedResource = {
  id: 'dev-1',
  name: '温度计',
  type: 'device',
  allMetrics: [metric('temperature'), metric('humidity')],
  allCommands: [],
  selectedMetrics: new Set(['temperature']),
  selectedCommands: new Set(),
}

describe('ScheduleCard', () => {
  it('renders label + description and fires onClick', () => {
    const onClick = vi.fn()
    render(
      <ScheduleCard icon={<span data-testid="ico" />} label="定时" description="按周期运行" active onClick={onClick} />,
    )
    expect(screen.getByText('定时')).toBeTruthy()
    expect(screen.getByText('按周期运行')).toBeTruthy()
    fireEvent.click(screen.getByRole('button'))
    expect(onClick).toHaveBeenCalledOnce()
  })
})

describe('RecommendationCard', () => {
  it('renders name + reason and reports the recommendation on click', () => {
    const onClick = vi.fn()
    const rec: ResourceRecommendation = {
      id: 'dev-1', name: '温度计', type: 'device', reason: '提示词提到温度',
    }
    render(<RecommendationCard recommendation={rec} selected={false} onClick={onClick} />)
    expect(screen.getByText('温度计')).toBeTruthy()
    expect(screen.getByText('提示词提到温度')).toBeTruthy()
    fireEvent.click(screen.getByRole('button'))
    expect(onClick).toHaveBeenCalledOnce()
  })
})

describe('ResourceListItem', () => {
  it('shows metric/command counts and toggles on click', () => {
    const onClick = vi.fn()
    render(<ResourceListItem resource={availableRes} selected={false} onClick={onClick} />)
    expect(screen.getByText('温度计')).toBeTruthy()
    expect(screen.getByText('2 metrics • 1 command')).toBeTruthy()
    fireEvent.click(screen.getByRole('button'))
    expect(onClick).toHaveBeenCalledOnce()
  })
})

describe('SelectedResourceItem', () => {
  it('renders the resource, and remove reports via its labelled button', () => {
    const onRemove = vi.fn()
    render(
      <SelectedResourceItem
        resource={selectedRes}
        setSelectedResources={vi.fn()}
        onRemove={onRemove}
        onToggleMetric={vi.fn()}
        onToggleCommand={vi.fn()}
      />,
    )
    expect(screen.getByText('温度计')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'remove 温度计' }))
    expect(onRemove).toHaveBeenCalledOnce()
  })

  it('expanding reveals metric checkboxes; toggling one reports the metric', () => {
    const onToggleMetric = vi.fn()
    render(
      <SelectedResourceItem
        resource={selectedRes}
        setSelectedResources={vi.fn()}
        onRemove={vi.fn()}
        onToggleMetric={onToggleMetric}
        onToggleCommand={vi.fn()}
      />,
    )
    fireEvent.click(screen.getByRole('button', { name: '温度计' }))
    const boxes = screen.getAllByRole('checkbox')
    expect(boxes.length).toBeGreaterThan(0)
    fireEvent.click(boxes[0])
    expect(onToggleMetric).toHaveBeenCalledWith('dev-1', expect.any(String))
  })
})

describe('ResourceSelectionDialog', () => {
  it('lists available resources and reports selection (desktop)', () => {
    const toggleResource = vi.fn()
    render(
      <ResourceSelectionDialog
        open
        onOpenChange={vi.fn()}
        availableResources={[availableRes]}
        selectedResources={[]}
        setSelectedResources={vi.fn()}
        recommendations={[]}
        generatingRecommendations={false}
        searchQuery=""
        setSearchQuery={vi.fn()}
        toggleResource={toggleResource}
        toggleRecommendation={vi.fn()}
        scheduleType="timer"
      />,
    )
    expect(screen.getByText('温度计')).toBeTruthy()
    fireEvent.click(screen.getByText('温度计'))
    expect(toggleResource).toHaveBeenCalledOnce()
  })
})
