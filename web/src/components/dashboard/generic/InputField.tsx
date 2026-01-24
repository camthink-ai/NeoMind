/**
 * Input Field Component
 *
 * Text input field with data binding support.
 * Supports various input types and validation.
 */

import { useState } from 'react'
import { Eye, EyeOff } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { DashboardComponentWrapper } from '../DashboardComponentWrapper'
import type { DataSource } from '@/types/dashboard'

export interface InputFieldProps {
  dataSource?: DataSource
  value?: string
  onValueChange?: (value: string) => void
  type?: 'text' | 'email' | 'password' | 'tel' | 'url' | 'search'
  placeholder?: string
  label?: string
  description?: string
  prefix?: string
  suffix?: string
  size?: 'sm' | 'md' | 'lg'
  variant?: 'default' | 'filled' | 'underline'
  disabled?: boolean
  readonly?: boolean
  maxLength?: number
  showCard?: boolean
  className?: string
  title?: string
}

const sizeClasses = {
  sm: 'h-8 px-2 py-1 text-sm',
  md: 'h-10 px-3 py-2',
  lg: 'h-12 px-4 py-3 text-lg',
}

const variantClasses = {
  default: 'bg-background border border-input',
  filled: 'bg-muted border-transparent',
  underline: 'bg-transparent border-x-0 border-t-0 rounded-none px-0',
}

export function InputField({
  dataSource,
  value: propValue = '',
  onValueChange,
  type = 'text',
  placeholder = 'Enter value',
  label,
  description,
  prefix,
  suffix,
  size = 'md',
  variant = 'default',
  disabled = false,
  readonly = false,
  maxLength,
  showCard = true,
  className,
  title,
}: InputFieldProps) {
  const [showPassword, setShowPassword] = useState(false)
  const [localValue, setLocalValue] = useState(propValue)

  // Get value from data source or use prop
  const { data } = useDataSource<string>(dataSource, { fallback: propValue })
  const value = data ?? localValue

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const newValue = e.target.value
    setLocalValue(newValue)
    onValueChange?.(newValue)
  }

  const isPassword = type === 'password'
  const inputType = isPassword && showPassword ? 'text' : type
  const displayTitle = title || label

  const content = (
    <div className={cn('space-y-1.5', className)}>
      {/* Label */}
      {(!title && label) && (
        <label className="block text-sm font-medium text-foreground">
          {label}
        </label>
      )}

      {/* Input wrapper */}
      <div className="relative flex items-center">
        {/* Prefix */}
        {prefix && (
          <span className="absolute left-3 text-sm text-muted-foreground">
            {prefix}
          </span>
        )}

        {/* Input */}
        <input
          type={inputType}
          value={value}
          onChange={handleChange}
          placeholder={placeholder}
          disabled={disabled}
          readOnly={readonly}
          maxLength={maxLength}
          className={cn(
            'w-full flex-1 transition-colors',
            'placeholder:text-muted-foreground',
            'focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2',
            'disabled:cursor-not-allowed disabled:opacity-50',
            'file:border-0 file:bg-transparent file:text-sm file:font-medium',
            sizeClasses[size],
            variantClasses[variant],
            prefix && 'pl-8',
            (suffix || isPassword) && 'pr-10',
            variant === 'default' && 'rounded-md',
            variant === 'filled' && 'rounded-md',
            variant !== 'underline' && 'border'
          )}
        />

        {/* Suffix or Password Toggle */}
        {(suffix || isPassword) && (
          <div className="absolute right-3 flex items-center gap-1">
            {isPassword ? (
              <button
                type="button"
                onClick={() => setShowPassword(!showPassword)}
                className="text-muted-foreground hover:text-foreground transition-colors"
              >
                {showPassword ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
              </button>
            ) : suffix ? (
              <span className="text-sm text-muted-foreground">{suffix}</span>
            ) : null}
          </div>
        )}
      </div>

      {/* Character count */}
      {maxLength && (
        <div className="text-xs text-muted-foreground text-right">
          {value?.length || 0} / {maxLength}
        </div>
      )}

      {/* Description */}
      {description && (
        <p className="text-xs text-muted-foreground">{description}</p>
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

