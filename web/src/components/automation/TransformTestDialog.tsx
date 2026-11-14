import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Card } from '@/components/ui/card'
import { Loader2, Play, AlertTriangle } from 'lucide-react'
import { FormField } from '@/components/ui/field'
import { FormSection, FormSectionGroup } from '@/components/ui/form-section'
import { api } from '@/lib/api'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { TransformTestResultPanel } from './TransformTestResultPanel'

interface TransformTestDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  transformId: string
  devices: Array<{ id: string; name: string; device_type?: string }>
}

type TestResult = {
  metrics: Array<{
    device_id: string
    metric: string
    value: number
    timestamp: number
    quality: number | null
  }>
  count: number
  warnings: string[]
  unverified: string[]
}

export function TransformTestDialog({ open, onOpenChange, transformId, devices }: TransformTestDialogProps) {
  const { t } = useTranslation(['automation', 'common'])

  const [selectedDevice, setSelectedDevice] = useState('')
  const [testData, setTestData] = useState('{\n  "temperature": 23.5,\n  "humidity": 65,\n  "sensors": [\n    { "id": 1, "value": 25.3 },\n    { "id": 2, "value": 22.1 }\n  ]\n}')
  const [testing, setTesting] = useState(false)
  const [result, setResult] = useState<TestResult | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (open && devices.length > 0 && !selectedDevice) {
      setSelectedDevice(devices[0].id)
    }
  }, [open, devices, selectedDevice])

  // Reset state when dialog opens
  useEffect(() => {
    if (open) {
      setResult(null)
      setError(null)
    }
  }, [open])

  const handleTest = async () => {
    if (!selectedDevice) return

    setTesting(true)
    setResult(null)
    setError(null)

    try {
      let parsedData
      try {
        parsedData = JSON.parse(testData)
      } catch {
        setError(t('automation:invalidJson', { defaultValue: 'Invalid JSON data' }))
        setTesting(false)
        return
      }

      const device = devices.find((d) => d.id === selectedDevice)
      const response = await api.testTransform(transformId, {
        device_id: selectedDevice,
        device_type: device?.device_type,
        data: parsedData,
        timestamp: Math.floor(Date.now() / 1000),
      })

      setResult({
        metrics: response.metrics ?? [],
        count: response.count ?? 0,
        warnings: response.warnings ?? [],
        unverified: response.unverified ?? [],
      })
    } catch (err) {
      setError(err instanceof Error ? err.message : t('automation:testFailed', { defaultValue: 'Test failed' }))
    } finally {
      setTesting(false)
    }
  }

  const loadSampleData = () => {
    if (!selectedDevice) return

    const device = devices.find((d) => d.id === selectedDevice)
    const deviceType = device?.device_type || 'sensor'

    const samples: Record<string, unknown> = {
      sensor: {
        temperature: 23.5,
        humidity: 65,
        pressure: 1013.25,
        sensors: [
          { id: 1, type: 'temp', value: 25.3 },
          { id: 2, type: 'temp', value: 22.1 },
          { id: 3, type: 'humidity', value: 68 },
        ],
      },
      switch: {
        state: 'on',
        power: 150,
        voltage: 220,
        current: 0.68,
      },
      thermostat: {
        current_temperature: 21.5,
        target_temperature: 23,
        mode: 'heating',
        humidity: 55,
      },
    }

    const sample = samples[deviceType] || samples.sensor
    setTestData(JSON.stringify(sample, null, 2))
  }

  return (
    <UnifiedFormDialog
      open={open}
      onOpenChange={onOpenChange}
      title={t('automation:testTransform', { defaultValue: 'Test Transform' })}
      description={t('automation:testTransformDesc', { defaultValue: 'Test your transform with sample data to verify it produces the expected virtual metrics.' })}
      icon={<Play className="h-5 w-5 text-primary" />}
      width="2xl"
      preventCloseOnSubmit={false}
      isSubmitting={testing}
      footer={
        <>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common:close')}
          </Button>
          <Button onClick={handleTest} disabled={!selectedDevice || testing}>
            {testing ? <Loader2 className="h-4 w-4 mr-2 animate-spin" /> : <Play className="h-4 w-4 mr-2" />}
            {t('automation:runTest', { defaultValue: 'Run Test' })}
          </Button>
        </>
      }
    >
      <FormSectionGroup>
        <FormSection title={t('automation:testDevice', { defaultValue: 'Test Device' })}>
          <FormField label={t('automation:selectDevice', { defaultValue: 'Select device' })}>
            <Select value={selectedDevice} onValueChange={setSelectedDevice}>
              <SelectTrigger>
                <SelectValue placeholder={t('automation:selectDevice', { defaultValue: 'Select device' })} />
              </SelectTrigger>
              <SelectContent>
                {devices.map((d) => (
                  <SelectItem key={d.id} value={d.id}>
                    {d.name} {d.device_type && `(${d.device_type})`}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>
        </FormSection>

        <FormSection title={t('automation:testData', { defaultValue: 'Test Data (JSON)' })}>
          <div className="flex justify-end mb-2">
            <Button variant="ghost" size="sm" onClick={loadSampleData}>
              {t('automation:loadSample', { defaultValue: 'Load Sample' })}
            </Button>
          </div>
          <Textarea
            value={testData}
            onChange={(e) => setTestData(e.target.value)}
            placeholder='{"temperature": 23.5, "humidity": 65}'
            rows={10}
            className="font-mono text-sm"
          />
        </FormSection>

        {error && (
          <Card className="p-4 bg-muted border-error">
            <div className="flex items-center gap-2 text-error">
              <AlertTriangle className="h-4 w-4" />
              <span>{error}</span>
            </div>
          </Card>
        )}

        {result && (
          <FormSection title={t('automation:testResults', { defaultValue: 'Test Results' })}>
            <TransformTestResultPanel
              metrics={result.metrics}
              count={result.count}
              warnings={result.warnings}
              unverified={result.unverified}
            />
          </FormSection>
        )}
      </FormSectionGroup>
    </UnifiedFormDialog>
  )
}
