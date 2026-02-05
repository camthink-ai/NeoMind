/**
 * Unified ColorPicker Component
 *
 * Color picker with preset palette, custom color input, and transparency support.
 * Follows system UI standards.
 */

import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, ChevronDown, Palette, Sun } from 'lucide-react'
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

// Theme-aware special colors
const THEME_FOREGROUND = 'hsl(var(--foreground))'
const THEME_CARD_FOREGROUND = 'hsl(var(--card-foreground))'

// Check if a color is theme-aware
function isThemeColor(color: string): boolean {
  return color.startsWith('hsl(var(')
}

// Preset color palettes
export const COLOR_PRESETS = {
  primary: [
    THEME_FOREGROUND, // Theme foreground (black in light, white in dark)
    '#ef4444', // Red
    '#f97316', // Orange
    '#eab308', // Yellow
    '#22c55e', // Green
    '#06b6d4', // Cyan
    '#3b82f6', // Blue
    '#8b5cf6', // Purple
    '#ec4899', // Pink
  ],
  neutral: [
    THEME_FOREGROUND, // Theme foreground (black in light, white in dark)
    '#171717', // Zinc 950
    '#404040', // Zinc 700
    '#737373', // Zinc 500
    '#a3a3a3', // Zinc 400
    '#d4d4d4', // Zinc 300
    '#e5e5e5', // Zinc 200
    '#f5f5f5', // Zinc 100
  ],
  semantic: [
    '#22c55e', // Success
    '#eab308', // Warning
    '#ef4444', // Error/Danger
    '#3b82f6', // Info
    '#8b5cf6', // Purple
  ],
}

export interface ColorPickerProps {
  value?: string
  onChange?: (color: string) => void
  label?: string
  presets?: keyof typeof COLOR_PRESETS | 'all' | 'none'
  showAlpha?: boolean
  disabled?: boolean
  className?: string
}

function isValidHexColor(value: string): boolean {
  return /^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6})$/.test(value)
}

function normalizeHexColor(value: string): string {
  if (!value.startsWith('#')) {
    return `#${value}`
  }
  return value
}

export function ColorPicker({
  value = '#3b82f6',
  onChange,
  label,
  presets = 'primary',
  showAlpha = false,
  disabled = false,
  className,
}: ColorPickerProps) {
  const { t } = useTranslation('ui')
  const [inputValue, setInputValue] = useState(value)

  const handleInputChange = (newValue: string) => {
    setInputValue(newValue)
    if (isValidHexColor(newValue)) {
      onChange?.(normalizeHexColor(newValue))
    }
  }

  const handlePresetClick = (color: string) => {
    setInputValue(color)
    onChange?.(color)
  }

  // Check if a preset color is currently selected
  const isColorSelected = (presetColor: string): boolean => {
    if (isThemeColor(presetColor) || isThemeColor(value)) {
      // For theme colors, compare the CSS variable part
      return presetColor === value
    }
    return value.toLowerCase() === presetColor.toLowerCase()
  }

  // Get display style for a preset color button
  const getPresetStyle = (color: string) => {
    if (isThemeColor(color)) {
      return { backgroundColor: color }
    }
    return { backgroundColor: color }
  }

  // Get icon for theme-aware colors
  const getThemeIcon = (color: string) => {
    if (color === THEME_FOREGROUND) {
      return <Sun className="h-3 w-3 text-foreground drop-shadow-md mx-auto" />
    }
    return null
  }

  // Get presets to show
  const getPresetColors = (): string[] => {
    if (presets === 'all') {
      return [...COLOR_PRESETS.primary, ...COLOR_PRESETS.neutral, ...COLOR_PRESETS.semantic]
    }
    if (presets === 'none') return []
    return COLOR_PRESETS[presets] || COLOR_PRESETS.primary
  }

  const presetColors = getPresetColors()

  return (
    <Field className={className}>
      {label && <Label>{label}</Label>}
      <div className="flex items-center gap-2">
        {/* Color preview + trigger */}
        <Popover>
          <PopoverTrigger asChild disabled={disabled}>
            <Button
              variant="outline"
              className="h-10 w-12 p-0 shrink-0 border-input"
            >
              <div
                className="h-5 w-5 rounded-sm"
                style={isThemeColor(value) ? { backgroundColor: value } : { backgroundColor: value }}
              />
            </Button>
          </PopoverTrigger>
          <PopoverContent className="w-64 p-3" align="start">
            {/* Preset colors */}
            {presetColors.length > 0 && (
              <div className="mb-3">
                <div className="grid grid-cols-8 gap-1">
                  {presetColors.map((color) => {
                    const themeIcon = getThemeIcon(color)
                    const isSelected = isColorSelected(color)
                    return (
                      <button
                        key={color}
                        type="button"
                        onClick={() => handlePresetClick(color)}
                        disabled={disabled}
                        className={cn(
                          "h-6 w-6 rounded-sm border transition-all hover:scale-110 relative",
                          isSelected
                            ? 'ring-2 ring-ring ring-offset-2'
                            : 'border-border'
                        )}
                        style={getPresetStyle(color)}
                      >
                        {themeIcon && !isSelected && (
                          <div className="absolute inset-0 flex items-center justify-center opacity-50">
                            {themeIcon}
                          </div>
                        )}
                        {isSelected && !themeIcon && (
                          <Check className="h-3 w-3 text-white drop-shadow-md mx-auto" />
                        )}
                        {isSelected && themeIcon && (
                          <div className="absolute inset-0 flex items-center justify-center">
                            {themeIcon}
                          </div>
                        )}
                      </button>
                    )
                  })}
                </div>
                {/* Legend for theme-aware colors */}
                <div className="flex items-center gap-3 mt-2 text-xs text-muted-foreground px-1">
                  <div className="flex items-center gap-1">
                    <Sun className="h-3 w-3" />
                    <span>{t('colorPicker.themeForegroundColor')}</span>
                  </div>
                </div>
              </div>
            )}

            {/* Custom color input */}
            <div className="flex items-center gap-2 pt-2 border-t">
              <div className="relative flex-1">
                <div className="absolute left-2 top-1/2 -translate-y-1/2 text-muted-foreground">
                  <Palette className="h-4 w-4" />
                </div>
                <Input
                  value={inputValue}
                  onChange={(e) => handleInputChange(e.target.value)}
                  placeholder="#3b82f6"
                  className="pl-8 h-9 font-mono text-sm"
                  disabled={disabled}
                />
              </div>
              <Input
                type="color"
                value={value}
                onChange={(e) => {
                  const newColor = e.target.value
                  setInputValue(newColor)
                  onChange?.(newColor)
                }}
                className="h-9 w-12 p-1 cursor-pointer"
                disabled={disabled || isThemeColor(value)}
              />
            </div>
          </PopoverContent>
        </Popover>

        {/* Text input */}
        <Input
          type="text"
          value={inputValue}
          onChange={(e) => handleInputChange(e.target.value)}
          placeholder="#3b82f6"
          className="flex-1 h-10 font-mono text-sm"
          disabled={disabled}
        />
      </div>
    </Field>
  )
}

/**
 * Compact ColorPicker - inline version without popover
 */
export interface CompactColorPickerProps {
  value?: string
  onChange?: (color: string) => void
  presets?: string[]
  disabled?: boolean
  className?: string
}

export function CompactColorPicker({
  value = '#3b82f6',
  onChange,
  presets = COLOR_PRESETS.primary,
  disabled = false,
  className,
}: CompactColorPickerProps) {
  // Check if a preset color is currently selected
  const isColorSelected = (presetColor: string): boolean => {
    if (isThemeColor(presetColor) || isThemeColor(value)) {
      return presetColor === value
    }
    return value.toLowerCase() === presetColor.toLowerCase()
  }

  // Get icon for theme-aware colors
  const getThemeIcon = (color: string) => {
    if (color === THEME_FOREGROUND) {
      return <Sun className="h-3 w-3" />
    }
    return null
  }

  return (
    <div className={cn('flex items-center gap-2', className)}>
      {/* Color input */}
      <Input
        type="color"
        value={value}
        onChange={(e) => onChange?.(e.target.value)}
        className="h-9 w-12 p-1 cursor-pointer"
        disabled={disabled || isThemeColor(value)}
      />
      {/* Text input */}
      <Input
        type="text"
        value={value}
        onChange={(e) => {
          const newValue = e.target.value
          if (isValidHexColor(newValue)) {
            onChange?.(normalizeHexColor(newValue))
          }
        }}
        placeholder="#3b82f6"
        className="flex-1 h-9 font-mono text-sm"
        disabled={disabled}
      />
      {/* Presets */}
      <div className="flex gap-1">
        {presets.map((color) => {
          const themeIcon = getThemeIcon(color)
          const isSelected = isColorSelected(color)
          return (
            <button
              key={color}
              type="button"
              onClick={() => onChange?.(color)}
              disabled={disabled}
              className={cn(
                'h-7 w-7 rounded-sm border transition-all hover:scale-110 shrink-0 relative flex items-center justify-center',
                isSelected
                  ? 'ring-2 ring-ring ring-offset-2'
                  : 'border-border'
              )}
              style={isThemeColor(color) ? { backgroundColor: color } : { backgroundColor: color }}
              >
                {themeIcon && !isSelected && (
                  <div className="opacity-40">{themeIcon}</div>
                )}
                {isSelected && themeIcon && (
                  <div className="text-foreground">{themeIcon}</div>
                )}
              </button>
          )
        })}
      </div>
    </div>
  )
}
