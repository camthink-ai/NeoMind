import { Check, Zap } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { CommandDefinition } from '@/types'

interface MobileCommandsListProps {
  device: any
  deviceCommandsMap: Map<string, CommandDefinition[]>
  selectedItems: Set<string>
  onSelectItem: (item: string) => void
  t: (key: string) => string
}

export function MobileCommandsList({
  device,
  deviceCommandsMap,
  selectedItems,
  onSelectItem,
  t,
}: MobileCommandsListProps) {
  const commands = deviceCommandsMap.get(device.id) || []

  if (commands.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-muted-foreground text-sm p-4">
        {t('dataSource.noAvailableCommands')}
      </div>
    )
  }

  return (
    <div className="p-4 space-y-3">
      {commands.map(cmd => {
        const itemKey = `device:${device.id}:${cmd.name}`
        const isSelected = selectedItems.has(itemKey)

        return (
          <button
            key={cmd.name}
            type="button"
            onClick={() => onSelectItem(itemKey)}
            className={cn(
              'w-full text-left transition-colors duration-fast',
              'group relative rounded-lg border p-4',
              isSelected
                ? 'bg-muted border-border'
                : 'bg-card border-border active:bg-accent'
            )}
          >
            <div className="flex items-center gap-3">
              <div className={cn(
                'shrink-0 w-6 h-6 rounded-full flex items-center justify-center transition-colors',
                isSelected
                  ? 'bg-primary text-primary-foreground'
                  : 'bg-muted text-muted-foreground'
              )}>
                <Check className={cn(
                  'h-4 w-4',
                  isSelected ? 'opacity-100' : 'opacity-0'
                )} />
              </div>
              <div className="flex-1 min-w-0">
                <div className={cn(
                  'text-base font-medium truncate',
                  isSelected ? 'text-foreground' : 'text-foreground'
                )}>
                  {cmd.display_name || cmd.name}
                </div>
                <div className="text-sm text-muted-foreground truncate">
                  {cmd.name}
                </div>
              </div>
              <Zap className={cn(
                'h-5 w-5 shrink-0',
                isSelected ? 'text-warning' : 'text-muted-foreground'
              )} />
            </div>
          </button>
        )
      })}
    </div>
  )
}
