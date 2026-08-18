import { createPortal } from 'react-dom'
import { getPortalRoot } from '@/lib/portal'
import { ChevronLeft } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { dialogHeader } from '@/design-system/tokens/size'
import type { MetricDefinition, CommandDefinition } from '@/types'
import type { Extension } from '@/types'
import { useMobileBodyScrollLock } from '@/hooks/useBodyScrollLock'
import type { CategoryType } from '../categories'
import { MobileMetricsList } from './MobileMetricsList'
import { MobileCommandsList } from './MobileCommandsList'
import { MobileExtensionMetricsList } from './MobileExtensionMetricsList'
import { MobileExtensionCommandsList } from './MobileExtensionCommandsList'

export interface MobileItemSelectorProps {
  isOpen: boolean
  onClose: () => void
  selectedDevice: any
  selectedExtension: any
  selectedCategory: CategoryType
  selectedItems: Set<string>
  onSelectItem: (item: string) => void
  deviceMetricsMap: Map<string, MetricDefinition[]>
  deviceCommandsMap: Map<string, CommandDefinition[]>
  extensionMetricsMap: Map<string, Array<{ name: string; display_name: string; data_type: string; unit?: string }>>
  devices: any[]
  extensions: Extension[]
  summaries: Map<string, any>
  availability: Map<string, { hasData: boolean; dataPointCount?: number }>
  checkingData: boolean
  getDeviceInfoProperties: (t: (key: string) => string) => Array<{ id: string; name: string }>
  t: (key: string) => string
  insets: { top: number; bottom: number; left: number; right: number }
}

export function MobileItemSelector({
  isOpen,
  onClose,
  selectedDevice,
  selectedExtension,
  selectedCategory,
  selectedItems,
  onSelectItem,
  deviceMetricsMap,
  deviceCommandsMap,
  extensionMetricsMap,
  devices,
  extensions,
  summaries,
  availability,
  checkingData,
  getDeviceInfoProperties,
  t,
  insets,
}: MobileItemSelectorProps) {
  // Lock body scroll when mobile selector is open
  useMobileBodyScrollLock(isOpen)

  if (!isOpen) return null

  const title = selectedCategory === 'device-metric' || selectedCategory === 'device-command'
    ? (selectedDevice?.name || selectedDevice?.id || t('dataSource.selectDevice'))
    : (selectedExtension?.name || t('extensions:selectExtension') || 'Select Extension')

  return createPortal(
    <div className="fixed inset-0 z-[100] bg-background animate-in slide-in-from-right-0 duration-200">
      <div className="flex h-full w-full flex-col">
        {/* Header */}
        <div
          className={cn(dialogHeader, 'gap-3')}
          style={{ paddingTop: `calc(1rem + ${insets.top}px)` }}
        >
          <Button variant="ghost" size="icon" onClick={onClose} className="shrink-0">
            <ChevronLeft className="h-5 w-5" />
          </Button>
          <div className="min-w-0 flex-1">
            <h1 className="text-base font-semibold truncate">{title}</h1>
            <p className="text-xs text-muted-foreground truncate">
              {selectedCategory === 'device-metric' && t('dataSource.selectMetrics')}
              {selectedCategory === 'device-command' && t('dataSource.selectCommands')}
              {selectedCategory === 'extension' && (t('dataSource.metrics') || 'Metrics')}
              {selectedCategory === 'extension-command' && (t('dataSource.commands') || 'Commands')}
            </p>
          </div>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto">
          {selectedCategory === 'device-metric' && selectedDevice && (
            <MobileMetricsList
              device={selectedDevice}
              deviceMetricsMap={deviceMetricsMap}
              summaries={summaries}
              availability={availability}
              checkingData={checkingData}
              getDeviceInfoProperties={getDeviceInfoProperties}
              selectedItems={selectedItems}
              onSelectItem={onSelectItem}
              t={t}
            />
          )}

          {selectedCategory === 'device-command' && selectedDevice && (
            <MobileCommandsList
              device={selectedDevice}
              deviceCommandsMap={deviceCommandsMap}
              selectedItems={selectedItems}
              onSelectItem={onSelectItem}
              t={t}
            />
          )}

          {selectedCategory === 'extension' && selectedExtension && (
            <MobileExtensionMetricsList
              extension={selectedExtension}
              extensionMetricsMap={extensionMetricsMap}
              selectedItems={selectedItems}
              onSelectItem={onSelectItem}
              t={t}
            />
          )}

          {selectedCategory === 'extension-command' && selectedExtension && (
            <MobileExtensionCommandsList
              extension={selectedExtension}
              selectedItems={selectedItems}
              onSelectItem={onSelectItem}
              t={t}
            />
          )}

          {selectedCategory === 'device-metric' && !selectedDevice && (
            <div className="flex items-center justify-center h-full text-muted-foreground text-sm p-4 text-center">
              {t('dataSource.selectDevice')}
            </div>
          )}

          {selectedCategory === 'device-command' && !selectedDevice && (
            <div className="flex items-center justify-center h-full text-muted-foreground text-sm p-4 text-center">
              {t('dataSource.selectDevice')}
            </div>
          )}

          {selectedCategory === 'extension' && !selectedExtension && (
            <div className="flex items-center justify-center h-full text-muted-foreground text-sm p-4 text-center">
              {t('extensions:selectExtension') || 'Select an extension'}
            </div>
          )}

          {selectedCategory === 'extension-command' && !selectedExtension && (
            <div className="flex items-center justify-center h-full text-muted-foreground text-sm p-4 text-center">
              {t('extensions:selectExtension') || 'Select an extension'}
            </div>
          )}
        </div>
      </div>
    </div>,
    getPortalRoot()
  )
}
