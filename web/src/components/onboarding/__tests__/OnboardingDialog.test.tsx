/// Render tests for the onboarding wizard — guards the step-header contract:
/// every step opens with a visible title (h2) + step kicker, the setup cards
/// list their feature rows, and the completed state renders as a badge beside
/// the title (not a strip that shifts the card). react-i18next is mocked
/// globally (test/setup.ts) to return keys, so assertions match on i18n keys.
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { OnboardingDialog } from '@/components/onboarding/OnboardingDialog'
import { ThemeProvider } from '@/components/ui/theme'
import type { OnboardingStatus } from '@/hooks/useOnboarding'

function makeStatus(llm: boolean, device: boolean): OnboardingStatus {
  return {
    dismissed: false,
    system_status: { has_llm_backend: llm, has_devices: device, device_count: device ? 1 : 0 },
    steps: { llm: { completed: llm }, device: { completed: device } },
  }
}

function renderDialog(status: OnboardingStatus) {
  return render(
    <ThemeProvider>
      <MemoryRouter>
        <OnboardingDialog
          open
          onOpenChange={vi.fn()}
          onDismiss={vi.fn()}
          status={status}
        />
      </MemoryRouter>
    </ThemeProvider>,
  )
}

const next = () => fireEvent.click(screen.getByRole('button', { name: 'onboarding.nav.next' }))
const prev = () => fireEvent.click(screen.getByRole('button', { name: 'onboarding.nav.prev' }))

describe('OnboardingDialog', () => {
  it('every step renders a visible title (h2) with the step kicker', () => {
    renderDialog(makeStatus(false, false))
    // Step 1: welcome — landing step while the LLM is unconfigured
    expect(screen.getByText('onboarding.setup.title').tagName).toBe('H2')
    expect(screen.getByText('onboarding.stepIndicator')).toBeInTheDocument()
    next()
    // Step 2: LLM
    expect(screen.getByText('onboarding.setup.llm.title').tagName).toBe('H2')
    next()
    // Step 3: device
    expect(screen.getByText('onboarding.setup.device.title').tagName).toBe('H2')
    next()
    // Step 4: ready (partial — setup incomplete)
    expect(screen.getByText('onboarding.ready.partialTitle').tagName).toBe('H2')
  })

  it('LLM step lists the three feature rows', () => {
    renderDialog(makeStatus(false, false))
    next()
    for (const key of ['builtin', 'local', 'cloud']) {
      expect(screen.getByText(`onboarding.setup.llm.features.${key}.title`)).toBeInTheDocument()
    }
  })

  it('completed setup step shows the Done badge beside the title', () => {
    renderDialog(makeStatus(true, false)) // lands on the device step
    prev() // back to the completed LLM step
    expect(screen.getByText('onboarding.setup.llm.title').tagName).toBe('H2')
    expect(screen.getByText('onboarding.completed')).toBeInTheDocument()
  })

  it('all-complete status lands on Ready with the celebration title', () => {
    renderDialog(makeStatus(true, true))
    expect(screen.getByText('onboarding.ready.allSetTitle').tagName).toBe('H2')
  })
})
