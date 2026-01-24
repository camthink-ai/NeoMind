/**
 * Dropdown Component
 *
 * Select dropdown with search support.
 * Supports data binding and custom options.
 */

import { useState } from 'react'
import { ChevronDown, Search, Check } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { DashboardComponentWrapper } from '../DashboardComponentWrapper'
import type { DataSource } from '@/types/dashboard'

export interface DropdownOption {
  label: string
  value: string
  icon?: string
  disabled?: boolean
}

export interface DropdownProps {
  dataSource?: DataSource
  value?: string
  onValueChange?: (value: string) => void
  options?: DropdownOption[]
  placeholder?: string
  label?: string
  description?: string
  searchable?: boolean
  size?: 'sm' | 'md' | 'lg'
  variant?: 'default' | 'outline' | 'ghost'
  disabled?: boolean
  showCard?: boolean
  className?: string
  title?: string
}

const sizeClasses = {
  sm: 'h-8 px-2 py-1 text-sm',
  md: 'h-10 px-3 py-2',
  lg: 'h-12 px-4 py-3 text-lg',
}

export function Dropdown({
  dataSource,
  value: propValue,
  onValueChange,
  options: propOptions = [],
  placeholder = 'Select an option',
  label,
  description,
  searchable = false,
  size = 'md',
  variant = 'default',
  disabled = false,
  showCard = true,
  className,
  title,
}: DropdownProps) {
  const [open, setOpen] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')

  // Get value from data source or use prop
  const { data } = useDataSource<string>(dataSource, { fallback: propValue })
  const value = data ?? propValue ?? ''

  // Get options from data source or use prop
  const { data: optionsData } = useDataSource<DropdownOption[]>(dataSource?.type === 'static' ? undefined : undefined, {
    fallback: propOptions,
  })
  const options = (optionsData as DropdownOption[]) || propOptions

  // Filter options based on search query
  const filteredOptions = searchable && searchQuery
    ? options.filter(opt =>
        opt.label.toLowerCase().includes(searchQuery.toLowerCase())
      )
    : options

  // Get selected option
  const selectedOption = options.find(opt => opt.value === value)

  const handleSelect = (optValue: string) => {
    onValueChange?.(optValue)
    setOpen(false)
    setSearchQuery('')
  }

  const displayLabel = selectedOption?.label || placeholder
  const displayTitle = title || label

  const content = (
    <div className={cn('relative', className)}>
      {/* Label */}
      {(!title && label) && (
        <label className="block text-sm font-medium mb-1.5 text-foreground">
          {label}
        </label>
      )}

      {/* Dropdown button */}
      <button
        type="button"
        onClick={() => !disabled && setOpen(!open)}
        disabled={disabled}
        className={cn(
          'w-full flex items-center justify-between gap-2 text-left transition-colors',
          'focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2',
          'disabled:cursor-not-allowed disabled:opacity-50',
          sizeClasses[size],
          variant === 'default' && 'bg-background border border-input hover:bg-accent rounded-md',
          variant === 'outline' && 'bg-transparent border-2 border-input hover:border-primary rounded-md',
          variant === 'ghost' && 'hover:bg-accent rounded-md',
          !selectedOption && 'text-muted-foreground'
        )}
      >
        <span className="truncate flex items-center gap-2">
          {selectedOption?.icon && <span>{selectedOption.icon}</span>}
          {displayLabel}
        </span>
        <ChevronDown
          className={cn(
            'h-4 w-4 shrink-0 transition-transform',
            open && 'rotate-180'
          )}
        />
      </button>

      {/* Dropdown menu */}
      {open && (
        <>
          {/* Backdrop */}
          <div
            className="fixed inset-0 z-40"
            onClick={() => setOpen(false)}
          />

          {/* Menu */}
          <div className={cn(
            'absolute z-50 w-full mt-1 bg-background border border-input rounded-md shadow-lg',
            'max-h-60 overflow-auto'
          )}>
            {/* Search input */}
            {searchable && (
              <div className="sticky top-0 p-2 border-b bg-background">
                <div className="relative">
                  <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
                  <input
                    type="text"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder="Search..."
                    className="w-full pl-8 pr-3 py-1.5 text-sm bg-background border border-input rounded-md focus:outline-none focus:ring-2 focus:ring-ring"
                    autoFocus
                  />
                </div>
              </div>
            )}

            {/* Options */}
            <div className="py-1">
              {filteredOptions.length === 0 ? (
                <div className="px-3 py-2 text-sm text-muted-foreground text-center">
                  No options found
                </div>
              ) : (
                filteredOptions.map((option) => (
                  <button
                    key={option.value}
                    type="button"
                    onClick={() => handleSelect(option.value)}
                    disabled={option.disabled}
                    className={cn(
                      'w-full flex items-center gap-2 px-3 py-2 text-sm text-left',
                      'hover:bg-accent hover:text-accent-foreground',
                      'disabled:opacity-50 disabled:cursor-not-allowed',
                      value === option.value && 'bg-accent'
                    )}
                  >
                    {option.icon && <span>{option.icon}</span>}
                    <span className="flex-1 truncate">{option.label}</span>
                    {value === option.value && (
                      <Check className="h-4 w-4 text-primary shrink-0" />
                    )}
                  </button>
                ))
              )}
            </div>
          </div>
        </>
      )}

      {/* Description */}
      {description && (
        <p className="text-xs text-muted-foreground mt-1">{description}</p>
      )}
    </div>
  )

  if (showCard) {
    return (
      <DashboardComponentWrapper
        title={displayTitle}
        showCard={true}
        padding="md"
      >
        {content}
      </DashboardComponentWrapper>
    )
  }

  return content
}

