import { useState, useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  CardFooter,
} from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { MoreVertical, Edit, Trash2, TestTube, Wifi, Loader2, Server, Home, Network, BrainCircuit, Sparkles, Gem, Zap } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { PluginUISchema, PluginInstance } from '@/types/plugin-schema'
import { SchemaConfigForm } from './SchemaConfigForm'

// ============================================================================
// Icons
// ============================================================================

const ICONS: Record<string, React.ComponentType<{ className?: string }>> = {
  Server,
  Home,
  Network,
  BrainCircuit,
  Sparkles,
  Gem,
  Zap,
  Wifi,
}

function getIcon(iconName?: string) {
  if (!iconName) return Server
  return ICONS[iconName] || Server
}

// ============================================================================
// Plugin Instance Card
// ============================================================================

interface SchemaPluginCardProps {
  instance: PluginInstance
  onEdit?: () => void
  onDelete?: () => void
  onTest?: () => void
  onToggle?: () => void
  onViewDevices?: () => void
}

export function SchemaPluginCard({
  instance,
  onEdit,
  onDelete,
  onTest,
  onToggle,
  onViewDevices,
}: SchemaPluginCardProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const { schema, config, state, enabled } = instance
  const isRunning = state === 'running'

  const getConfigDisplay = () => {
    if (schema.listTemplate?.showConfig && schema.listTemplate.configDisplay) {
      return schema.listTemplate.configDisplay(config)
    }
    return ''
  }

  return (
    <Card
      className={cn(
        "transition-all duration-200 hover:shadow-md",
        isRunning && "border-green-500"
      )}
    >
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="flex-1">
            <div className="flex items-center gap-2 mb-1">
              <CardTitle className="text-base">{config.name as string || schema.name}</CardTitle>
              {schema.builtin && (
                <Badge variant="outline" className="text-xs">{t('plugins:builtin')}</Badge>
              )}
              <Badge variant={isRunning ? "default" : "secondary"} className="text-xs">
                {isRunning ? t('plugins:running') : t('plugins:stopped')}
              </Badge>
            </div>
            <CardDescription className="flex items-center gap-2 text-xs">
              <span>{schema.description}</span>
              {getConfigDisplay() && (
                <>
                  <span>•</span>
                  <span className="text-muted-foreground">{getConfigDisplay()}</span>
                </>
              )}
            </CardDescription>
          </div>

          {/* Action Menu */}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="sm" className="h-8 w-8 p-0">
                <MoreVertical className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {onEdit && (
                <DropdownMenuItem onClick={onEdit}>
                  <Edit className="mr-2 h-4 w-4" />
                  {t('plugins:edit')}
                </DropdownMenuItem>
              )}
              {onTest && (
                <DropdownMenuItem onClick={onTest}>
                  <TestTube className="mr-2 h-4 w-4" />
                  {t('plugins:llm.testConnection')}
                </DropdownMenuItem>
              )}
              <DropdownMenuSeparator />
              {onViewDevices && instance.deviceCount !== undefined && (
                <DropdownMenuItem onClick={onViewDevices}>
                  <Wifi className="mr-2 h-4 w-4" />
                  {t('plugins:viewDevicesWithCount', { count: instance.deviceCount })}
                </DropdownMenuItem>
              )}
              {onDelete && !schema.builtin && (
                <>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem onClick={onDelete} className="text-destructive focus:text-destructive">
                    <Trash2 className="mr-2 h-4 w-4" />
                    {t('plugins:delete')}
                  </DropdownMenuItem>
                </>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </CardHeader>

      <CardContent className="pb-3">
        <div className="space-y-2 text-sm">
          {/* Device Count */}
          {instance.deviceCount !== undefined && (
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">{t('plugins:deviceCount')}:</span>
              <span className="font-medium">{instance.deviceCount}</span>
            </div>
          )}
          {/* Connection Status */}
          {instance.connected !== undefined && (
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">{t('plugins:llm.endpoint')}:</span>
              <span className={instance.connected ? "text-green-600 font-medium" : "text-muted-foreground font-medium"}>
                {instance.connected ? t('plugins:connected') : t('plugins:disconnected')}
              </span>
            </div>
          )}
          {/* Config display */}
          {getConfigDisplay() && (
            <div className="flex items-center justify-between">
              <span className="text-muted-foreground">{t('plugins:pluginConfig')}:</span>
              <span className="font-mono text-xs">{getConfigDisplay()}</span>
            </div>
          )}
        </div>
      </CardContent>

      <CardFooter className="pt-3 border-t justify-between">
        {onToggle && !schema.builtin && (
          <div className="flex items-center gap-2">
            <Switch
              checked={enabled}
              onCheckedChange={onToggle}
              disabled={isRunning}
            />
            <span className="text-xs text-muted-foreground">
              {enabled ? t('plugins:enabled') : t('plugins:disabled')}
            </span>
          </div>
        )}
        {onViewDevices && instance.deviceCount !== undefined && (
          <Button variant="outline" size="sm" onClick={onViewDevices}>
            <Wifi className="mr-2 h-4 w-4" />
            {t('plugins:viewDevices')}
          </Button>
        )}
      </CardFooter>
    </Card>
  )
}

// ============================================================================
// Plugin Type Card (for schema list view)
// ============================================================================

interface SchemaPluginTypeCardProps {
  schema: PluginUISchema
  instanceCount: number
  isActive: boolean
  onClick: () => void
}

export function SchemaPluginTypeCard({
  schema,
  instanceCount,
  isActive,
  onClick,
}: SchemaPluginTypeCardProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const getIconBg = () => {
    switch (schema.category) {
      case 'ai': return 'bg-green-100 text-green-700 dark:bg-green-900/20 dark:text-green-400'
      case 'devices': return 'bg-blue-100 text-blue-700 dark:bg-blue-900/20 dark:text-blue-400'
      case 'storage': return 'bg-purple-100 text-purple-700 dark:bg-purple-900/20 dark:text-purple-400'
      case 'notify': return 'bg-orange-100 text-orange-700 dark:bg-orange-900/20 dark:text-orange-400'
      default: return 'bg-gray-100 text-gray-700 dark:bg-gray-900/20 dark:text-gray-400'
    }
  }

  const Icon = getIcon(schema.icon)

  return (
    <Card
      className={cn(
        "transition-all duration-200 cursor-pointer hover:shadow-md",
        isActive && "border-green-500 border-2"
      )}
      onClick={onClick}
    >
      <CardHeader className="pb-3">
        <div className={cn("flex items-center justify-center w-12 h-12 rounded-lg", getIconBg())}>
          <Icon className="h-6 w-6" />
        </div>
        <CardTitle className="text-base mt-3">{schema.name}</CardTitle>
        <CardDescription className="mt-1 text-xs">
          {schema.description}
        </CardDescription>
      </CardHeader>
      <CardContent className="text-sm">
        <div className="flex justify-between items-center">
          <span className="text-muted-foreground">{t('plugins:status')}:</span>
          <span className={isActive ? "text-green-600 font-medium" : "text-muted-foreground font-medium"}>
            {isActive ? t('plugins:running') : instanceCount > 0 ? t('plugins:configured') : t('plugins:notConfigured')}
          </span>
        </div>
        {instanceCount > 0 && (
          <div className="flex justify-between items-center mt-2">
            <span className="text-muted-foreground">{t('plugins:instanceCount')}:</span>
            <span className="font-medium">{instanceCount}{t('plugins:count')}</span>
          </div>
        )}
      </CardContent>
    </Card>
  )
}

// ============================================================================
// Schema Config Dialog
// ============================================================================

interface SchemaPluginConfigDialogProps {
  open: boolean
  onClose: () => void
  schema: PluginUISchema
  instance?: PluginInstance
  saving?: boolean
  onSave: (config: Record<string, unknown>) => Promise<void>
}

export function SchemaPluginConfigDialog({
  open,
  onClose,
  schema,
  instance,
  saving = false,
  onSave,
}: SchemaPluginConfigDialogProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const [config, setConfig] = useState<Record<string, unknown>>({})

  // Initialize config when dialog opens with instance
  useEffect(() => {
    if (open) {
      if (instance?.config) {
        setConfig(instance.config)
      } else {
        // Initialize with defaults from schema
        const defaults: Record<string, unknown> = {}
        Object.entries(schema.fields).forEach(([key, field]) => {
          if (field.default !== undefined) {
            defaults[key] = field.default
          }
        })
        setConfig(defaults)
      }
    }
  }, [open, instance, schema.fields])

  const handleSubmit = async () => {
    await onSave(config)
    onClose()
  }

  const Icon = getIcon(schema.icon)

  return (
    <Dialog open={open} onOpenChange={onClose}>
      <DialogContent className="max-w-md max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <div className={cn(
              "p-2 rounded-lg",
              schema.category === 'ai' && "bg-emerald-100 text-emerald-700 dark:bg-emerald-900/20 dark:text-emerald-400",
              schema.category === 'devices' && "bg-blue-100 text-blue-700 dark:bg-blue-900/20 dark:text-blue-400",
            )}>
              <Icon className="h-5 w-5" />
            </div>
            {instance ? t('plugins:edit') : t('plugins:add')} {schema.name}
          </DialogTitle>
          <DialogDescription>
            {schema.description}
          </DialogDescription>
        </DialogHeader>

        <SchemaConfigForm
          schema={schema}
          config={config}
          onChange={setConfig}
        />

        <DialogFooter>
          <Button type="button" variant="outline" onClick={onClose} disabled={saving}>
            {t('common:cancel')}
          </Button>
          <Button onClick={handleSubmit} disabled={saving}>
            {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            {saving ? t('plugins:saving') : t('plugins:save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
