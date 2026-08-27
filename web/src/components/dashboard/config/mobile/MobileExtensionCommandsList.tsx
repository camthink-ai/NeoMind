import { Check, Zap } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { Extension, ExtensionCommandDescriptor } from '@/types'

interface MobileExtensionCommandsListProps {
  extension: Extension
  selectedItems: Set<string>
  onSelectItem: (item: string) => void
  t: (key: string) => string
}

export function MobileExtensionCommandsList({
  extension,
  selectedItems,
  onSelectItem,
  t,
}: MobileExtensionCommandsListProps) {
  const commands = extension.commands || []

  if (commands.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-muted-foreground text-sm p-4">
        {t('extensions:noCommands') || 'No commands available'}
      </div>
    )
  }

  return (
    <div className="p-4 space-y-3">
      {commands.map((cmd: ExtensionCommandDescriptor) => {
        const itemKey = `extension:${extension.id}:${cmd.id}`
        const isSelected = selectedItems.has(itemKey)

        return (
          <button
            key={cmd.id}
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
                  {cmd.display_name || cmd.id}
                </div>
                <div className="text-sm text-muted-foreground truncate">
                  {cmd.description || cmd.id}
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
