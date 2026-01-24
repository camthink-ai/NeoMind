/**
 * Device Control Component
 *
 * Control panel for sending commands to devices.
 * Shows device status and available commands.
 */

import { useState } from 'react'
import { ToggleLeft, ToggleRight, Send } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { DashboardComponentWrapper } from '../DashboardComponentWrapper'
import type { DataSource } from '@/types/dashboard'

export type CommandType = 'toggle' | 'button' | 'slider' | 'select' | 'color' | 'number'

export interface DeviceCommand {
  id: string
  name: string
  type: CommandType
  currentValue?: unknown
  options?: Array<{ label: string; value: string }>
  min?: number
  max?: number
  step?: number
  unit?: string
  icon?: string
  sending?: boolean
  lastResult?: 'success' | 'error'
}

export interface DeviceControlProps {
  deviceId?: string
  dataSource?: DataSource
  commands?: DeviceCommand[]
  deviceName?: string
  deviceStatus?: 'online' | 'offline' | 'error'
  title?: string
  showStatus?: boolean
  onCommand?: (commandId: string, value: unknown) => void
  layout?: 'grid' | 'list'
  size?: 'sm' | 'md' | 'lg'
  showCard?: boolean
  className?: string
}

const sizeClasses = {
  sm: {
    button: 'px-3 py-1.5 text-sm',
    input: 'px-2 py-1 text-sm',
  },
  md: {
    button: 'px-4 py-2 text-sm',
    input: 'px-3 py-2 text-sm',
  },
  lg: {
    button: 'px-5 py-3 text-base',
    input: 'px-4 py-3 text-base',
  },
}

export function DeviceControl({
  deviceId,
  dataSource,
  commands: propCommands,
  deviceName,
  deviceStatus = 'offline',
  title,
  showStatus = true,
  onCommand,
  layout = 'grid',
  size = 'md',
  showCard = true,
  className,
}: DeviceControlProps) {
  const [localValues, setLocalValues] = useState<Record<string, unknown>>({})
  const [sendingStates, setSendingStates] = useState<Record<string, boolean>>({})

  // Get data from data source or use prop
  const { data } = useDataSource<DeviceCommand[]>(dataSource, { fallback: propCommands || [] })
  const commands = data || []

  const sizes = sizeClasses[size]

  const handleCommand = (command: DeviceCommand, value: unknown) => {
    setSendingStates(prev => ({ ...prev, [command.id]: true }))

    // Call the onCommand callback
    onCommand?.(command.id, value)

    // Simulate command completion
    setTimeout(() => {
      setSendingStates(prev => ({ ...prev, [command.id]: false }))
    }, 500)
  }

  const renderToggle = (command: DeviceCommand) => {
    const isOn = Boolean(localValues[command.id] ?? command.currentValue)
    const Icon = isOn ? ToggleRight : ToggleLeft

    return (
      <button
        onClick={() => {
          const newValue = !isOn
          setLocalValues(prev => ({ ...prev, [command.id]: newValue }))
          handleCommand(command, newValue)
        }}
        className={cn(
          'flex items-center gap-2 p-3 rounded-lg border transition-all',
          'hover:border-primary/50',
          isOn ? 'bg-green-500/10 border-green-500/30' : 'bg-muted/30'
        )}
      >
        <Icon className={cn('h-5 w-5', isOn ? 'text-green-500' : 'text-muted-foreground')} />
        <span className="text-sm font-medium">{command.name}</span>
        {command.icon && <span className="text-lg">{command.icon}</span>}
      </button>
    )
  }

  const renderButton = (command: DeviceCommand) => {
    const isSending = sendingStates[command.id] || command.sending

    return (
      <button
        onClick={() => handleCommand(command, true)}
        disabled={isSending || deviceStatus === 'offline'}
        className={cn(
          'flex items-center justify-center gap-2 rounded-lg border transition-all',
          'hover:bg-primary hover:text-primary-foreground hover:border-primary',
          'disabled:opacity-50 disabled:cursor-not-allowed',
          sizes.button
        )}
      >
        {isSending ? (
          <span className="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
        ) : (
          <Send className="h-4 w-4" />
        )}
        <span>{command.name}</span>
      </button>
    )
  }

  const renderSlider = (command: DeviceCommand) => {
    const currentValue = (localValues[command.id] ?? command.currentValue ?? command.min ?? 0) as number

    return (
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <label className="text-sm font-medium">{command.name}</label>
          <span className="text-sm text-muted-foreground">
            {currentValue}{command.unit}
          </span>
        </div>
        <input
          type="range"
          min={command.min ?? 0}
          max={command.max ?? 100}
          step={command.step ?? 1}
          value={currentValue}
          onChange={(e) => {
            const newValue = Number(e.target.value)
            setLocalValues(prev => ({ ...prev, [command.id]: newValue }))
          }}
          onMouseUp={() => handleCommand(command, currentValue)}
          disabled={deviceStatus === 'offline'}
          className="w-full"
        />
      </div>
    )
  }

  const renderSelect = (command: DeviceCommand) => {
    const currentValue = localValues[command.id] ?? command.currentValue

    return (
      <div className="space-y-2">
        <label className="text-sm font-medium">{command.name}</label>
        <select
          value={String(currentValue ?? '')}
          onChange={(e) => {
            const newValue = e.target.value
            setLocalValues(prev => ({ ...prev, [command.id]: newValue }))
            handleCommand(command, newValue)
          }}
          disabled={deviceStatus === 'offline'}
          className={cn(
            'w-full border border-input rounded-md bg-background',
            sizes.input
          )}
        >
          {command.options?.map(option => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>
    )
  }

  const renderNumber = (command: DeviceCommand) => {
    const currentValue = localValues[command.id] ?? command.currentValue ?? 0

    return (
      <div className="space-y-2">
        <label className="text-sm font-medium">{command.name}</label>
        <div className="flex items-center gap-2">
          <input
            type="number"
            min={command.min}
            max={command.max}
            step={command.step ?? 1}
            value={String(currentValue)}
            onChange={(e) => setLocalValues(prev => ({ ...prev, [command.id]: e.target.value }))}
            disabled={deviceStatus === 'offline'}
            className={cn(
              'flex-1 border border-input rounded-md bg-background',
              sizes.input
            )}
          />
          <button
            onClick={() => handleCommand(command, Number(currentValue))}
            disabled={deviceStatus === 'offline'}
            className="px-3 py-2 bg-primary text-primary-foreground rounded-md hover:bg-primary/90"
          >
            <Send className="h-4 w-4" />
          </button>
        </div>
      </div>
    )
  }

  const renderCommand = (command: DeviceCommand) => {
    switch (command.type) {
      case 'toggle':
        return renderToggle(command)
      case 'button':
        return renderButton(command)
      case 'slider':
        return renderSlider(command)
      case 'select':
        return renderSelect(command)
      case 'number':
        return renderNumber(command)
      default:
        return renderButton(command)
    }
  }

  const statusIndicator = deviceStatus === 'online' && (
    <div className="flex items-center gap-2 text-sm">
      <span className="h-2 w-2 rounded-full bg-green-500 animate-pulse" />
      <span className="text-muted-foreground">Online</span>
    </div>
  )

  const content = (
    <div className={cn('space-y-3', className)}>
      {/* Device status header */}
      {showStatus && deviceName && (
        <div className="flex items-center justify-between pb-3 border-b border-border">
          <div>
            <h3 className="font-medium">{deviceName}</h3>
            {deviceId && <p className="text-xs text-muted-foreground">{deviceId}</p>}
          </div>
          {statusIndicator}
        </div>
      )}

      {/* Commands */}
      {commands.length === 0 ? (
        <div className="py-8 text-center text-muted-foreground text-sm">
          {deviceStatus === 'offline' ? 'Device is offline' : 'No commands available'}
        </div>
      ) : (
        <div className={cn(
          layout === 'grid' ? 'grid grid-cols-2 gap-3' : 'space-y-3'
        )}>
          {commands.map(renderCommand)}
        </div>
      )}
    </div>
  )

  if (showCard && title) {
    return (
      <DashboardComponentWrapper
        title={title}
        showCard={true}
        padding="md"
      >
        {content}
      </DashboardComponentWrapper>
    )
  }

  return content
}

