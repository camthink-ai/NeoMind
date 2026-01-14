import { useState } from "react"
import { useTranslation } from "react-i18next"
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { Badge } from "@/components/ui/badge"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { CodeEditor } from "@/components/ui/code-editor"
import { toast } from "@/components/ui/use-toast"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Plus, Trash2, ArrowDown, FileText, Sparkles, Code } from "lucide-react"
import type { DeviceType, MetricDefinition, CommandDefinition } from "@/types"

// Validation result type
interface ValidationResult {
  valid: boolean
  errors?: string[]
  warnings?: string[]
  message: string
}

// Add Device Type Dialog
interface AddDeviceTypeDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  onAdd: (definition: DeviceType) => Promise<boolean>
  onValidate: (definition: DeviceType) => Promise<ValidationResult>
  onGenerateMDL: (deviceName: string, description: string, uplink: string, downlink: string) => Promise<string>
  adding: boolean
  validating: boolean
  generating: boolean
}

export function AddDeviceTypeDialog({
  open,
  onOpenChange,
  onAdd,
  onValidate,
  onGenerateMDL,
  adding,
  validating,
  generating,
}: AddDeviceTypeDialogProps) {
  const { t } = useTranslation(['common', 'devices'])
  const [addTypeMode, setAddTypeMode] = useState<"advanced" | "ai">("ai")

  // Advanced mode state
  const [advancedDefinition, setAdvancedDefinition] = useState("")
  const [advancedError, setAdvancedError] = useState("")
  const [advancedValidation, setAdvancedValidation] = useState<ValidationResult | null>(null)

  // AI mode state
  const [aiDeviceName, setAiDeviceName] = useState("")
  const [aiDeviceDesc, setAiDeviceDesc] = useState("")
  const [aiUplinkExample, setAiUplinkExample] = useState("")
  const [aiDownlinkExample, setAiDownlinkExample] = useState("")
  const [aiGeneratedDefinition, setAiGeneratedDefinition] = useState("")
  const [aiError, setAiError] = useState("")
  const [aiValidation, setAiValidation] = useState<ValidationResult | null>(null)

  const generateDeviceTypeId = (name: string): string => {
    return name.toLowerCase()
      .replace(/\s+/g, "_")
      .replace(/[^a-z0-9_]/g, "")
      .replace(/_+/g, "_")
      .replace(/^_|_$/g, "")
  }

  const handleAdd = async () => {
    let definition: DeviceType

    if (addTypeMode === "advanced") {
      if (!advancedDefinition.trim()) {
        setAdvancedError(t('devices:types.add.definitionRequired'))
        return
      }
      try {
        definition = JSON.parse(advancedDefinition)
      } catch (e) {
        setAdvancedError(t('devices:types.add.jsonError', { error: (e as Error).message }))
        return
      }
      if (!definition.device_type || !definition.name) {
        setAdvancedError(t('devices:types.add.missingFields'))
        return
      }
    } else {
      if (!aiGeneratedDefinition.trim()) {
        setAiError(t('devices:types.add.generateFirst'))
        return
      }
      try {
        definition = JSON.parse(aiGeneratedDefinition)
      } catch (e) {
        setAiError(t('devices:types.add.parseError', { error: (e as Error).message }))
        return
      }
    }

    const success = await onAdd(definition)
    if (success) {
      resetAllForms()
      onOpenChange(false)
      toast({
        title: t('devices:types.save.success'),
        description: t('devices:types.save.added', { name: definition.name }),
      })
    } else {
      toast({
        title: t('devices:types.save.error'),
        description: t('devices:types.save.retry'),
        variant: "destructive",
      })
    }
  }

  const handleValidate = async () => {
    if (addTypeMode === "advanced") {
      setAdvancedError("")
      setAdvancedValidation(null)
      if (!advancedDefinition.trim()) {
        setAdvancedError(t('devices:types.add.definitionRequired'))
        return
      }
      try {
        const definition = JSON.parse(advancedDefinition)
        const result = await onValidate(definition)
        setAdvancedValidation(result)
        if (!result.valid) {
          setAdvancedError(result.errors?.join("; ") || t('devices:types.validate.failed'))
        }
      } catch (e) {
        setAdvancedError(t('devices:types.add.jsonError', { error: (e as Error).message }))
      }
    } else {
      setAiError("")
      setAiValidation(null)
      if (!aiGeneratedDefinition.trim()) {
        setAiError(t('devices:types.add.nameRequired'))
        return
      }
      try {
        const definition = JSON.parse(aiGeneratedDefinition)
        const result = await onValidate(definition)
        setAiValidation(result)
        if (!result.valid) {
          setAiError(result.errors?.join("; ") || t('devices:types.validate.failed'))
        }
      } catch (e) {
        setAiError(t('devices:types.add.jsonError', { error: (e as Error).message }))
      }
    }
  }

  const handleGenerateMDL = async () => {
    if (!aiDeviceName.trim()) {
      setAiError(t('devices:types.add.nameRequired'))
      return
    }

    try {
      const mdlData = await onGenerateMDL(aiDeviceName, aiDeviceDesc, aiUplinkExample, aiDownlinkExample)
      setAiGeneratedDefinition(JSON.stringify(JSON.parse(mdlData), null, 2))
      setAiValidation(null)
      setAiError("")
    } catch (e) {
      setAiError(t('devices:types.generate.failed', { error: (e as Error).message }))
    }
  }

  const resetAllForms = () => {
    setAdvancedDefinition("")
    setAdvancedError("")
    setAdvancedValidation(null)
    setAiDeviceName("")
    setAiDeviceDesc("")
    setAiUplinkExample("")
    setAiDownlinkExample("")
    setAiGeneratedDefinition("")
    setAiError("")
    setAiValidation(null)
  }

  const loadExample = () => {
    const example = {
      device_type: "example_sensor",
      name: t('devices:types.example.sensor'),
      description: t('devices:types.example.description'),
      categories: ["sensor", "example"],
      uplink: {
        metrics: [
          {
            name: "temperature",
            display_name: t('devices:types.example.temperature'),
            topic: "sensor/${device_id}/temperature",
            data_type: "Float",
            unit: "°C",
            min: -40,
            max: 100,
            required: true,
          },
        ],
      },
      downlink: {
        commands: [
          {
            name: "set_interval",
            display_name: t('devices:types.example.setInterval'),
            topic: "sensor/${device_id}/command",
            payload_template: '{"action":"set_interval","interval":${interval}}',
            parameters: [
              {
                name: "interval",
                display_name: t('devices:types.example.interval'),
                data_type: "Integer",
                default_value: { Integer: 60 },
                min: 10,
                max: 3600,
                unit: t('devices:types.example.seconds'),
              },
            ],
            timeout_ms: 5000,
          },
        ],
      },
    }
    setAdvancedDefinition(JSON.stringify(example, null, 2))
  }

  return (
    <Dialog open={open} onOpenChange={(open) => { if (!open) resetAllForms(); onOpenChange(open) }}>
      <DialogContent className="max-w-5xl max-h-[90vh] flex flex-col p-0">
        <DialogHeader className="px-6 pt-6 pb-4">
          <DialogTitle>{t('devices:types.add.title')}</DialogTitle>
        </DialogHeader>
        <div className="flex-1 overflow-y-auto px-6">
          <Tabs value={addTypeMode} onValueChange={(v) => setAddTypeMode(v as "advanced" | "ai")} className="w-full">
            <TabsList className="grid w-full grid-cols-2">
              <TabsTrigger value="ai" className="flex items-center gap-2">
                <Sparkles className="h-4 w-4" />
                {t('devices:types.smartGeneration')}
              </TabsTrigger>
              <TabsTrigger value="advanced" className="flex items-center gap-2">
                <Code className="h-4 w-4" />
                {t('devices:types.advancedMode')}
              </TabsTrigger>
            </TabsList>

            {/* Advanced Mode */}
            <TabsContent value="advanced" className="mt-4 space-y-4">
              <div className="rounded-md bg-muted p-3 text-sm">
                <p className="font-medium text-foreground mb-1">
                  {t('devices:types.advanced.format')}
                </p>
                <p className="text-muted-foreground mb-2">{t('devices:types.advanced.supported')}</p>
                <button
                  onClick={loadExample}
                  className="text-primary hover:underline"
                >
                  {t('devices:types.loadExample')}
                </button>
              </div>
              <CodeEditor
                value={advancedDefinition}
                onChange={setAdvancedDefinition}
                language="json"
                placeholder={t('devices:types.advanced.placeholder')}
                className="min-h-[400px] max-h-[500px]"
                error={advancedError}
              />
              <div className="flex items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleValidate}
                  disabled={validating || !advancedDefinition.trim()}
                >
                  {generating ? t('devices:types.validating') : t('devices:types.validate.button')}
                </Button>
                {advancedValidation && (
                  <div className={`text-sm ${advancedValidation.valid ? "text-green-600" : "text-error"}`}>
                    {advancedValidation.valid ? "✓ " : "✗ "}{advancedValidation.message}
                  </div>
                )}
              </div>
              {advancedValidation && advancedValidation.errors && advancedValidation.errors.length > 0 && (
                <div className="rounded-md bg-destructive/10 p-3 text-sm">
                  <p className="font-medium text-destructive mb-2">{t('devices:types.errors')}</p>
                  <ul className="list-disc list-inside space-y-1 text-destructive">
                    {advancedValidation.errors.map((err, i) => (
                      <li key={i}>{err}</li>
                    ))}
                  </ul>
                </div>
              )}
              {advancedValidation && advancedValidation.warnings && advancedValidation.warnings.length > 0 && (
                <div className="rounded-md bg-muted p-3 text-sm">
                  <p className="font-medium mb-2">{t('devices:types.warnings')}</p>
                  <ul className="list-disc list-inside space-y-1 text-muted-foreground">
                    {advancedValidation.warnings.map((warn, i) => (
                      <li key={i}>{warn}</li>
                    ))}
                  </ul>
                </div>
              )}
            </TabsContent>

            {/* AI Assisted Mode */}
            <TabsContent value="ai" className="mt-4 space-y-4">
              <div className="rounded-md bg-muted p-3 text-sm">
                <p className="font-medium text-foreground mb-1">{t('devices:types.smart.title')}</p>
                <p className="text-muted-foreground">{t('devices:types.smart.description')}</p>
              </div>

              <div className="grid grid-cols-2 gap-4">
                <div className="space-y-2">
                  <Label htmlFor="ai-device-name">{t('devices:types.smart.nameRequired')}</Label>
                  <Input
                    id="ai-device-name"
                    value={aiDeviceName}
                    onChange={(e) => setAiDeviceName(e.target.value)}
                    placeholder={t('devices:types.smart.nameExample')}
                  />
                  {aiDeviceName && (
                    <p className="text-xs text-muted-foreground">
                      {t('devices:types.add.typeId', { id: generateDeviceTypeId(aiDeviceName) })}
                    </p>
                  )}
                </div>
                <div className="space-y-2">
                  <Label htmlFor="ai-device-desc">{t('devices:types.smart.description')}</Label>
                  <Textarea
                    id="ai-device-desc"
                    value={aiDeviceDesc}
                    onChange={(e) => setAiDeviceDesc(e.target.value)}
                    placeholder={t('devices:types.smart.descExample')}
                    rows={2}
                  />
                </div>
              </div>

              <div className="grid grid-cols-2 gap-4">
                <div className="space-y-2">
                  <Label className="flex items-center gap-2">
                    <ArrowDown className="h-4 w-4 text-green-600" />
                    {t('devices:types.smart.uplink')}
                  </Label>
                  <Textarea
                    value={aiUplinkExample}
                    onChange={(e) => setAiUplinkExample(e.target.value)}
                    placeholder={t('devices:types.smart.uplinkExample')}
                    rows={4}
                    className="font-mono text-sm"
                  />
                  <p className="text-xs text-muted-foreground">{t('devices:types.smart.uplinkHint')}</p>
                </div>
                <div className="space-y-2">
                  <Label className="flex items-center gap-2">
                    <FileText className="h-4 w-4 text-blue-600" />
                    {t('devices:types.smart.downlink')}
                  </Label>
                  <Textarea
                    value={aiDownlinkExample}
                    onChange={(e) => setAiDownlinkExample(e.target.value)}
                    placeholder={t('devices:types.smart.downlinkExample')}
                    rows={4}
                    className="font-mono text-sm"
                  />
                  <p className="text-xs text-muted-foreground">{t('devices:types.smart.downlinkHint')}</p>
                </div>
              </div>

              <Button
                onClick={handleGenerateMDL}
                disabled={!aiDeviceName || generating}
                className="w-full"
              >
                {generating ? (
                  <>
                    <Sparkles className="mr-2 h-4 w-4 animate-spin" />
                    {t('devices:types.generating')}
                  </>
                ) : (
                  <>
                    <Sparkles className="mr-2 h-4 w-4" />
                    {t('devices:types.generate.title')}
                  </>
                )}
              </Button>

              {aiGeneratedDefinition && (
                <div className="space-y-2">
                  <div className="flex items-center justify-between">
                    <Label>{t('devices:types.generated.title')}</Label>
                    <div className="flex gap-2">
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={handleValidate}
                        disabled={validating}
                      >
                        {t('devices:types.validate.button')}
                      </Button>
                    </div>
                  </div>
                  <CodeEditor
                    value={aiGeneratedDefinition}
                    onChange={setAiGeneratedDefinition}
                    language="json"
                    className="min-h-[300px] max-h-[400px]"
                    error={aiError}
                    readOnly={generating}
                  />
                  {aiValidation && (
                    <div className={`text-sm ${aiValidation.valid ? "text-green-600" : "text-error"}`}>
                      {aiValidation.valid ? "✓ " : "✗ "}{aiValidation.message}
                    </div>
                  )}
                </div>
              )}
            </TabsContent>
          </Tabs>
        </div>
        <DialogFooter className="px-6 pb-6 pt-4">
          <Button
            variant="outline"
            size="sm"
            onClick={() => onOpenChange(false)}
            disabled={adding}
          >
            {t('common:cancel')}
          </Button>
          <Button
            onClick={handleAdd}
            disabled={
              adding ||
              (addTypeMode === "advanced" && !advancedDefinition) ||
              (addTypeMode === "ai" && !aiGeneratedDefinition)
            }
            size="sm"
          >
            {adding ? t('devices:types.adding') : t('common:add')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// View Device Type Dialog
interface ViewDeviceTypeDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  deviceType: DeviceType | null
}

export function ViewDeviceTypeDialog({ open, onOpenChange, deviceType }: ViewDeviceTypeDialogProps) {
  const { t } = useTranslation(['common', 'devices'])

  if (!deviceType) return null

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[85vh] flex flex-col">
        <DialogHeader>
          <DialogTitle>{t('devices:types.view.title')}</DialogTitle>
        </DialogHeader>
        <div className="flex-1 overflow-y-auto -mx-6 px-6">
          <Tabs defaultValue="info" className="w-full">
            <TabsList className="sticky top-0 bg-background z-10 mb-4">
              <TabsTrigger value="info">{t('devices:types.view.basicInfo')}</TabsTrigger>
              <TabsTrigger value="uplink">{t('devices:types.view.uplink')}</TabsTrigger>
              <TabsTrigger value="downlink">{t('devices:types.view.downlink')}</TabsTrigger>
            </TabsList>
            <TabsContent value="info" className="mt-0 space-y-4">
              <div className="space-y-2">
                <Label>{t('devices:types.view.typeId')}</Label>
                <p className="font-mono text-sm">{deviceType.device_type}</p>
              </div>
              <div className="space-y-2">
                <Label>{t('devices:types.view.name')}</Label>
                <p className="text-sm">{deviceType.name}</p>
              </div>
              <div className="space-y-2">
                <Label>{t('devices:types.view.description')}</Label>
                <p className="text-sm text-muted-foreground">
                  {deviceType.description || t('devices:types.view.noDescription')}
                </p>
              </div>
              <div className="space-y-2">
                <Label>{t('devices:types.view.categories')}</Label>
                <div className="flex gap-1 flex-wrap">
                  {deviceType.categories.length > 0 ? (
                    deviceType.categories.map((cat, i) => (
                      <Badge key={i} variant="secondary">{cat}</Badge>
                    ))
                  ) : (
                    <span className="text-sm text-muted-foreground">{t('devices:types.view.none')}</span>
                  )}
                </div>
              </div>
              <div className="grid grid-cols-2 gap-4">
                <div className="space-y-2">
                  <Label>{t('devices:types.view.metricCount')}</Label>
                  <p className="text-sm">{deviceType.uplink?.metrics?.length ?? deviceType.metric_count ?? 0}</p>
                </div>
                <div className="space-y-2">
                  <Label>{t('devices:types.view.commandCount')}</Label>
                  <p className="text-sm">{deviceType.downlink?.commands?.length ?? deviceType.command_count ?? 0}</p>
                </div>
              </div>
            </TabsContent>
            <TabsContent value="uplink" className="mt-0">
              {deviceType.uplink?.metrics && deviceType.uplink.metrics.length > 0 ? (
                <div className="space-y-2">
                  {deviceType.uplink.metrics.map((metric, i) => (
                    <div key={i} className="rounded-md border p-3">
                      <div className="flex items-center justify-between">
                        <span className="font-medium">{metric.display_name}</span>
                        <Badge variant="outline" className="text-xs">{metric.data_type}</Badge>
                      </div>
                      <p className="font-mono text-xs text-muted-foreground mt-1">{metric.name}</p>
                      {metric.unit && (
                        <p className="text-xs text-muted-foreground">{t('devices:types.view.unit')}: {metric.unit}</p>
                      )}
                    </div>
                  ))}
                </div>
              ) : (
                <p className="text-sm text-muted-foreground">{t('devices:types.view.noMetrics')}</p>
              )}
            </TabsContent>
            <TabsContent value="downlink" className="mt-0">
              {deviceType.downlink?.commands && deviceType.downlink.commands.length > 0 ? (
                <div className="space-y-2">
                  {deviceType.downlink.commands.map((cmd, i) => (
                    <div key={i} className="rounded-md border p-3">
                      <p className="font-medium">{cmd.display_name || cmd.name}</p>
                      <p className="font-mono text-xs text-muted-foreground mt-1">{cmd.name}</p>
                      <p className="text-xs text-muted-foreground">Topic: {cmd.topic}</p>
                    </div>
                  ))}
                </div>
              ) : (
                <p className="text-sm text-muted-foreground">{t('devices:types.view.noCommands')}</p>
              )}
            </TabsContent>
          </Tabs>
        </div>
        <DialogFooter>
          <Button onClick={() => onOpenChange(false)}>{t('common:close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

// Edit Device Type Dialog
interface EditDeviceTypeDialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  deviceType: {
    typeId: string
    typeName: string
    typeDesc: string
    metrics: MetricDefinition[]
    commands: CommandDefinition[]
  } | null
  onEdit: (data: {
    typeId: string
    typeName: string
    typeDesc: string
    metrics: MetricDefinition[]
    commands: CommandDefinition[]
  }) => Promise<boolean>
  editing: boolean
}

export function EditDeviceTypeDialog({ open, onOpenChange, deviceType, onEdit, editing }: EditDeviceTypeDialogProps) {
  const { t } = useTranslation(['common', 'devices'])

  if (!deviceType) return null

  const [editTypeId, setEditTypeId] = useState(deviceType.typeId)
  const [editTypeName, setEditTypeName] = useState(deviceType.typeName)
  const [editTypeDesc, setEditTypeDesc] = useState(deviceType.typeDesc)
  const [editMetrics, setEditMetrics] = useState<MetricDefinition[]>(deviceType.metrics)
  const [editCommands, setEditCommands] = useState<CommandDefinition[]>(deviceType.commands)

  // Reset state when deviceType changes
  if (editTypeId !== deviceType.typeId) {
    setEditTypeId(deviceType.typeId)
    setEditTypeName(deviceType.typeName)
    setEditTypeDesc(deviceType.typeDesc)
    setEditMetrics(deviceType.metrics)
    setEditCommands(deviceType.commands)
  }

  const handleEdit = async () => {
    if (!editTypeId || !editTypeName) return

    const success = await onEdit({
      typeId: editTypeId,
      typeName: editTypeName,
      typeDesc: editTypeDesc,
      metrics: editMetrics,
      commands: editCommands,
    })

    if (success) {
      onOpenChange(false)
      toast({
        title: t('devices:types.save.success'),
        description: t('devices:types.save.updated', { name: editTypeName }),
      })
    } else {
      toast({
        title: t('devices:types.save.error'),
        description: t('devices:types.save.retry'),
        variant: "destructive",
      })
    }
  }

  const addMetric = () => {
    setEditMetrics([...editMetrics, {
      name: `metric_${editMetrics.length + 1}`,
      display_name: t('devices:types.edit.newMetric'),
      topic: `device/${editTypeId}/${editMetrics.length + 1}`,
      data_type: "Float",
    }])
  }

  const updateMetric = (index: number, field: keyof MetricDefinition, value: string | number | boolean) => {
    const newMetrics = [...editMetrics]
    newMetrics[index] = { ...newMetrics[index], [field]: value }
    setEditMetrics(newMetrics)
  }

  const removeMetric = (index: number) => {
    setEditMetrics(editMetrics.filter((_, i) => i !== index))
  }

  const addCommand = () => {
    setEditCommands([...editCommands, {
      name: `cmd_${editCommands.length + 1}`,
      display_name: t('devices:types.edit.newCommand'),
      topic: `device/${editTypeId}/command`,
      payload_template: '{"action": "${cmd}"}',
    }])
  }

  const updateCommand = (index: number, field: keyof CommandDefinition, value: string) => {
    const newCommands = [...editCommands]
    newCommands[index] = { ...newCommands[index], [field]: value }
    setEditCommands(newCommands)
  }

  const removeCommand = (index: number) => {
    setEditCommands(editCommands.filter((_, i) => i !== index))
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t('devices:types.edit.title')}</DialogTitle>
        </DialogHeader>
        <Tabs defaultValue="basic" className="w-full">
          <TabsList>
            <TabsTrigger value="basic">{t('devices:types.view.basicInfo')}</TabsTrigger>
            <TabsTrigger value="uplink">{t('devices:types.view.uplink')}</TabsTrigger>
            <TabsTrigger value="downlink">{t('devices:types.view.downlink')}</TabsTrigger>
          </TabsList>
          <TabsContent value="basic" className="mt-4 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="edit-type-id">{t('devices:types.edit.typeId')}</Label>
              <Input id="edit-type-id" value={editTypeId} disabled />
              <p className="text-xs text-muted-foreground">{t('devices:types.edit.idReadOnly')}</p>
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-type-name">{t('devices:types.edit.typeName')}</Label>
              <Input
                id="edit-type-name"
                value={editTypeName}
                onChange={(e) => setEditTypeName(e.target.value)}
                placeholder={t('devices:types.edit.typeNamePlaceholder')}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="edit-type-desc">{t('devices:types.edit.description')}</Label>
              <Textarea
                id="edit-type-desc"
                value={editTypeDesc}
                onChange={(e) => setEditTypeDesc(e.target.value)}
                placeholder={t('devices:types.edit.descPlaceholder')}
                rows={3}
              />
            </div>
          </TabsContent>
          <TabsContent value="uplink" className="mt-4">
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <h4 className="text-sm font-medium">{t('devices:types.edit.metrics', { count: editMetrics.length })}</h4>
                <Button onClick={addMetric} size="sm" variant="outline">
                  <Plus className="mr-2 h-4 w-4" />
                  {t('devices:types.edit.addMetric')}
                </Button>
              </div>
              {editMetrics.length === 0 ? (
                <p className="text-sm text-muted-foreground">{t('devices:types.edit.noMetrics')}</p>
              ) : (
                <div className="space-y-2">
                  {editMetrics.map((metric, i) => (
                    <div key={i} className="rounded-md border p-3 space-y-2">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-mono text-muted-foreground">#{i + 1}</span>
                        <Button variant="ghost" size="icon" onClick={() => removeMetric(i)}>
                          <Trash2 className="h-4 w-4 text-destructive" />
                        </Button>
                      </div>
                      <div className="grid grid-cols-2 gap-2">
                        <div className="space-y-1">
                          <Label className="text-xs">{t('devices:types.edit.displayName')}</Label>
                          <Input
                            value={metric.display_name}
                            onChange={(e) => updateMetric(i, "display_name", e.target.value)}
                            placeholder={t('devices:types.edit.displayNamePlaceholder')}
                            className="h-8"
                          />
                        </div>
                        <div className="space-y-1">
                          <Label className="text-xs">{t('devices:types.edit.fieldName')}</Label>
                          <Input
                            value={metric.name}
                            onChange={(e) => updateMetric(i, "name", e.target.value)}
                            placeholder={t('devices:types.edit.fieldNamePlaceholder')}
                            className="h-8"
                          />
                        </div>
                      </div>
                      <div className="grid grid-cols-2 gap-2">
                        <div className="space-y-1">
                          <Label className="text-xs">{t('devices:types.edit.dataType')}</Label>
                          <Select value={metric.data_type} onValueChange={(v) => updateMetric(i, "data_type", v)}>
                            <SelectTrigger className="h-8">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              <SelectItem value="String">String</SelectItem>
                              <SelectItem value="Integer">Integer</SelectItem>
                              <SelectItem value="Float">Float</SelectItem>
                              <SelectItem value="Boolean">Boolean</SelectItem>
                              <SelectItem value="Binary">Binary</SelectItem>
                            </SelectContent>
                          </Select>
                        </div>
                        <div className="space-y-1">
                          <Label className="text-xs">{t('devices:types.edit.unit')}</Label>
                          <Input
                            value={metric.unit || ""}
                            onChange={(e) => updateMetric(i, "unit", e.target.value)}
                            placeholder={t('devices:types.edit.unitPlaceholder')}
                            className="h-8"
                          />
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </TabsContent>
          <TabsContent value="downlink" className="mt-4">
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <h4 className="text-sm font-medium">{t('devices:types.edit.commands', { count: editCommands.length })}</h4>
                <Button onClick={addCommand} size="sm" variant="outline">
                  <Plus className="mr-2 h-4 w-4" />
                  {t('devices:types.edit.addCommand')}
                </Button>
              </div>
              {editCommands.length === 0 ? (
                <p className="text-sm text-muted-foreground">{t('devices:types.edit.noCommands')}</p>
              ) : (
                <div className="space-y-2">
                  {editCommands.map((cmd, i) => (
                    <div key={i} className="rounded-md border p-3 space-y-2">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-mono text-muted-foreground">#{i + 1}</span>
                        <Button variant="ghost" size="icon" onClick={() => removeCommand(i)}>
                          <Trash2 className="h-4 w-4 text-destructive" />
                        </Button>
                      </div>
                      <div className="grid grid-cols-2 gap-2">
                        <div className="space-y-1">
                          <Label className="text-xs">{t('devices:types.edit.displayName')}</Label>
                          <Input
                            value={cmd.display_name || ""}
                            onChange={(e) => updateCommand(i, "display_name", e.target.value)}
                            placeholder={t('devices:types.edit.cmdDisplayPlaceholder')}
                            className="h-8"
                          />
                        </div>
                        <div className="space-y-1">
                          <Label className="text-xs">{t('devices:types.edit.cmdName')}</Label>
                          <Input
                            value={cmd.name}
                            onChange={(e) => updateCommand(i, "name", e.target.value)}
                            placeholder={t('devices:types.edit.cmdNamePlaceholder')}
                            className="h-8"
                          />
                        </div>
                      </div>
                      <div className="space-y-1">
                        <Label className="text-xs">Topic</Label>
                        <Input
                          value={cmd.topic}
                          onChange={(e) => updateCommand(i, "topic", e.target.value)}
                          placeholder={t('devices:types.edit.topicPlaceholder')}
                          className="h-8"
                        />
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </TabsContent>
        </Tabs>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common:cancel')}
          </Button>
          <Button onClick={handleEdit} disabled={!editTypeId || !editTypeName || editing}>
            {editing ? t('devices:types.edit.saving') : t('common:save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
