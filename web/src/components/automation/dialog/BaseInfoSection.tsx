/**
 * BaseInfoSection Component
 *
 * Unified base information section for automation dialogs.
 * Contains: name, description, enabled toggle, and optional scope selection.
 */

import { ReactNode } from 'react'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { cn } from '@/lib/utils'

export type ScopeType = 'global' | 'device_type' | 'device'

export interface ScopeOption {
  value: string
  label: string
}

export interface BaseInfoSectionProps {
  // Basic fields
  name: string
  onNameChange: (value: string) => void
  description: string
  onDescriptionChange: (value: string) => void
  enabled: boolean
  onEnabledChange: (value: boolean) => void

  // Validation
  errors?: {
    name?: string
  }

  // Transform-specific: Scope
  showScope?: boolean
  scopeType?: ScopeType
  onScopeTypeChange?: (value: ScopeType) => void
  scopeValue?: string
  onScopeValueChange?: (value: string) => void
  scopeOptions?: ScopeOption[]

  // Additional content (e.g., extra fields)
  extraContent?: ReactNode

  // Labels
  nameLabel?: string
  namePlaceholder?: string
  descriptionPlaceholder?: string
}

export function BaseInfoSection({
  name,
  onNameChange,
  description,
  onDescriptionChange,
  enabled,
  onEnabledChange,
  errors,
  showScope = false,
  scopeType = 'global',
  onScopeTypeChange,
  scopeValue = '',
  onScopeValueChange,
  scopeOptions = [],
  extraContent,
  nameLabel = '名称',
  namePlaceholder = '输入名称...',
  descriptionPlaceholder = '可选描述...',
}: BaseInfoSectionProps) {
  return (
    <section className="px-4 md:px-8 py-4 md:py-6 border-b bg-muted/10 shrink-0">
      <div className="max-w-4xl mx-auto">
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4 md:gap-6">
          {/* Name */}
          <div className="space-y-2">
            <Label htmlFor="info-name" className="text-sm font-medium">
              {nameLabel} <span className="text-destructive">*</span>
            </Label>
            <Input
              id="info-name"
              value={name}
              onChange={e => onNameChange(e.target.value)}
              placeholder={namePlaceholder}
              className={cn(errors?.name && "border-destructive focus-visible:ring-destructive")}
            />
            {errors?.name && (
              <p className="text-xs text-destructive">{errors.name}</p>
            )}
          </div>

          {/* Description */}
          <div className="space-y-2">
            <Label htmlFor="info-desc" className="text-sm font-medium">描述</Label>
            <Input
              id="info-desc"
              value={description}
              onChange={e => onDescriptionChange(e.target.value)}
              placeholder={descriptionPlaceholder}
            />
          </div>

          {/* Enabled toggle */}
          <div className="flex items-center gap-3 pt-6">
            <Switch
              id="info-enabled"
              checked={enabled}
              onCheckedChange={onEnabledChange}
            />
            <Label htmlFor="info-enabled" className="text-sm font-medium cursor-pointer">
              启用此{showScope ? '转换' : '规则'}
            </Label>
          </div>

          {/* Transform: Scope Type */}
          {showScope && onScopeTypeChange && (
            <div className="space-y-2">
              <Label className="text-sm font-medium">作用范围</Label>
              <Select value={scopeType} onValueChange={onScopeTypeChange}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="global">全局</SelectItem>
                  <SelectItem value="device_type">设备类型</SelectItem>
                  <SelectItem value="device">单个设备</SelectItem>
                </SelectContent>
              </Select>
            </div>
          )}

          {/* Transform: Scope Value (conditional) */}
          {showScope && scopeType !== 'global' && onScopeValueChange && (
            <div className="space-y-2">
              <Label className="text-sm font-medium">
                {scopeType === 'device_type' ? '设备类型' : '设备'}
              </Label>
              <Select value={scopeValue} onValueChange={onScopeValueChange}>
                <SelectTrigger>
                  <SelectValue placeholder="选择..." />
                </SelectTrigger>
                <SelectContent>
                  {scopeOptions.map(opt => (
                    <SelectItem key={opt.value} value={opt.value}>
                      {opt.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
        </div>

        {/* Extra content area */}
        {extraContent}
      </div>
    </section>
  )
}
