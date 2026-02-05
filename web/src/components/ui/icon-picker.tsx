/**
 * Unified IconPicker Component
 *
 * Icon picker with search and category filtering.
 * Uses lucide-react icons.
 */

import { useState, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Search, X, Check } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Field } from '@/components/ui/field'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { cn } from '@/lib/utils'
import * as LucideIcons from 'lucide-react'

// Icon categories with their icons (no duplicates across categories)
export const ICON_CATEGORIES = {
  common: [
    'Settings', 'Home', 'User', 'Users', 'Search', 'Bell', 'Heart',
    'Star', 'Check', 'X', 'Plus', 'Minus', 'Filter', 'Menu',
  ],
  status: [
    'CheckCircle', 'XCircle', 'AlertCircle', 'AlertTriangle', 'Info',
    'HelpCircle', 'Circle', 'Dot', 'Loader2', 'Clock', 'Timer',
  ],
  arrows: [
    'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'ArrowUpDown',
    'ChevronUp', 'ChevronDown', 'ChevronLeft', 'ChevronRight',
    'Expand', 'Shrink', 'Minimize', 'Maximize', 'Move',
  ],
  media: [
    'Image', 'Video', 'Camera', 'Mic', 'Volume2', 'VolumeX', 'Music',
    'Play', 'Pause', 'Square', 'Radio', 'Tv', 'Film', 'Clapperboard',
  ],
  files: [
    'File', 'FileText', 'Folder', 'FolderOpen', 'Download', 'Upload',
    'Copy', 'Clipboard', 'Scissors', 'Archive', 'Trash', 'Trash2',
  ],
  devices: [
    'Laptop', 'Monitor', 'Smartphone', 'Tablet', 'HardDrive', 'Cpu',
    'Wifi', 'Bluetooth', 'Usb', 'Cable', 'Plug', 'Power',
  ],
  charts: [
    'BarChart', 'BarChart2', 'BarChart3', 'BarChart4', 'LineChart',
    'PieChart', 'TrendingUp', 'TrendingDown', 'Activity', 'Target',
    'Flame', 'Droplet', 'Wind',
  ],
  misc: [
    'Sun', 'Moon', 'Cloud', 'CloudRain', 'Snow', 'Thunder',
    'MapPin', 'Navigation', 'Compass', 'Globe', 'Earth',
    'Package', 'Box', 'ShoppingCart', 'CreditCard',
  ],
}

export interface IconPickerProps {
  value?: string
  onChange?: (iconName: string) => void
  label?: string
  disabled?: boolean
  className?: string
  allowedCategories?: (keyof typeof ICON_CATEGORIES)[]
}

export function IconPicker({
  value,
  onChange,
  label,
  disabled = false,
  className,
  allowedCategories,
}: IconPickerProps) {
  const { t } = useTranslation('ui')
  const [searchQuery, setSearchQuery] = useState('')
  const [activeCategory, setActiveCategory] = useState<keyof typeof ICON_CATEGORIES | 'all'>('all')

  // Get category labels from i18n
  const CATEGORY_LABELS: Record<keyof typeof ICON_CATEGORIES | 'all', string> = {
    all: t('iconPicker.allCategories', 'All'),
    common: t('iconPicker.common', 'Common'),
    status: t('iconPicker.status', 'Status'),
    arrows: t('iconPicker.arrows', 'Arrows'),
    media: t('iconPicker.media', 'Media'),
    files: t('iconPicker.files', 'Files'),
    devices: t('iconPicker.devices', 'Devices'),
    charts: t('iconPicker.charts', 'Charts'),
    misc: t('iconPicker.misc', 'Misc'),
  }

  // Filter categories based on allowedCategories
  const categories = useMemo(() => {
    if (allowedCategories) {
      return allowedCategories
    }
    return Object.keys(ICON_CATEGORIES) as (keyof typeof ICON_CATEGORIES)[]
  }, [allowedCategories])

  // Get all icons from allowed categories
  const allIcons = useMemo(() => {
    const icons: string[] = []
    for (const cat of categories) {
      icons.push(...ICON_CATEGORIES[cat])
    }
    return icons
  }, [categories])

  // Filter icons by search query
  const filteredIcons = useMemo(() => {
    if (!searchQuery) return allIcons
    const query = searchQuery.toLowerCase()
    return allIcons.filter(icon => icon.toLowerCase().includes(query))
  }, [allIcons, searchQuery])

  // Get icons for active category
  const categoryIcons = useMemo(() => {
    if (activeCategory === 'all') return filteredIcons
    return ICON_CATEGORIES[activeCategory as keyof typeof ICON_CATEGORIES] || []
  }, [activeCategory, filteredIcons])

  // Get icon component by name - returns JSX element or null
  const renderIcon = (name: string, className = 'h-4 w-4') => {
    const Icon = (LucideIcons as any)[name]
    return Icon ? <Icon className={className} /> : null
  }

  const handleSelectIcon = (iconName: string) => {
    onChange?.(iconName)
  }

  const handleClear = () => {
    onChange?.('')
  }

  return (
    <Field className={className}>
      {label && <Label>{label}</Label>}
      <div className="flex items-center gap-2">
        {/* Current icon preview */}
        <Popover>
          <PopoverTrigger asChild disabled={disabled}>
            <Button
              variant="outline"
              className="h-10 flex-1 justify-start font-normal"
            >
              {value ? (
                <div className="flex items-center gap-2">
                  {renderIcon(value, 'h-4 w-4')}
                  <span className="text-sm truncate">{value}</span>
                </div>
              ) : (
                <span className="text-sm text-muted-foreground">{t('iconPicker.selectIcon', 'Select icon')}</span>
              )}
            </Button>
          </PopoverTrigger>
          <PopoverContent className="w-80 p-0" align="start">
            {/* Search */}
            <div className="p-3 border-b">
              <div className="relative">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                <Input
                  placeholder={t('iconPicker.searchPlaceholder')}
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="pl-9 h-9"
                />
              </div>
            </div>

            {/* Category tabs */}
            {!searchQuery && (
              <div className="flex border-b overflow-x-auto">
                <button
                  type="button"
                  onClick={() => setActiveCategory('all')}
                  className={cn(
                    'flex items-center gap-1.5 px-3 py-2 text-xs font-medium border-b-2 transition-colors whitespace-nowrap',
                    activeCategory === 'all'
                      ? 'border-primary text-primary'
                      : 'border-transparent text-muted-foreground hover:text-foreground'
                  )}
                >
                  {CATEGORY_LABELS.all}
                </button>
                {categories.map((cat) => (
                  <button
                    key={cat}
                    type="button"
                    onClick={() => setActiveCategory(cat)}
                    className={cn(
                      'flex items-center gap-1.5 px-3 py-2 text-xs font-medium border-b-2 transition-colors whitespace-nowrap',
                      activeCategory === cat
                        ? 'border-primary text-primary'
                        : 'border-transparent text-muted-foreground hover:text-foreground'
                    )}
                  >
                    {CATEGORY_LABELS[cat]}
                  </button>
                ))}
              </div>
            )}

            {/* Icon grid */}
            <div className="p-3 max-h-64 overflow-y-auto">
              {categoryIcons.length === 0 ? (
                <div className="text-center py-8 text-muted-foreground text-sm">
                  {t('iconPicker.noIconsFound', 'No matching icons found')}
                </div>
              ) : (
                <div className="grid grid-cols-8 gap-1">
                  {categoryIcons.map((iconName) => {
                    const iconElement = renderIcon(iconName, 'h-4 w-4')
                    if (!iconElement) return null
                    return (
                      <button
                        key={iconName}
                        type="button"
                        onClick={() => handleSelectIcon(iconName)}
                        disabled={disabled}
                        className={cn(
                          'h-8 w-8 flex items-center justify-center rounded-md transition-colors',
                          'hover:bg-accent',
                          value === iconName
                            ? 'bg-primary text-primary-foreground'
                            : 'hover:text-accent-foreground'
                        )}
                        title={iconName}
                      >
                        {iconElement}
                      </button>
                    )
                  })}
                </div>
              )}
            </div>

            {/* Footer */}
            {value && (
              <div className="p-2 border-t flex justify-between items-center">
                <span className="text-xs text-muted-foreground truncate max-w-[150px]">
                  {value}
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={handleClear}
                  className="h-7 px-2 text-xs"
                >
                  <X className="h-3 w-3 mr-1" />
                  {t('common.clear', 'Clear')}
                </Button>
              </div>
            )}
          </PopoverContent>
        </Popover>

        {/* Clear button */}
        {value && (
          <Button
            variant="ghost"
            size="sm"
            onClick={handleClear}
            disabled={disabled}
            className="h-10 w-10 p-0"
          >
            <X className="h-4 w-4" />
          </Button>
        )}
      </div>
    </Field>
  )
}

/**
 * Render an icon by name
 */
export interface IconDisplayProps {
  name: string
  className?: string
}

export function IconDisplay({ name, className }: IconDisplayProps) {
  const Icon = (LucideIcons as any)[name]
  if (!Icon) return null
  return <Icon className={className} />
}
