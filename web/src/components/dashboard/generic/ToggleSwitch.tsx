/**
 * Toggle Switch Component
 *
 * Command trigger button for sending device commands.
 * Displays actual device state from telemetry (read-only).
 * Clicking opens confirmation dialog with parameter form (if command has parameters).
 * State updates only when device reports back via telemetry.
 *
 * This is a "fire and forget" command sender with confirmation, not a stateful toggle.
 */

import { Power, Lightbulb, Fan, Lock, Info } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useState, useEffect, useCallback } from 'react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { Skeleton } from '@/components/ui/skeleton'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { indicatorFontWeight } from '@/design-system/tokens/indicator'
import type { DataSource } from '@/types/dashboard'
import type { ParameterDefinition } from '@/types'
import { api } from '@/lib/api'
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { ScrollArea } from '@/components/ui/scroll-area'

export interface ToggleSwitchProps {
  // Command data source (required)
  dataSource?: DataSource

  // Display
  title?: string
  label?: string  // Alternative to title, displayed in style section
  size?: 'sm' | 'md' | 'lg'

  // Initial state for display before command response
  initialState?: boolean

  // Edit mode - disable click when editing dashboard
  editMode?: boolean

  disabled?: boolean
  className?: string
}

// Get icon based on title
function getIconForTitle(title?: string): React.ComponentType<{ className?: string }> {
  if (!title) return Power
  const lower = title.toLowerCase()
  if (lower.includes('light') || lower.includes('lamp')) return Lightbulb
  if (lower.includes('fan')) return Fan
  if (lower.includes('lock')) return Lock
  return Power
}

export function ToggleSwitch({
  dataSource,
  title,
  size = 'md',
  initialState = false,
  editMode = false,
  disabled = false,
  className,
}: ToggleSwitchProps) {
  const { t } = useTranslation('dashboardComponents')
  const { data, loading, sendCommand, sending } = useDataSource<boolean>(dataSource, {
    fallback: initialState,
  })

  // Dialog and parameter states
  const [dialogOpen, setDialogOpen] = useState(false)
  const [commandParams, setCommandParams] = useState<Record<string, unknown>>({})
  const [parameterDefinitions, setParameterDefinitions] = useState<ParameterDefinition[]>([])
  const [commandDisplayName, setCommandDisplayName] = useState<string>('')
  const [loadingParams, setLoadingParams] = useState(false)
  const [currentValue, setCurrentValue] = useState<boolean | null>(null)

  // Display current device state from telemetry (read-only display)
  const checked = data ?? initialState
  const hasCommand = dataSource?.type === 'command'
  const deviceId = dataSource?.deviceId
  const commandName = dataSource?.command || 'setValue'

  // Fetch command parameters when dialog opens
  useEffect(() => {
    if (dialogOpen && deviceId) {
      const fetchCommandParams = async () => {
        setLoadingParams(true)
        try {
          // Get device current state which includes command definitions
          const response = await api.getDeviceCurrent(deviceId)
          const commandDef = response.commands?.find(c => c.name === commandName)
          if (commandDef) {
            setParameterDefinitions(commandDef.parameters || [])
            setCommandDisplayName(commandDef.display_name || commandDef.name)

            // Initialize parameters with defaults
            const defaults: Record<string, unknown> = {}
            commandDef.parameters?.forEach(param => {
              if (param.default_value !== undefined) {
                defaults[param.name] = param.default_value
              } else if (param.data_type === 'integer' || param.data_type === 'float') {
                defaults[param.name] = 0
              } else if (param.data_type === 'boolean') {
                defaults[param.name] = false
              } else if (param.data_type === 'string') {
                defaults[param.name] = ''
              }
            })
            setCommandParams(defaults)
          }
        } catch (error) {
          console.error('[ToggleSwitch] Failed to fetch command parameters:', error)
        } finally {
          setLoadingParams(false)
        }
      }
      fetchCommandParams()
    }
  }, [dialogOpen, deviceId, commandName])

  const handleClick = async () => {
    // Don't execute click in edit mode - allows dragging
    if (editMode) return
    if (disabled || loading || sending || !hasCommand) return

    // If command has parameters or needs confirmation, show dialog
    // Otherwise send command directly with toggled value
    setCurrentValue(!checked)

    // Check if we need to show the dialog (either for confirmation or parameters)
    // For toggle switch, we always show confirmation for safety
    setDialogOpen(true)
  }

  const handleConfirmSend = async () => {
    if (!sendCommand) return

    setDialogOpen(false)

    // Combine value toggle with user parameters
    const finalParams = {
      ...commandParams,
      value: currentValue,
    }

    // Send command with combined parameters
    await sendCommand(finalParams.value)
  }

  const updateParameter = (name: string, value: unknown) => {
    setCommandParams(prev => ({
      ...prev,
      [name]: value,
    }))
  }

  // Render parameter input based on data type
  const renderParameterInput = (param: ParameterDefinition) => {
    const value = commandParams[param.name]

    if (param.allowed_values && param.allowed_values.length > 0) {
      return (
        <Select
          value={String(value ?? '')}
          onValueChange={(v) => updateParameter(param.name, v)}
        >
          <SelectTrigger>
            <SelectValue placeholder={t('toggleSwitch.selectValue')} />
          </SelectTrigger>
          <SelectContent>
            {param.allowed_values.map((allowed, idx) => (
              <SelectItem key={idx} value={String(allowed)}>
                {String(allowed)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )
    }

    if (param.data_type === 'boolean') {
      return (
        <Select
          value={String(value ?? 'false')}
          onValueChange={(v) => updateParameter(param.name, v === 'true')}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="true">{t('toggleSwitch.yes')}</SelectItem>
            <SelectItem value="false">{t('toggleSwitch.no')}</SelectItem>
          </SelectContent>
        </Select>
      )
    }

    if (param.data_type === 'integer' || param.data_type === 'float') {
      return (
        <div className="flex items-center gap-2">
          <Input
            type="number"
            value={String(value ?? 0)}
            onChange={(e) =>
              updateParameter(
                param.name,
                param.data_type === 'integer'
                  ? parseInt(e.target.value) || 0
                  : parseFloat(e.target.value) || 0
              )
            }
            min={param.min}
            max={param.max}
            step={param.data_type === 'integer' ? 1 : 0.1}
          />
          {param.unit && <span className="text-xs text-muted-foreground">{param.unit}</span>}
        </div>
      )
    }

    return (
      <Input
        value={String(value ?? '')}
        onChange={(e) => updateParameter(param.name, e.target.value)}
        placeholder={param.display_name || param.name}
      />
    )
  }

  const config = dashboardComponentSize[size]
  const Icon = getIconForTitle(title)

  // Loading state
  if (loading) {
    return (
      <div className={cn(dashboardCardBase, 'flex-row items-center', config.contentGap, config.padding, className)}>
        <Skeleton className={cn(config.iconContainer, 'rounded-full')} />
        <Skeleton className={cn('h-4 w-20 rounded')} />
      </div>
    )
  }

  return (
    <>
      <button
        onClick={handleClick}
        disabled={disabled || loading || sending || !hasCommand || editMode}
        className={cn(
          dashboardCardBase,
          'flex-row items-center',
          config.contentGap,
          config.padding,
          'transition-all duration-200',
          !disabled && !sending && hasCommand && !editMode && 'hover:bg-accent/50',
          (disabled || sending || !hasCommand || editMode) && 'opacity-50 cursor-not-allowed',
          editMode && 'pointer-events-none',  // Allow dragging in edit mode
          className
        )}
      >
        {/* Icon Section - left side */}
        <div className={cn(
          'flex items-center justify-center shrink-0 rounded-full transition-all duration-300',
          config.iconContainer,
          checked
            ? 'bg-primary text-primary-foreground shadow-md'
            : 'bg-muted/50 text-muted-foreground'
        )}>
          <Icon className={cn(config.iconSize, checked ? 'opacity-100' : 'opacity-50')} />
        </div>

        {/* Title section - right side */}
        <div className="flex flex-col min-w-0 flex-1 text-left">
          {title ? (
            <span className={cn(indicatorFontWeight.title, 'text-foreground truncate', config.titleText)}>
              {title}
            </span>
          ) : (
            <span className={cn(indicatorFontWeight.title, 'text-foreground', config.titleText)}>
              {checked ? t('toggleSwitch.enabled') : t('toggleSwitch.disabled')}
            </span>
          )}
          {title && (
            <span className={cn(indicatorFontWeight.label, 'text-muted-foreground', config.labelText)}>
              {checked ? t('toggleSwitch.enabled') : t('toggleSwitch.disabled')}
            </span>
          )}
        </div>

        {/* Sending indicator */}
        {sending && (
          <span className="absolute top-3 right-3 w-2 h-2 rounded-full bg-amber-500 animate-pulse" />
        )}

        {/* Warning: no command configured */}
        {!hasCommand && (
          <span className="absolute top-3 right-3 w-2 h-2 rounded-full bg-orange-500" title={t('toggleSwitch.noCommandSource')} />
        )}
      </button>

      {/* Confirmation and Parameter Dialog */}
      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Icon className="h-5 w-5" />
              {title || commandDisplayName || t('toggleSwitch.confirmCommand')}
            </DialogTitle>
            <DialogDescription>
              {t('toggleSwitch.confirmDescription')}
            </DialogDescription>
          </DialogHeader>

          <ScrollArea className="max-h-[60vh] pr-4">
            <div className="space-y-4 py-4">
              {/* Current state info */}
              <div className="flex items-center gap-2 p-3 rounded-lg bg-muted/50">
                <Info className="h-4 w-4 text-muted-foreground" />
                <span className="text-sm">
                  {t('toggleSwitch.currentState')}: <span className={cn('font-semibold', checked ? 'text-green-600' : 'text-muted-foreground')}>
                    {checked ? t('toggleSwitch.enabled') : t('toggleSwitch.disabled')}
                  </span>
                                   {' → '}
                  <span className={cn('font-semibold', !checked ? 'text-green-600' : 'text-muted-foreground')}>
                    {!checked ? t('toggleSwitch.enabled') : t('toggleSwitch.disabled')}
                  </span>
                </span>
              </div>

              {/* Parameter inputs */}
              {!loadingParams && parameterDefinitions.length > 0 && (
                <div className="space-y-4">
                  <div className="text-sm font-medium">{t('toggleSwitch.parameters')}</div>
                  {parameterDefinitions.map(param => (
                    <div key={param.name} className="space-y-2">
                      <div className="flex items-center justify-between">
                        <Label className="text-sm">
                          {param.display_name || param.name}
                          {param.required && <span className="text-red-500 ml-1">*</span>}
                        </Label>
                        {(param.min !== undefined || param.max !== undefined) && (
                          <span className="text-xs text-muted-foreground">
                            {param.min !== undefined && `${t('range.min')} ${param.min}`}
                            {param.min !== undefined && param.max !== undefined && ' | '}
                            {param.max !== undefined && `${t('range.max')} ${param.max}`}
                          </span>
                        )}
                      </div>
                      {renderParameterInput(param)}
                      {param.help_text && (
                        <p className="text-xs text-muted-foreground">{param.help_text}</p>
                      )}
                    </div>
                  ))}
                </div>
              )}

              {/* Loading params */}
              {loadingParams && (
                <div className="text-sm text-muted-foreground text-center py-4">
                  {t('toggleSwitch.loadingParameters')}
                </div>
              )}
            </div>
          </ScrollArea>

          <DialogFooter className="gap-2 sm:gap-0">
            <Button
              variant="outline"
              onClick={() => setDialogOpen(false)}
              disabled={sending}
            >
              {t('toggleSwitch.cancel')}
            </Button>
            <Button
              onClick={handleConfirmSend}
              disabled={sending || loadingParams}
            >
              {sending ? t('toggleSwitch.sending') : t('toggleSwitch.confirm')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
