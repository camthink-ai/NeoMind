/// Save-contract test for the manual device-add form (the tab of
/// AddDeviceGlobalDialog users type into): choosing a device type enables
/// the footer submit, and the composed AddDeviceRequest carries the typed
/// fields with the documented fallbacks.
import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { useState } from 'react'
import type { ReactNode } from 'react'
import { ManualAddForm } from '../AddDeviceGlobalDialog'
import type { DeviceType, AddDeviceRequest } from '@/types'

// Stable `t`: without a provider, react-i18next hands the component a `t`
// whose identity changes every render; the form's footer-publishing effect
// depends on it and re-renders forever inside act(). A module-level
// constant t breaks the cycle — the app gets stability from its provider.
vi.mock('react-i18next', () => {
  const t = (key: string) => key
  return {
    useTranslation: () => ({ t, i18n: {} }),
    initReactI18next: { type: '3rdParty', init: () => {} },
  }
})

// The real Radix Select doesn't open under jsdom fireEvent in this version
// (panel renders empty). Swap it for a native <select> wired to the same
// value/onValueChange props — the contract under test is the form state →
// AddDeviceRequest composition, not Radix's popup.
vi.mock('@/components/ui/select', async () => {
  const React = await import('react')
  return {
    Select: ({ value, onValueChange, children }: {
      value?: string
      onValueChange?: (v: string) => void
      children: React.ReactNode
    }) => (
      <select
        data-testid="native-select"
        value={value ?? ''}
        onChange={(e) => onValueChange?.(e.target.value)}
      >
        {children}
      </select>
    ),
    SelectTrigger: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
    SelectValue: () => null,
    SelectContent: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
    SelectItem: ({ value, children }: { value: string; children?: React.ReactNode }) => (
      <option value={value}>{children}</option>
    ),
  }
})

// No '@/lib/api' mock: the form fail-softs its mount-time fetches
// (.catch(() => null)), and mocking the barrel with importOriginal
// deadlocks the store↔api module cycle (store reads tokenManager from the
// barrel at module scope). The fetches reject on the relative URL in jsdom
// exactly as they do offline — the paths under test don't touch them.

const deviceTypes = [
  { device_type: 'temp-sensor', name: '温度传感器' },
] as unknown as DeviceType[]

function Harness(props: {
  onAdd: (r: AddDeviceRequest) => Promise<boolean>
  onSuccess: () => void
}) {
  // Both callbacks come from the test scope (stable identities): inline
  // arrows here would recreate handleSubmit's deps every footer re-render
  // and loop the form's footer-publishing effect forever inside act().
  const [footer, setFooter] = useState<ReactNode>(null)
  return (
    <>
      <ManualAddForm
        deviceTypes={deviceTypes}
        onAdd={props.onAdd}
        adding={false}
        onSuccess={props.onSuccess}
        renderFooter={setFooter}
      />
      <div data-testid="footer">{footer}</div>
    </>
  )
}

describe('ManualAddForm (device-add save contract)', () => {
  it('submit stays disabled until a device type is chosen', () => {
    render(<Harness onAdd={vi.fn().mockResolvedValue(true)} onSuccess={() => {}} />)
    const submit = screen.getByTestId('footer').querySelector('button')!
    expect(submit).toBeTruthy()
    expect((submit as HTMLButtonElement).disabled).toBe(true)
  })

  it('composes the AddDeviceRequest from typed fields', async () => {
    const onAdd = vi.fn().mockResolvedValue(true)
    const onSuccess = vi.fn()
    render(<Harness onAdd={onAdd} onSuccess={onSuccess} />)

    await waitFor(() => {
      // device-type select is the first native select in the form
      expect(screen.getAllByTestId('native-select').length).toBeGreaterThan(0)
    })
    fireEvent.change(screen.getAllByTestId('native-select')[0], {
      target: { value: 'temp-sensor' },
    })

    const inputs = screen.getAllByRole('textbox')
    fireEvent.change(inputs[0], { target: { value: 'cam-001' } }) // device id
    fireEvent.change(inputs[1], { target: { value: '车间摄像头' } }) // name

    await waitFor(() => {
      const submit = screen.getByTestId('footer').querySelector('button') as HTMLButtonElement
      expect(submit.disabled).toBe(false)
    })
    fireEvent.click(screen.getByTestId('footer').querySelector('button')!)

    await waitFor(() => expect(onAdd).toHaveBeenCalledTimes(1))
    const request = onAdd.mock.calls[0][0]
    expect(request.device_id).toBe('cam-001')
    expect(request.name).toBe('车间摄像头')
    expect(request.device_type).toBe('temp-sensor')
    expect(request.adapter_type).toBe('mqtt')
    // the form auto-derives MQTT topics from type + device id
    expect(request.connection_config).toEqual({
      telemetry_topic: 'device/temp-sensor/cam-001/uplink',
      command_topic: 'device/temp-sensor/cam-001/downlink',
    })
  })
})
