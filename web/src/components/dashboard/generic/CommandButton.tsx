/**
 * Command Button Component
 *
 * A button that opens a command form dialog when clicked.
 * Supports both device commands and extension commands.
 * Shows command parameters for user input before sending.
 *
 * This is NOT a toggle switch - it's a command trigger button.
 */

import { Power, Lightbulb, Fan, Lock, Play, ChevronRight } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useState, useCallback, useEffect } from 'react'
import { cn } from '@/lib/utils'
import { findDevice } from '@/lib/deviceUtils'
import type { DataSource } from '@/types/dashboard'
import { getSourceId } from '@/types/dashboard'
import type { ParameterDefinition, ParameterGroup } from '@/types'
import { api } from '@/lib/api'
import { UnifiedFormDialog } from '@/components/dialog/UnifiedFormDialog'
import { ParameterForm } from '@/components/devices/ParameterForm'
import { seedCommandDefaults } from '@/components/devices/seedCommandDefaults'
import { useToast } from '@/hooks/use-toast'
import { dashboardCardBase, dashboardComponentSize } from '@/design-system/tokens/size'
import { useStore } from '@/store'

export interface CommandButtonProps {
  // Command data source
  dataSource?: DataSource

  // Display
  title?: string
  label?: string
  size?: 'sm' | 'md' | 'lg'

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

export function CommandButton({
  dataSource,
  title,
  size = 'md',
  editMode = false,
  disabled = false,
  className,
}: CommandButtonProps) {
  const { t } = useTranslation('dashboardComponents')
  const { toast } = useToast()

  // Dialog and parameter states
  const [dialogOpen, setDialogOpen] = useState(false)
  const [commandParams, setCommandParams] = useState<Record<string, unknown>>({})
  const [parameterDefinitions, setParameterDefinitions] = useState<ParameterDefinition[]>([])
  const [parameterGroups, setParameterGroups] = useState<ParameterGroup[]>([])
  const [commandDisplayName, setCommandDisplayName] = useState<string>('')
  const [loadingParams, setLoadingParams] = useState(false)
  const [sending, setSending] = useState(false)

  // Check data source type
  const isDeviceCommand = dataSource?.type === 'command'
  const isExtensionCommand = dataSource?.type === 'extension-command'
  const hasCommand = isDeviceCommand || isExtensionCommand

  const deviceId = isDeviceCommand ? getSourceId(dataSource!) : undefined
  const commandName = isDeviceCommand ? (dataSource?.command || 'setValue') : undefined

  const extensionId = isExtensionCommand ? dataSource?.extensionId : undefined
  const extensionCommand = isExtensionCommand ? dataSource?.extensionCommand : undefined

  // Fetch command parameters when dialog opens
  useEffect(() => {
    if (!dialogOpen) return

    const fetchCommandParams = async () => {
      setLoadingParams(true)
      try {
        if (isDeviceCommand && deviceId) {
          // Get device current state which includes command definitions
          const response = await api.getDeviceCurrent(deviceId)
          const commandDef = response.commands?.find((c: any) => c.name === commandName)
          if (commandDef) {
            const params: ParameterDefinition[] = commandDef.parameters || []
            setParameterDefinitions(params)
            setParameterGroups(commandDef.parameter_groups || [])
            setCommandDisplayName(commandDef.display_name || commandDef.name)
            // Seed defaults (auto-generates request_id, applies declared defaults)
            setCommandParams(seedCommandDefaults(params))
          }
        } else if (isExtensionCommand && extensionId && extensionCommand) {
          // Get extension command parameters
          const extensions = await api.listExtensions()
          const ext = extensions.find(e => e.id === extensionId)
          if (ext) {
            const cmd = ext.commands?.find(c => c.id === extensionCommand)
            if (cmd) {
              // Convert extension command format to ParameterDefinition
              const params = cmd.input_schema?.properties || {}
              const required = (cmd.input_schema?.required as string[]) || []
              const paramDefs: ParameterDefinition[] = Object.entries(params).map(([name, schema]: [string, any]) => ({
                name,
                display_name: schema.title || name,
                help_text: schema.description,
                data_type: schema.type || 'string',
                required: required.includes(name),
                default_value: schema.default,
                min: schema.minimum,
                max: schema.maximum,
                allowed_values: schema.enum,
              }))
              setParameterDefinitions(paramDefs)
              setParameterGroups([])
              setCommandDisplayName(cmd.display_name || cmd.id)
              setCommandParams(seedCommandDefaults(paramDefs))
            }
          }
        }
      } catch (error) {
        console.error('[CommandButton] Failed to fetch command parameters:', error)
      } finally {
        setLoadingParams(false)
      }
    }

    fetchCommandParams()
  }, [dialogOpen, isDeviceCommand, isExtensionCommand, deviceId, commandName, extensionId, extensionCommand])

  const handleClick = () => {
    // Don't execute click in edit mode - allows dragging
    if (editMode) return
    if (disabled || sending || !hasCommand) return

    // Open the command dialog
    setDialogOpen(true)
  }

  const handleConfirmSend = async () => {
    if (!hasCommand || loadingParams) return

    setSending(true)

    try {
      if (isDeviceCommand && deviceId && commandName) {
        // Send device command
        await api.sendCommand(deviceId, commandName, commandParams)
        toast({
          title: t('commandButton.commandSent'),
          description: t('commandButton.commandSentDesc'),
        })
      } else if (isExtensionCommand && extensionId && extensionCommand) {
        // Send extension command
        await api.invokeExtension(extensionId, extensionCommand, commandParams)
        toast({
          title: t('commandButton.commandSent'),
          description: t('commandButton.commandSentDesc'),
        })
      }

      setDialogOpen(false)
      setCommandParams({})
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : 'Command failed'
      toast({
        title: t('commandButton.commandFailed'),
        description: errorMessage,
        variant: 'destructive',
      })
    } finally {
      setSending(false)
    }
  }

  const updateParameter = (name: string, value: unknown) => {
    setCommandParams(prev => ({
      ...prev,
      [name]: value,
    }))
  }

  // Whether the command declares any groups — drives flat vs. grouped render.
  const hasGroups = parameterGroups.length > 0

  const config = dashboardComponentSize[size]
  const Icon = getIconForTitle(title)

  // Look up device name from store for display
  const storeDeviceName = useStore(useCallback(
    (state) => {
      if (!isDeviceCommand || !deviceId) return null
      const device = findDevice(state.devices, deviceId)
      return device?.name || null
    },
    [isDeviceCommand, deviceId]
  ))

  // Determine button label
  const buttonLabel = title || t('commandButton.defaultLabel')
  const buttonSubtext = isDeviceCommand
    ? (storeDeviceName || t('commandButton.deviceCommand'))
    : isExtensionCommand
      ? (extensionCommand || t('commandButton.extensionCommand'))
      : t('commandButton.notConfigured')

  return (
    <>
      <button
        onClick={handleClick}
        disabled={disabled || sending || !hasCommand || editMode}
        className={cn(
          dashboardCardBase,
          'flex-row items-center',
          config.contentGap,
          config.padding,
          'transition-all duration-normal',
          'relative overflow-hidden group',
          !disabled && !sending && hasCommand && !editMode && 'hover:shadow-md hover:bg-accent',
          (disabled || sending || !hasCommand || editMode) && 'opacity-50 cursor-not-allowed',
          editMode && 'pointer-events-none',
          className
        )}
      >
        {/* Icon Section */}
        <div className={cn(
          'flex items-center justify-center shrink-0 rounded-full transition-all duration-slow',
          config.iconContainer,
          'bg-primary text-primary-foreground shadow-md',
        )}>
          <Play className={cn(config.iconSize, 'fill-current')} />
        </div>

        {/* Text section */}
        <div className="flex flex-col min-w-0 flex-1 text-left">
          <span className={cn('font-medium text-foreground truncate', config.titleText)}>
            {buttonLabel}
          </span>
          <span className={cn('text-muted-foreground', config.labelText)}>
            {buttonSubtext}
          </span>
        </div>

        {/* Arrow indicator */}
        <ChevronRight className={cn(
          'h-4 w-4 text-muted-foreground transition-transform duration-normal',
          !disabled && !sending && hasCommand && !editMode && 'group-hover:translate-x-0.5'
        )} />

        {/* Sending indicator */}
        {sending && (
          <span className="absolute top-3 right-3 w-2 h-2 rounded-full bg-warning animate-pulse" />
        )}

        {/* Warning: no command configured */}
        {!hasCommand && (
          <span className="absolute top-3 right-3 w-2 h-2 rounded-full bg-warning" title={t('commandButton.noCommandConfig')} />
        )}
      </button>

      {/* Command Dialog */}
      <UnifiedFormDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        title={title || commandDisplayName || t('commandButton.sendCommand')}
        icon={<Icon className="h-5 w-5" />}
        width="sm"
        loading={loadingParams}
        isSubmitting={sending}
        onSubmit={handleConfirmSend}
        submitLabel={sending ? t('commandButton.sending') : t('commandButton.send')}
        submitDisabled={loadingParams}
        description={
          isDeviceCommand
            ? `${t('commandButton.deviceCommandDesc')}${deviceId ? ` (${deviceId})` : ''}`
            : isExtensionCommand
              ? `${t('commandButton.extensionCommandDesc')}${extensionCommand ? ` (${extensionCommand})` : ''}`
              : undefined
        }
      >
        <div className="space-y-4">
          {!loadingParams && (
            <>
              <ParameterForm
                parameters={parameterDefinitions}
                groups={parameterGroups}
                values={commandParams}
                onChange={updateParameter}
                hideDefault={false}
                grouped={hasGroups}
              />
            </>
          )}
        </div>
      </UnifiedFormDialog>
    </>
  )
}

/** @deprecated Use CommandButton instead */
export const ToggleSwitch = CommandButton
