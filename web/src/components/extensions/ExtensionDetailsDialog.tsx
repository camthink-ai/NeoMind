import { useState, useEffect } from "react"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogContentBody,
} from "@/components/ui/dialog"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Badge } from "@/components/ui/badge"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Switch } from "@/components/ui/switch"
import { Settings, FileCode, Info, Loader2, Terminal, Database, Zap, Save, RefreshCw, X } from "lucide-react"
import { useErrorHandler } from "@/hooks/useErrorHandler"
import { useStore } from "@/store"
import { formatTimestamp } from "@/lib/utils/format"
import { useToast } from "@/hooks/use-toast"
import { api } from "@/lib/api"
import type { Extension, ExtensionStatsDto, ExtensionConfigResponse, ExtensionConfigSchema } from "@/types"

interface ExtensionDetailsDialogProps {
  extension: Extension | null
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function ExtensionDetailsDialog({
  extension,
  open,
  onOpenChange,
}: ExtensionDetailsDialogProps) {
  const { handleError } = useErrorHandler()
  const { toast } = useToast()
  const getExtensionStats = useStore((state) => state.getExtensionStats)
  const getExtensionHealth = useStore((state) => state.getExtensionHealth)
  const fetchExtensions = useStore((state) => state.fetchExtensions)
  const reloadExtensionStore = useStore((state) => state.reloadExtension)

  const [stats, setStats] = useState<ExtensionStatsDto | null>(null)
  const [health, setHealth] = useState<{ healthy: boolean } | null>(null)
  const [loading, setLoading] = useState(false)
  const [reloading, setReloading] = useState(false)

  // Config state
  const [configData, setConfigData] = useState<ExtensionConfigResponse | null>(null)
  const [configLoading, setConfigLoading] = useState(false)
  const [configValues, setConfigValues] = useState<Record<string, unknown>>({})
  const [saving, setSaving] = useState(false)

  // Load extension details when dialog opens
  const loadDetails = async () => {
    if (!extension) return

    setLoading(true)
    try {
      const [statsData, healthData] = await Promise.all([
        getExtensionStats(extension.id),
        getExtensionHealth(extension.id),
      ])
      setStats(statsData)
      setHealth(healthData)
    } catch (error) {
      handleError(error, { operation: 'Load extension details', showToast: false })
    } finally {
      setLoading(false)
    }
  }

  // Load config when config tab is opened
  const loadConfig = async () => {
    if (!extension) return

    setConfigLoading(true)
    try {
      const response = await api.get<ExtensionConfigResponse>(`/extensions/${extension.id}/config`)
      setConfigData(response)
      setConfigValues(response.current_config || {})
    } catch (error) {
      handleError(error, { operation: 'Load extension config', showToast: false })
    } finally {
      setConfigLoading(false)
    }
  }

  // Save config
  const saveConfig = async () => {
    if (!extension) return

    setSaving(true)
    try {
      await api.put(`/extensions/${extension.id}/config`, configValues)
      toast({ title: "Configuration saved successfully" })

      // Reload extension to apply new config
      const reloadSuccess = await reloadExtensionStore(extension.id)
      if (reloadSuccess) {
        toast({ title: "Extension reloaded with new configuration" })
      } else {
        toast({ title: "Extension reloaded, but config may not have been applied", variant: "default" })
      }

      // Refresh extensions list and config
      await Promise.all([
        fetchExtensions(),
        loadConfig()
      ])
    } catch (error) {
      handleError(error, { operation: 'Save extension config' })
    } finally {
      setSaving(false)
    }
  }

  // Update config value
  const updateConfigValue = (name: string, value: unknown) => {
    setConfigValues(prev => ({ ...prev, [name]: value }))
  }

  // Reset state when dialog closes
  const handleClose = (open: boolean) => {
    if (!open) {
      setStats(null)
      setHealth(null)
      setConfigData(null)
      setConfigValues({})
    }
    onOpenChange(open)
  }

  // Reload extension
  const handleReloadExtension = async () => {
    if (!extension) return

    setReloading(true)
    try {
      const success = await reloadExtensionStore(extension.id)
      if (success) {
        toast({ title: "Extension reloaded successfully" })
        // Refresh details
        await loadDetails()
      } else {
        handleError(new Error("Failed to reload extension"), { operation: 'Reload extension' })
      }
    } catch (error) {
      handleError(error, { operation: 'Reload extension' })
    } finally {
      setReloading(false)
    }
  }

  // Render config input based on type
  const renderConfigInput = (paramName: string, param: any) => {
    const value = configValues[paramName] ?? param.default

    switch (param.type) {
      case 'boolean':
        return (
          <div key={paramName} className="flex items-center justify-between py-2 gap-2">
            <div className="flex-1 min-w-0">
              <Label className="text-sm font-medium break-words">{param.title || paramName}</Label>
              {param.description && (
                <p className="text-xs text-muted-foreground mt-1 break-words">{param.description}</p>
              )}
            </div>
            <Switch
              checked={value as boolean}
              onCheckedChange={(v) => updateConfigValue(paramName, v)}
            />
          </div>
        )

      case 'integer':
      case 'number':
        return (
          <div key={paramName} className="space-y-2 py-2">
            <Label htmlFor={paramName} className="text-sm font-medium">
              {param.title || paramName}
            </Label>
            <Input
              id={paramName}
              type="number"
              value={value as number ?? ''}
              onChange={(e) => updateConfigValue(paramName, Number(e.target.value))}
              min={param.minimum}
              max={param.maximum}
            />
            {param.description && (
              <p className="text-xs text-muted-foreground">{param.description}</p>
            )}
          </div>
        )

      case 'string':
      default:
        if (param.enum && param.enum.length > 0) {
          return (
            <div key={paramName} className="space-y-2 py-2">
              <Label htmlFor={paramName} className="text-sm font-medium">
                {param.title || paramName}
              </Label>
              <Select
                value={value as string ?? ''}
                onValueChange={(v) => updateConfigValue(paramName, v)}
              >
                <SelectTrigger id={paramName}>
                  <SelectValue placeholder="Select..." />
                </SelectTrigger>
                <SelectContent>
                  {param.enum.map((opt: string) => (
                    <SelectItem key={opt} value={opt}>
                      {opt}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {param.description && (
                <p className="text-xs text-muted-foreground">{param.description}</p>
              )}
            </div>
          )
        }

        return (
          <div key={paramName} className="space-y-2 py-2">
            <Label htmlFor={paramName} className="text-sm font-medium">
              {param.title || paramName}
            </Label>
            <Input
              id={paramName}
              type="text"
              value={value as string ?? ''}
              onChange={(e) => updateConfigValue(paramName, e.target.value)}
            />
            {param.description && (
              <p className="text-xs text-muted-foreground">{param.description}</p>
            )}
          </div>
        )
    }
  }

  // Check if config has any parameters
  const hasConfigParams = configData?.config_schema &&
    Object.keys(configData.config_schema.properties || {}).length > 0

  return (
    <Dialog open={open} onOpenChange={handleClose}>
      <DialogContent className="sm:max-w-[600px] sm:max-h-[90vh] flex flex-col overflow-hidden">
        <DialogHeader>
          <DialogTitle>Extension Details</DialogTitle>
          <DialogDescription>
            {extension ? `View ${extension.name} details and capabilities` : "Select an extension"}
          </DialogDescription>
        </DialogHeader>

        {!extension ? (
          <div className="py-8 text-center text-muted-foreground">
            No extension selected
          </div>
        ) : (
          <DialogContentBody className="flex-1 overflow-hidden pt-6 pb-4 px-4 sm:px-6">
            <Tabs defaultValue="info" className="w-full h-full flex flex-col overflow-hidden" onValueChange={(v) => {
              if (v === "info") loadDetails()
              if (v === "config") loadConfig()
            }}>
            <TabsList className="w-full inline-flex grid grid-cols-5 sm:grid-cols-5">
              <TabsTrigger value="info" className="gap-1 sm:gap-2">
                <Info className="h-3.5 w-3.5 sm:h-4 sm:w-4 sm:mr-0" />
                <span className="hidden sm:inline">Info</span>
                <span className="sm:hidden text-xs">Info</span>
              </TabsTrigger>
              <TabsTrigger value="capabilities" className="gap-1 sm:gap-2">
                <Zap className="h-3.5 w-3.5 sm:h-4 sm:w-4 sm:mr-0" />
                <span className="hidden sm:inline">Capabilities</span>
                <span className="sm:hidden text-xs">Caps</span>
              </TabsTrigger>
              <TabsTrigger value="config" className="gap-1 sm:gap-2">
                <Settings className="h-3.5 w-3.5 sm:h-4 sm:w-4 sm:mr-0" />
                <span className="hidden sm:inline">Config</span>
                <span className="sm:hidden text-xs">Cfg</span>
              </TabsTrigger>
              <TabsTrigger value="stats" className="gap-1 sm:gap-2">
                <Terminal className="h-3.5 w-3.5 sm:h-4 sm:w-4 sm:mr-0" />
                <span className="hidden sm:inline">Stats</span>
                <span className="sm:hidden text-xs">Stats</span>
              </TabsTrigger>
              <TabsTrigger value="file" className="gap-1 sm:gap-2">
                <FileCode className="h-3.5 w-3.5 sm:h-4 sm:w-4 sm:mr-0" />
                <span className="hidden sm:inline">File</span>
                <span className="sm:hidden text-xs">File</span>
              </TabsTrigger>
            </TabsList>

            {/* Info Tab */}
            <TabsContent value="info" className="flex-1 overflow-y-auto mt-4 space-y-4">
              <div className="flex justify-end pb-2">
                <Button
                  size="sm"
                  variant="outline"
                  onClick={handleReloadExtension}
                  disabled={reloading}
                >
                  <RefreshCw className={`h-4 w-4 mr-1 ${reloading ? 'animate-spin' : ''}`} />
                  {reloading ? 'Reloading...' : 'Reload Extension'}
                </Button>
              </div>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 sm:gap-4">
                <div>
                  <Label className="text-muted-foreground text-xs">ID</Label>
                  <p className="text-sm font-mono break-all">{extension.id}</p>
                </div>
                <div>
                  <Label className="text-muted-foreground text-xs">Name</Label>
                  <p className="text-sm break-words">{extension.name}</p>
                </div>
                <div>
                  <Label className="text-muted-foreground text-xs">Version</Label>
                  <p className="text-sm">{extension.version}</p>
                </div>
                <div>
                  <Label className="text-muted-foreground text-xs">State</Label>
                  <Badge variant={extension.state === "Running" ? "default" : "secondary"}>
                    {extension.state}
                  </Badge>
                </div>
                <div>
                  <Label className="text-muted-foreground text-xs">Commands</Label>
                  <p className="text-sm">{extension.commands?.length ?? 0}</p>
                </div>
                <div>
                  <Label className="text-muted-foreground text-xs">Metrics</Label>
                  <p className="text-sm">{extension.metrics?.length ?? 0}</p>
                </div>
              </div>

              {extension.description && (
                <div>
                  <Label className="text-muted-foreground text-xs">Description</Label>
                  <p className="text-sm break-words">{extension.description}</p>
                </div>
              )}

              {extension.author && (
                <div>
                  <Label className="text-muted-foreground text-xs">Author</Label>
                  <p className="text-sm break-words">{extension.author}</p>
                </div>
              )}

              {health && (
                <div>
                  <Label className="text-muted-foreground text-xs">Health Status</Label>
                  <Badge variant={health.healthy ? "default" : "destructive"}>
                    {health.healthy ? "Healthy" : "Unhealthy"}
                  </Badge>
                </div>
              )}
            </TabsContent>

            {/* Capabilities Tab - Commands and Metrics */}
            <TabsContent value="capabilities" className="flex-1 overflow-y-auto mt-4 space-y-4">
              {/* Commands Section */}
              {extension.commands && extension.commands.length > 0 && (
                <Accordion type="single" collapsible defaultValue="commands" className="w-full">
                  <AccordionItem value="commands">
                    <AccordionTrigger className="py-3">
                      <span className="flex items-center gap-2 text-sm font-medium">
                        <Terminal className="h-4 w-4" />
                        Commands ({extension.commands.length})
                      </span>
                    </AccordionTrigger>
                    <AccordionContent className="pt-2 pb-4 space-y-2 max-h-[40vh] overflow-y-auto">
                      {extension.commands.map((command) => (
                        <div
                          key={command.id}
                          className="p-3 rounded-lg border bg-muted/30 space-y-2"
                        >
                          <div className="flex flex-wrap items-center gap-2">
                            <Badge variant="outline" className="text-xs break-all">
                              {command.id}
                            </Badge>
                            <span className="font-medium text-sm break-words">{command.display_name}</span>
                          </div>
                          {command.description && (
                            <p className="text-xs text-muted-foreground break-words">{command.description}</p>
                          )}
                        </div>
                      ))}
                    </AccordionContent>
                  </AccordionItem>
                </Accordion>
              )}

              {/* Metrics Section */}
              {extension.metrics && extension.metrics.length > 0 && (
                <Accordion type="single" collapsible defaultValue="metrics" className="w-full">
                  <AccordionItem value="metrics">
                    <AccordionTrigger className="py-3">
                      <span className="flex items-center gap-2 text-sm font-medium">
                        <Database className="h-4 w-4" />
                        Metrics ({extension.metrics.length})
                      </span>
                    </AccordionTrigger>
                    <AccordionContent className="pt-2 pb-4 max-h-[40vh] overflow-y-auto">
                      <div className="grid grid-cols-1 gap-2">
                        {extension.metrics.map((metric) => (
                          <div
                            key={metric.name}
                            className="p-3 rounded-lg border bg-muted/20 flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2"
                          >
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="font-medium text-sm break-words">{metric.display_name}</span>
                              <Badge variant="outline" className="text-xs break-all">
                                {metric.name}
                              </Badge>
                            </div>
                            <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
                              <span>{metric.data_type}</span>
                              {metric.unit && <span>({metric.unit})</span>}
                              {metric.min !== undefined && metric.max !== undefined && (
                                <span>[{metric.min} - {metric.max}]</span>
                              )}
                              {metric.required && (
                                <Badge variant="secondary" className="text-xs">required</Badge>
                              )}
                            </div>
                          </div>
                        ))}
                      </div>
                    </AccordionContent>
                  </AccordionItem>
                </Accordion>
              )}

              {/* Empty State */}
              {(!extension.commands || extension.commands.length === 0) &&
               (!extension.metrics || extension.metrics.length === 0) && (
                <div className="text-center py-8 text-muted-foreground text-sm">
                  No commands or metrics available for this extension
                </div>
              )}
            </TabsContent>

            {/* Config Tab */}
            <TabsContent value="config" className="flex-1 overflow-y-auto mt-4 space-y-4">
              {configLoading ? (
                <div className="flex justify-center py-8">
                  <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
                </div>
              ) : hasConfigParams ? (
                <div className="space-y-4">
                  <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-3 pb-2 border-b">
                    <div>
                      <h3 className="text-sm font-medium">Extension Configuration</h3>
                      <p className="text-xs text-muted-foreground">
                        Configure {extension.name} settings
                      </p>
                    </div>
                    <div className="flex gap-2 sm:gap-2">
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={loadConfig}
                        disabled={saving}
                      >
                        <RefreshCw className="h-4 w-4 mr-1" />
                        Reload
                      </Button>
                      <Button
                        size="sm"
                        onClick={saveConfig}
                        disabled={saving}
                      >
                        <Save className="h-4 w-4 mr-1" />
                        {saving ? 'Saving...' : 'Save & Reload'}
                      </Button>
                    </div>
                  </div>

                  <div className="space-y-1">
                    {Object.entries(configData!.config_schema.properties || {}).map(([name, param]) =>
                      renderConfigInput(name, param)
                    )}
                  </div>

                  <div className="pt-4 border-t">
                    <p className="text-xs text-muted-foreground">
                      Changes will be saved and the extension will be reloaded to apply the new configuration.
                    </p>
                  </div>
                </div>
              ) : (
                <div className="text-center py-8 text-muted-foreground text-sm">
                  <Settings className="h-8 w-8 mx-auto mb-2 opacity-50" />
                  <p>This extension does not have configurable options.</p>
                </div>
              )}
            </TabsContent>

            {/* Stats Tab */}
            <TabsContent value="stats" className="flex-1 overflow-y-auto mt-4 space-y-4">
              {loading ? (
                <div className="flex justify-center py-8">
                  <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
                </div>
              ) : stats ? (
                <div className="grid grid-cols-2 sm:grid-cols-2 gap-3 sm:gap-4">
                  <div className="border rounded-lg p-3">
                    <p className="text-xs text-muted-foreground">Start Count</p>
                    <p className="text-2xl font-semibold">{stats.start_count ?? 0}</p>
                  </div>
                  <div className="border rounded-lg p-3">
                    <p className="text-xs text-muted-foreground">Stop Count</p>
                    <p className="text-2xl font-semibold">{stats.stop_count ?? 0}</p>
                  </div>
                  <div className="border rounded-lg p-3">
                    <p className="text-xs text-muted-foreground">Error Count</p>
                    <p className="text-2xl font-semibold">{stats.error_count ?? 0}</p>
                  </div>
                  {stats.last_error && (
                    <div className="border rounded-lg p-3 col-span-2">
                      <p className="text-xs text-muted-foreground">Last Error</p>
                      <p className="text-sm text-destructive break-words">{stats.last_error}</p>
                    </div>
                  )}
                </div>
              ) : (
                <div className="text-center py-8 text-muted-foreground text-sm">
                  Select Info tab to load extension details
                </div>
              )}
            </TabsContent>

            {/* File Info Tab */}
            <TabsContent value="file" className="flex-1 overflow-y-auto mt-4 space-y-4">
              <div>
                <Label className="text-muted-foreground text-xs">File Path</Label>
                <p className="text-sm font-mono break-all">{extension.file_path || "N/A"}</p>
              </div>
              <div>
                <Label className="text-muted-foreground text-xs">Version</Label>
                <p className="text-sm">{extension.version}</p>
              </div>
              {extension.loaded_at && (
                <div>
                  <Label className="text-muted-foreground text-xs">Loaded At</Label>
                  <p className="text-sm break-all">{formatTimestamp(extension.loaded_at)}</p>
                </div>
              )}
            </TabsContent>
          </Tabs>
          </DialogContentBody>
        )}
      </DialogContent>
    </Dialog>
  )
}
