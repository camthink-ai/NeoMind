import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { Badge } from '@/components/ui/badge'
import { Card } from '@/components/ui/card'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Switch } from '@/components/ui/switch'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Slider } from '@/components/ui/slider'
import {
  Sparkles,
  Loader2,
  CheckCircle2,
  AlertTriangle,
  ChevronRight,
  Info,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { api } from '@/lib/api'
import { useToast } from '@/hooks/use-toast'

export interface DeviceTypeGeneratorDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onDeviceTypeCreated: () => void
}

interface GeneratedMetric {
  name: string
  path: string
  display_name: string
  description: string
  data_type: string
  semantic_type: string
  unit: string | null
  readable: boolean
  writable: boolean
  confidence: number
}

interface GeneratedCommand {
  name: string
  display_name: string
  description: string
  parameters: Array<{ name: string; type_: string; required: boolean }>
  confidence: number
}

// Helper to convert backend data_type string to MetricDataType format
function convertDataType(dataType: string): any {
  // Backend returns types like "Integer", "Float", "String", "Boolean", "Binary", "Enum"
  // These match the Rust MetricDataType enum variants
  const normalized = dataType.toLowerCase()
  if (normalized === 'enum') {
    // For enum types, backend returns format like "Enum([...])"
    // We'll need to parse this or default to String
    return 'string'
  }
  return normalized
}

interface GeneratedDeviceType {
  id: string
  name: string
  description: string
  category: string
  manufacturer: string
  metrics: GeneratedMetric[]
  commands: GeneratedCommand[]
  confidence: number
}

interface ValidationResult {
  is_valid: boolean
  issues: string[]
  warnings: string[]
}

type TabValue = 'samples' | 'metrics' | 'commands' | 'preview'

export function DeviceTypeGeneratorDialog({
  open,
  onOpenChange,
  onDeviceTypeCreated,
}: DeviceTypeGeneratorDialogProps) {
  const { t } = useTranslation(['common', 'devices'])
  const { toast } = useToast()

  // Sample data input
  const [sampleData, setSampleData] = useState('')

  // Analysis state
  const [analyzing, setAnalyzing] = useState(false)
  const [generated, setGenerated] = useState<GeneratedDeviceType | null>(null)
  const [activeTab, setActiveTab] = useState<TabValue>('samples')

  // Creation state
  const [creating, setCreating] = useState(false)
  const [deviceTypeName, setDeviceTypeName] = useState('')
  const [deviceTypeId, setDeviceTypeId] = useState('')
  const [enableAfterCreate, setEnableAfterCreate] = useState(true)

  // Validation
  const [validation, setValidation] = useState<ValidationResult | null>(null)

  // Filtering configuration
  const [minCoverage, setMinCoverage] = useState(0) // 0-100, default 0 (include all)
  const [minConfidence, setMinConfidence] = useState(0) // 0-100, default 0 (include all)

  // Helper: Parse JSON samples - supports multiple formats
  // 1. JSONL: One JSON object per line
  // 2. JSON Array: [{"key": "value"}, {"key": "value"}]
  // 3. Single multi-line JSON object
  const parseSamples = (): Array<{ timestamp: number; data: Record<string, unknown> }> => {
    const trimmed = sampleData.trim()
    if (!trimmed) return []

    const samples: Array<{ timestamp: number; data: Record<string, unknown> }> = []

    // Try parsing as JSON array first
    try {
      const parsed = JSON.parse(trimmed)
      if (Array.isArray(parsed)) {
        // Valid JSON array
        return parsed.filter((item): item is Record<string, unknown> =>
          item && typeof item === 'object'
        ).map(data => ({
          timestamp: Date.now(),
          data,
        }))
      } else if (typeof parsed === 'object' && parsed !== null) {
        // Single JSON object
        return [{
          timestamp: Date.now(),
          data: parsed as Record<string, unknown>,
        }]
      }
    } catch {
      // Not a valid JSON array/object, fall through to JSONL parsing
    }

    // Try JSONL format (one JSON per line)
    const lines = trimmed.split('\n').filter(Boolean)
    for (const line of lines) {
      const trimmedLine = line.trim()
      if (!trimmedLine) continue

      try {
        const data = JSON.parse(trimmedLine)
        if (typeof data === 'object' && data !== null) {
          samples.push({
            timestamp: Date.now(),
            data: data as Record<string, unknown>,
          })
        }
      } catch {
        // Skip invalid JSON lines
      }
    }

    return samples
  }

  const tGen = (key: string, params?: Record<string, unknown>) => t(`devices:types.generator.${key}`, params)

  // Analyze samples to generate device type
  const handleAnalyze = async () => {
    const samples = parseSamples()
    if (samples.length === 0) {
      toast({
        title: tGen('toast.noValidSamples'),
        description: tGen('step1.noValidSamplesDesc'),
        variant: 'destructive',
      })
      return
    }

    setAnalyzing(true)
    try {
      const response = await api.generateDeviceTypeFromSamples({
        device_id: 'device-type', // Default fallback, not user-facing
        manufacturer: undefined,
        samples,
        min_coverage: minCoverage / 100, // Convert 0-100 to 0.0-1.0
        min_confidence: minConfidence / 100, // Convert 0-100 to 0.0-1.0
      })

      setGenerated(response as unknown as GeneratedDeviceType)

      // Pre-fill device type name
      if (response.name) {
        setDeviceTypeName(response.name)
      }
      if (response.id) {
        setDeviceTypeId(response.id.replace('auto-generated-', ''))
      }

      setActiveTab('metrics')
      toast({
        title: tGen('toast.analysisComplete'),
        description: tGen('toast.analysisCompleteDesc', {
          metrics: response.metrics?.length || 0,
          commands: response.commands?.length || 0,
        }),
      })
    } catch (error) {
      toast({
        title: tGen('toast.analysisFailed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    } finally {
      setAnalyzing(false)
    }
  }

  // Validate generated device type
  const handleValidate = () => {
    if (!generated) return

    const issues: string[] = []
    const warnings: string[] = []

    // Check if device type has any metrics
    if (generated.metrics.length === 0) {
      warnings.push(tGen('step4.noMetricsWarning'))
    }

    // Check confidence level
    if (generated.confidence < 0.5) {
      warnings.push(
        tGen('step4.lowConfidenceWarning', {
          percent: Math.round(generated.confidence * 100),
        })
      )
    }

    // Check for required fields based on category
    if (generated.category === 'unknown') {
      warnings.push(tGen('step4.unknownCategoryWarning'))
    }

    // Check for duplicate metric names
    const metricNames = new Set<string>()
    for (const metric of generated.metrics) {
      if (!metricNames.add(metric.name)) {
        issues.push(tGen('step4.duplicateMetric', { name: metric.name }))
      }
    }

    setValidation({
      is_valid: issues.length === 0,
      issues,
      warnings,
    })

    return issues.length === 0
  }

  // Create the device type
  const handleCreate = async () => {
    if (!generated) return
    if (!deviceTypeName.trim()) {
      toast({
        title: tGen('toast.nameRequired'),
        description: tGen('toast.nameRequiredDesc'),
        variant: 'destructive',
      })
      return
    }
    if (!deviceTypeId.trim()) {
      toast({
        title: tGen('toast.idRequired'),
        description: tGen('toast.idRequiredDesc'),
        variant: 'destructive',
      })
      return
    }

    // Validate first
    if (!handleValidate()) {
      toast({
        title: tGen('toast.validationFailed'),
        description: tGen('toast.validationFailedDesc'),
        variant: 'destructive',
      })
      return
    }

    setCreating(true)
    try {
      await api.addDeviceType({
        device_type: deviceTypeId,
        name: deviceTypeName,
        description: generated.description,
        categories: [generated.category],
        mode: 'full',
        metrics: generated.metrics.map((m) => ({
          name: m.name,
          display_name: m.display_name,
          data_type: convertDataType(m.data_type),
          unit: m.unit || undefined,
          min: undefined,
          max: undefined,
          required: false,
        })),
        commands: generated.commands.map((c) => ({
          name: c.name,
          display_name: c.display_name,
          payload_template: `{"action": "${c.name}"}`,
          parameters: c.parameters.map((p) => ({
            name: p.name,
            display_name: p.name,
            data_type: convertDataType(p.type_),
            default_value: undefined,
            min: undefined,
            max: undefined,
            unit: undefined,
            allowed_values: [],
          })),
          samples: [],
          llm_hints: c.description,
        })),
        uplink_samples: parseSamples().map((s) => s.data),
      })

      toast({
        title: tGen('toast.created'),
        description: tGen('toast.createdDesc', { name: deviceTypeName }),
      })

      onDeviceTypeCreated()
      onOpenChange(false)

      // Reset state
      setSampleData('')
      setGenerated(null)
      setDeviceTypeName('')
      setDeviceTypeId('')
      setActiveTab('samples')
      setValidation(null)
    } catch (error) {
      toast({
        title: tGen('toast.creationFailed'),
        description: (error as Error).message,
        variant: 'destructive',
      })
    } finally {
      setCreating(false)
    }
  }

  // Load sample data
  const loadSample = () => {
    setSampleData(`// JSONL format (one JSON object per line):
{"temp": 25.5, "hum": 60, "battery": 85}
{"temp": 26.0, "hum": 58, "battery": 84}
{"temp": 24.8, "hum": 62, "battery": 83}
{"temp": 25.2, "hum": 59, "battery": 84}

// You can also use JSON array: [{"temp": 25.5, "hum": 60}, {"temp": 26.0, "hum": 58}]
// Or a single multi-line JSON object`)
  }

  const getConfidenceColor = (confidence: number) => {
    if (confidence >= 0.8) return 'text-green-500'
    if (confidence >= 0.5) return 'text-yellow-500'
    return 'text-red-500'
  }

  const getConfidenceLabel = (confidence: number) => {
    if (confidence >= 0.8) return tGen('confidence.high')
    if (confidence >= 0.5) return tGen('confidence.medium')
    return tGen('confidence.low')
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-4xl max-h-[90vh] flex flex-col p-0 [&>[data-radix-dialog-close]]:right-6 [&>[data-radix-dialog-close]]:top-7">
        <DialogHeader className="px-6 pt-6 pb-4 border-b shrink-0">
          <DialogTitle className="flex items-center gap-2">
            <Sparkles className="h-5 w-5 text-purple-500" />
            {tGen('title')}
          </DialogTitle>
          <DialogDescription>
            {tGen('description')}
          </DialogDescription>
        </DialogHeader>

        <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as TabValue)} className="flex-1 flex flex-col min-h-0">
          <div className="px-6 shrink-0">
            <TabsList className="grid w-full grid-cols-4">
              <TabsTrigger value="samples" disabled={analyzing}>
                1. {tGen('tabs.samples')}
              </TabsTrigger>
              <TabsTrigger value="metrics" disabled={!generated || analyzing}>
                2. {tGen('tabs.metrics')}
              </TabsTrigger>
              <TabsTrigger value="commands" disabled={!generated || analyzing}>
                3. {tGen('tabs.commands')}
              </TabsTrigger>
              <TabsTrigger value="preview" disabled={!generated || analyzing}>
                4. {tGen('tabs.preview')}
              </TabsTrigger>
            </TabsList>
          </div>

          {/* Scrollable content area */}
          <div className="flex-1 overflow-y-auto px-6">
            <div className="py-4 space-y-4">
              {/* Step 1: Input Samples */}
              {activeTab === 'samples' && (
                <>
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2 text-sm text-muted-foreground">
                      <Info className="h-4 w-4" />
                      <span>{tGen('step1.info')}</span>
                    </div>
                    <Button variant="ghost" size="sm" onClick={loadSample}>
                      {tGen('step1.loadSample')}
                    </Button>
                  </div>

                  {/* Filtering Configuration */}
                  <div className="grid grid-cols-2 gap-6 p-4 rounded-lg border bg-muted/30">
                    <div className="space-y-3">
                      <div className="flex items-center justify-between">
                        <Label htmlFor="min-coverage" className="text-sm">
                          {tGen('step1.minCoverage')}
                        </Label>
                        <span className="text-xs text-muted-foreground">
                          {minCoverage}%
                        </span>
                      </div>
                      <Slider
                        id="min-coverage"
                        value={[minCoverage]}
                        onValueChange={([v]: number[]) => setMinCoverage(v)}
                        min={0}
                        max={100}
                        step={5}
                        className="w-full"
                      />
                      <p className="text-xs text-muted-foreground">
                        {tGen('step1.minCoverageDesc')}
                      </p>
                    </div>

                    <div className="space-y-3">
                      <div className="flex items-center justify-between">
                        <Label htmlFor="min-confidence" className="text-sm">
                          {tGen('step1.minConfidence')}
                        </Label>
                        <span className="text-xs text-muted-foreground">
                          {minConfidence}%
                        </span>
                      </div>
                      <Slider
                        id="min-confidence"
                        value={[minConfidence]}
                        onValueChange={([v]: number[]) => setMinConfidence(v)}
                        min={0}
                        max={100}
                        step={5}
                        className="w-full"
                      />
                      <p className="text-xs text-muted-foreground">
                        {tGen('step1.minConfidenceDesc')}
                      </p>
                    </div>
                  </div>

                  <div className="space-y-2">
                    <Label htmlFor="sample-data">{tGen('step1.sampleData')}</Label>
                    <Textarea
                      id="sample-data"
                      placeholder={tGen('step1.sampleDataPlaceholder')}
                      value={sampleData}
                      onChange={(e) => setSampleData(e.target.value)}
                      rows={8}
                      className="font-mono text-sm"
                    />
                    <p className="text-xs text-muted-foreground">
                      {tGen('step1.validSamples', { count: parseSamples().length })}
                    </p>
                  </div>

                  <Button
                    onClick={handleAnalyze}
                    disabled={parseSamples().length === 0 || analyzing}
                    className="w-full"
                    size="lg"
                  >
                    {analyzing ? (
                      <>
                        <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                        {tGen('step1.analyzing')}
                      </>
                    ) : (
                      <>
                        <Sparkles className="mr-2 h-4 w-4" />
                        {tGen('step1.generateButton')}
                      </>
                    )}
                  </Button>
                </>
              )}

              {/* Step 2: Generated Metrics */}
              {activeTab === 'metrics' && generated && (
                <>
                  <div className="flex items-center justify-between">
                    <div>
                      <h3 className="text-lg font-semibold">{tGen('step2.title')}</h3>
                      <p className="text-sm text-muted-foreground">
                        {tGen('step2.metricsFound', { count: generated.metrics.length })}
                      </p>
                    </div>
                    <Badge variant="outline" className={getConfidenceColor(generated.confidence)}>
                      {getConfidenceLabel(generated.confidence)} {tGen('step2.confidence')}
                    </Badge>
                  </div>

                  <Card>
                    <div className="overflow-x-auto">
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead>{tGen('step2.field')}</TableHead>
                            <TableHead>{tGen('step2.semanticType')}</TableHead>
                            <TableHead>{tGen('step2.displayName')}</TableHead>
                            <TableHead>{tGen('step2.unit')}</TableHead>
                            <TableHead className="text-center">{tGen('step2.readable')}</TableHead>
                            <TableHead className="text-center">{tGen('step2.writable')}</TableHead>
                            <TableHead>{tGen('step2.confidence')}</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {generated.metrics.map((metric) => (
                            <TableRow key={metric.name}>
                              <TableCell className="font-mono text-xs">{metric.path}</TableCell>
                              <TableCell>
                                <Badge variant="secondary">{metric.semantic_type}</Badge>
                              </TableCell>
                              <TableCell>{metric.display_name}</TableCell>
                              <TableCell>{metric.unit || '-'}</TableCell>
                              <TableCell className="text-center">
                                {metric.readable ? (
                                  <CheckCircle2 className="h-4 w-4 text-green-500 mx-auto" />
                                ) : (
                                  <span className="text-muted-foreground">-</span>
                                )}
                              </TableCell>
                              <TableCell className="text-center">
                                {metric.writable ? (
                                  <CheckCircle2 className="h-4 w-4 text-green-500 mx-auto" />
                                ) : (
                                  <span className="text-muted-foreground">-</span>
                                )}
                              </TableCell>
                              <TableCell>
                                <Badge
                                  variant="outline"
                                  className={getConfidenceColor(metric.confidence)}
                                >
                                  {Math.round(metric.confidence * 100)}%
                                </Badge>
                              </TableCell>
                            </TableRow>
                          ))}
                        </TableBody>
                      </Table>
                    </div>
                  </Card>

                  <div className="flex justify-between">
                    <Button variant="outline" onClick={() => setActiveTab('samples')}>
                      {tGen('step2.back')}
                    </Button>
                    <Button onClick={() => setActiveTab('commands')}>
                      {tGen('step2.next')}
                      <ChevronRight className="ml-2 h-4 w-4" />
                    </Button>
                  </div>
                </>
              )}

              {/* Step 3: Generated Commands */}
              {activeTab === 'commands' && generated && (
                <>
                  <div>
                    <h3 className="text-lg font-semibold">{tGen('step3.title')}</h3>
                    <p className="text-sm text-muted-foreground">
                      {tGen('step3.commandsFound', { count: generated.commands.length })}
                    </p>
                  </div>

                  {generated.commands.length > 0 ? (
                    <Card>
                      <div className="overflow-x-auto">
                        <Table>
                          <TableHeader>
                            <TableRow>
                              <TableHead>{tGen('step3.command')}</TableHead>
                              <TableHead>{tGen('step2.displayName')}</TableHead>
                              <TableHead>{tGen('step3.description')}</TableHead>
                              <TableHead>{tGen('step3.parameters')}</TableHead>
                              <TableHead>{tGen('step2.confidence')}</TableHead>
                            </TableRow>
                          </TableHeader>
                          <TableBody>
                            {generated.commands.map((cmd) => (
                              <TableRow key={cmd.name}>
                                <TableCell className="font-mono text-xs">{cmd.name}</TableCell>
                                <TableCell>{cmd.display_name}</TableCell>
                                <TableCell className="text-sm text-muted-foreground">
                                  {cmd.description}
                                </TableCell>
                                <TableCell>
                                  {cmd.parameters.length > 0 ? (
                                    cmd.parameters.map((p) => p.name).join(', ')
                                  ) : (
                                    '-'
                                  )}
                                </TableCell>
                                <TableCell>
                                  <Badge
                                    variant="outline"
                                    className={getConfidenceColor(cmd.confidence)}
                                  >
                                    {Math.round(cmd.confidence * 100)}%
                                  </Badge>
                                </TableCell>
                              </TableRow>
                            ))}
                          </TableBody>
                        </Table>
                      </div>
                    </Card>
                  ) : (
                    <Card className="p-8 text-center">
                      <p className="text-muted-foreground">
                        {tGen('step3.noCommands')}
                        <br />
                        {tGen('step3.noCommandsDesc')}
                      </p>
                    </Card>
                  )}

                  <div className="flex justify-between">
                    <Button variant="outline" onClick={() => setActiveTab('metrics')}>
                      {tGen('step3.back')}
                    </Button>
                    <Button onClick={() => setActiveTab('preview')}>
                      {tGen('step3.next')}
                      <ChevronRight className="ml-2 h-4 w-4" />
                    </Button>
                  </div>
                </>
              )}

              {/* Step 4: Preview & Create */}
              {activeTab === 'preview' && generated && (
                <>
                  <div>
                    <h3 className="text-lg font-semibold">{tGen('step4.title')}</h3>
                    <p className="text-sm text-muted-foreground">
                      {tGen('step4.description')}
                    </p>
                  </div>

                  <div className="grid grid-cols-2 gap-4">
                    <div className="space-y-2">
                      <Label htmlFor="type-name">{tGen('step4.typeName')}</Label>
                      <Input
                        id="type-name"
                        placeholder={tGen('step4.typeNamePlaceholder')}
                        value={deviceTypeName}
                        onChange={(e) => setDeviceTypeName(e.target.value)}
                      />
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor="type-id">{tGen('step4.typeId')}</Label>
                      <Input
                        id="type-id"
                        placeholder={tGen('step4.typeIdPlaceholder')}
                        value={deviceTypeId}
                        onChange={(e) => setDeviceTypeId(e.target.value)}
                      />
                    </div>
                  </div>

                  <Card className="p-4">
                    <ScrollArea className="h-48">
                      <pre className="text-xs">
                        {JSON.stringify(
                          {
                            device_type: deviceTypeId || 'example-id',
                            name: deviceTypeName || 'Example Device',
                            description: generated.description,
                            categories: [generated.category],
                            mode: 'full',
                            metrics: generated.metrics.map((m) => ({
                              name: m.name,
                              display_name: m.display_name,
                              data_type: convertDataType(m.data_type),
                              unit: m.unit || undefined,
                              min: undefined,
                              max: undefined,
                              required: false,
                            })),
                            commands: generated.commands.map((c) => ({
                              name: c.name,
                              display_name: c.display_name,
                              payload_template: `{"action": "${c.name}"}`,
                              parameters: c.parameters.map((p) => ({
                                name: p.name,
                                display_name: p.name,
                                data_type: convertDataType(p.type_),
                                default_value: undefined,
                                min: undefined,
                                max: undefined,
                                unit: undefined,
                                allowed_values: [],
                              })),
                              samples: [],
                              llm_hints: c.description,
                            })),
                          },
                          null,
                          2
                        )}
                      </pre>
                    </ScrollArea>
                  </Card>

                  {/* Validation Results */}
                  {validation && (
                    <Card className={cn('p-4', validation.is_valid ? 'border-green-500' : 'border-yellow-500')}>
                      <h4 className="font-semibold flex items-center gap-2 mb-2">
                        {validation.is_valid ? (
                          <>
                            <CheckCircle2 className="h-4 w-4 text-green-500" />
                            {tGen('step4.validationPassed')}
                          </>
                        ) : (
                          <>
                            <AlertTriangle className="h-4 w-4 text-yellow-500" />
                            {tGen('step4.validationWarnings')}
                          </>
                        )}
                      </h4>
                      {validation.issues.length > 0 && (
                        <ul className="list-disc list-inside text-sm text-red-500 mb-2">
                          {validation.issues.map((issue, i) => (
                            <li key={i}>{issue}</li>
                          ))}
                        </ul>
                      )}
                      {validation.warnings.length > 0 && (
                        <ul className="list-disc list-inside text-sm text-yellow-600">
                          {validation.warnings.map((warning, i) => (
                            <li key={i}>{warning}</li>
                          ))}
                        </ul>
                      )}
                    </Card>
                  )}

                  <div className="flex items-center space-x-2">
                    <Switch
                      id="enable"
                      checked={enableAfterCreate}
                      onCheckedChange={setEnableAfterCreate}
                    />
                    <Label htmlFor="enable">{tGen('step4.enableImmediately')}</Label>
                  </div>

                  <div className="flex justify-between pt-4">
                    <Button variant="outline" onClick={() => setActiveTab('commands')}>
                      {tGen('step4.back')}
                    </Button>
                    <Button onClick={handleValidate} disabled={!deviceTypeName || !deviceTypeId}>
                      {tGen('step4.validate')}
                    </Button>
                  </div>
                </>
              )}
            </div>
          </div>
        </Tabs>

        <DialogFooter className="px-6 pt-4 pb-6 border-t shrink-0">
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={creating}>
            {t('common:cancel')}
          </Button>
          {activeTab === 'preview' && generated && (
            <Button onClick={handleCreate} disabled={creating || !deviceTypeName || !deviceTypeId}>
              {creating ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  {t('common:creating')}
                </>
              ) : (
                <>
                  <CheckCircle2 className="mr-2 h-4 w-4" />
                  {tGen('step4.create')}
                </>
              )}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
