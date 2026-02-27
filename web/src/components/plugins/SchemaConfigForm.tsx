import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
import { useTranslation } from 'react-i18next'
import { useState } from 'react'
import { Plus, X, Code, Braces, Hash } from 'lucide-react'
import type { PluginUISchema, FieldSchema } from '@/types/plugin-schema'

interface SchemaConfigFormProps {
  schema: PluginUISchema
  config: Record<string, unknown>
  onChange: (config: Record<string, unknown>) => void
}

export function SchemaConfigForm({ schema, config, onChange }: SchemaConfigFormProps) {
  const updateField = (name: string, value: unknown) => {
    onChange({ ...config, [name]: value })
  }

  // Group fields by their group property
  const groupedFields: Record<string, FieldSchema[]> = {}
  Object.values(schema.fields).forEach((field) => {
    const group = field.group || 'default'
    if (!groupedFields[group]) {
      groupedFields[group] = []
    }
    groupedFields[group].push(field)
  })

  return (
    <div className="space-y-4">
      {Object.entries(groupedFields).map(([groupName, fields]) => (
        <div key={groupName} className="space-y-3">
          {groupName !== 'default' && schema.groups?.[groupName] && (
            <div className={cn(
              "pt-2 pb-1 border-b",
              schema.groups[groupName].collapsible && "cursor-pointer"
            )}>
              <h4 className="font-medium text-sm">
                {schema.groups[groupName].label}
              </h4>
              {schema.groups[groupName].description && (
                <p className="text-xs text-muted-foreground mt-0.5">
                  {schema.groups[groupName].description}
                </p>
              )}
            </div>
          )}

          {fields
            .filter((f) => !f.hidden && isFieldVisible(f, config))
            .sort((a, b) => (a.order || 0) - (b.order || 0))
            .map((field) => (
              <FormField
                key={field.name}
                field={field}
                value={config[field.name]}
                onChange={(value) => updateField(field.name, value)}
              />
            ))}
        </div>
      ))}
    </div>
  )
}

// ============================================================================
// Form Field Component
// ============================================================================

interface FormFieldProps {
  field: FieldSchema
  value: unknown
  onChange: (value: unknown) => void
}

function FormField({ field, value, onChange }: FormFieldProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const id = `field-${field.name}`
  const isReadOnly = field.readonly
  const currentValue = value !== undefined ? value : field.default

  switch (field.type) {
    case 'string':
    case 'url':
    case 'email':
      return (
        <div className="space-y-2">
          <Label htmlFor={id}>
            {field.label}
            {field.required && <span className="text-destructive ml-1">*</span>}
          </Label>
          <Input
            id={id}
            type="text"
            inputMode={field.type === 'url' ? 'url' : field.type === 'email' ? 'email' : undefined}
            placeholder={field.placeholder}
            value={currentValue as string ?? ''}
            onChange={(e) => onChange(e.target.value)}
            disabled={isReadOnly}
            required={field.required}
          />
          {field.description && (
            <p className="text-xs text-muted-foreground">{field.description}</p>
          )}
        </div>
      )

    case 'password':
      return (
        <div className="space-y-2">
          <Label htmlFor={id}>
            {field.label}
            {field.required && <span className="text-destructive ml-1">*</span>}
          </Label>
          <Input
            id={id}
            type="password"
            placeholder={field.placeholder || '••••••••'}
            value={currentValue as string ?? ''}
            onChange={(e) => onChange(e.target.value)}
            disabled={isReadOnly}
            required={field.required}
          />
          {field.description && (
            <p className="text-xs text-muted-foreground">{field.description}</p>
          )}
        </div>
      )

    case 'number':
      return (
        <div className="space-y-2">
          <Label htmlFor={id}>
            {field.label}
            {field.required && <span className="text-destructive ml-1">*</span>}
          </Label>
          <Input
            id={id}
            type="number"
            placeholder={field.placeholder}
            value={currentValue as number ?? 0}
            onChange={(e) => onChange(e.target.value ? Number(e.target.value) : undefined)}
            disabled={isReadOnly}
            min={field.minimum}
            max={field.maximum}
            step={field.step}
            required={field.required}
          />
          {field.description && (
            <p className="text-xs text-muted-foreground">{field.description}</p>
          )}
        </div>
      )

    case 'boolean':
      return (
        <div className="flex items-center gap-2">
          <Switch
            id={id}
            checked={!!currentValue}
            onCheckedChange={onChange}
            disabled={isReadOnly}
          />
          <Label htmlFor={id} className="cursor-pointer">
            {field.label}
          </Label>
          {field.description && (
            <span className="text-xs text-muted-foreground">- {field.description}</span>
          )}
        </div>
      )

    case 'select':
      return (
        <div className="space-y-2">
          <Label htmlFor={id}>
            {field.label}
            {field.required && <span className="text-destructive ml-1">*</span>}
          </Label>
          <Select
            value={currentValue as string ?? field.default}
            onValueChange={onChange}
            disabled={isReadOnly}
          >
            <SelectTrigger id={id}>
              <SelectValue placeholder={field.placeholder || t('plugins:selectPlaceholder')} />
            </SelectTrigger>
            <SelectContent>
              {field.options?.map((option) => (
                <SelectItem key={option.value} value={option.value as string}>
                  <div className="flex flex-col">
                    <span>{option.label}</span>
                    {option.description && (
                      <span className="text-xs text-muted-foreground">{option.description}</span>
                    )}
                  </div>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          {field.description && (
            <p className="text-xs text-muted-foreground">{field.description}</p>
          )}
        </div>
      )

    case 'text':
      return (
        <div className="space-y-2">
          <Label htmlFor={id}>
            {field.label}
            {field.required && <span className="text-destructive ml-1">*</span>}
          </Label>
          <Textarea
            id={id}
            placeholder={field.placeholder}
            value={currentValue as string ?? ''}
            onChange={(e) => onChange(e.target.value)}
            disabled={isReadOnly}
            rows={3}
            required={field.required}
          />
          {field.description && (
            <p className="text-xs text-muted-foreground">{field.description}</p>
          )}
        </div>
      )

    case 'json': {
      return (
        <JsonFormField
          field={field}
          value={currentValue as string}
          onChange={onChange}
          isReadOnly={isReadOnly ?? false}
          id={id}
        />
      )
    }

    case 'multiselect': {
      return (
        <MultiSelectFormField
          field={field}
          value={currentValue as unknown[] || []}
          onChange={onChange}
          isReadOnly={isReadOnly ?? false}
          id={id}
        />
      )
    }

    case 'array': {
      return (
        <ArrayFormField
          field={field}
          value={currentValue as unknown[] || []}
          onChange={onChange}
          isReadOnly={isReadOnly ?? false}
          id={id}
        />
      )
    }

    case 'object': {
      return (
        <ObjectFormField
          field={field}
          value={currentValue as Record<string, unknown> || {}}
          onChange={onChange}
          isReadOnly={isReadOnly ?? false}
          id={id}
        />
      )
    }

    case 'keyvalue': {
      return (
        <KeyValueFormField
          field={field}
          value={currentValue as Record<string, unknown> || {}}
          onChange={onChange}
          isReadOnly={isReadOnly ?? false}
          id={id}
        />
      )
    }

    default:
      return (
        <div className="space-y-2">
          <Label htmlFor={id}>
            {field.label}
            {field.required && <span className="text-destructive ml-1">*</span>}
          </Label>
          <Input
            id={id}
            placeholder={field.placeholder}
            value={String(currentValue ?? '')}
            onChange={(e) => onChange(e.target.value)}
            disabled={isReadOnly}
          />
          <p className="text-xs text-muted-foreground">{t('plugins:unsupportedFieldType', { type: field.type })}</p>
        </div>
      )
  }
}

// ============================================================================
// JSON Field Component
// ============================================================================

interface JsonFormFieldProps {
  field: FieldSchema
  value: string
  onChange: (value: string) => void
  isReadOnly: boolean
  id: string
}

function JsonFormField({ field, value, onChange, isReadOnly, id }: JsonFormFieldProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const [jsonError, setJsonError] = useState<string | null>(null)

  const handleJsonChange = (newValue: string) => {
    onChange(newValue)
    // Validate JSON
    if (newValue.trim() === '') {
      setJsonError(null)
      return
    }
    try {
      JSON.parse(newValue)
      setJsonError(null)
    } catch {
      setJsonError(t('plugins:invalidJson'))
    }
  }

  const formatJson = () => {
    try {
      const parsed = JSON.parse(value || '{}')
      onChange(JSON.stringify(parsed, null, 2))
      setJsonError(null)
    } catch {
      setJsonError(t('plugins:invalidJson'))
    }
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <Label htmlFor={id}>
          {field.label}
          {field.required && <span className="text-destructive ml-1">*</span>}
        </Label>
        {!isReadOnly && value && (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={formatJson}
            className="h-6 px-2 text-xs"
          >
            <Code className="mr-1 h-3 w-3" />
            {t('plugins:formatJson')}
          </Button>
        )}
      </div>
      <Textarea
        id={id}
        placeholder={field.placeholder || '{"key": "value"}'}
        value={typeof value === 'object' ? JSON.stringify(value, null, 2) : (value || '')}
        onChange={(e) => handleJsonChange(e.target.value)}
        disabled={isReadOnly}
        rows={4}
        className={cn(
          "font-mono text-xs",
          jsonError && "border-destructive focus-visible:ring-destructive"
        )}
        required={field.required}
      />
      {jsonError && (
        <p className="text-xs text-destructive">{jsonError}</p>
      )}
      {field.description && !jsonError && (
        <p className="text-xs text-muted-foreground">{field.description}</p>
      )}
    </div>
  )
}

// ============================================================================
// Multi-Select Field Component
// ============================================================================

interface MultiSelectFormFieldProps {
  field: FieldSchema
  value: unknown[]
  onChange: (value: unknown[]) => void
  isReadOnly: boolean
  id: string
}

function MultiSelectFormField({ field, value, onChange, isReadOnly }: MultiSelectFormFieldProps) {
  const { t } = useTranslation(['plugins', 'common'])

  const toggleOption = (optionValue: unknown) => {
    if (value.includes(optionValue)) {
      onChange(value.filter(v => v !== optionValue))
    } else {
      onChange([...value, optionValue])
    }
  }

  return (
    <div className="space-y-2">
      <Label>
        {field.label}
        {field.required && value.length === 0 && <span className="text-destructive ml-1">*</span>}
      </Label>
      <div className="flex flex-wrap gap-2">
        {field.options?.map((option) => {
          const isSelected = value.includes(option.value)
          return (
            <Badge
              key={option.value as string}
              variant={isSelected ? "default" : "outline"}
              className={cn(
                "cursor-pointer transition-colors",
                isReadOnly && "cursor-not-allowed opacity-60"
              )}
              onClick={() => !isReadOnly && toggleOption(option.value)}
            >
              {option.label}
            </Badge>
          )
        })}
      </div>
      {field.description && (
        <p className="text-xs text-muted-foreground">{field.description}</p>
      )}
      {value.length > 0 && (
        <p className="text-xs text-muted-foreground">
          {t('plugins:selectedCount', { count: value.length })}
        </p>
      )}
    </div>
  )
}

// ============================================================================
// Array Field Component
// ============================================================================

interface ArrayFormFieldProps {
  field: FieldSchema
  value: unknown[]
  onChange: (value: unknown[]) => void
  isReadOnly: boolean
  id: string
}

function ArrayFormField({ field, value, onChange, isReadOnly, id }: ArrayFormFieldProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const [newItemValue, setNewItemValue] = useState('')

  const addItem = () => {
    if (newItemValue.trim()) {
      onChange([...value, newItemValue.trim()])
      setNewItemValue('')
    }
  }

  const removeItem = (index: number) => {
    onChange(value.filter((_, i) => i !== index))
  }

  const updateItem = (index: number, newValue: string) => {
    const updated = [...value]
    updated[index] = newValue
    onChange(updated)
  }

  return (
    <div className="space-y-2">
      <Label htmlFor={id}>
        {field.label}
        {field.required && value.length === 0 && <span className="text-destructive ml-1">*</span>}
      </Label>
      <div className="space-y-2">
        {value.map((item, index) => (
          <div key={index} className="flex items-center gap-2">
            <Hash className="h-4 w-4 text-muted-foreground flex-shrink-0" />
            <Input
              value={String(item)}
              onChange={(e) => updateItem(index, e.target.value)}
              disabled={isReadOnly}
              placeholder={`${field.label} ${index + 1}`}
              className="flex-1"
            />
            {!isReadOnly && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => removeItem(index)}
                className="flex-shrink-0"
              >
                <X className="h-4 w-4" />
              </Button>
            )}
          </div>
        ))}
        {!isReadOnly && (
          <div className="flex items-center gap-2">
            <Input
              value={newItemValue}
              onChange={(e) => setNewItemValue(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && addItem()}
              placeholder={t('plugins:addItemPlaceholder')}
              className="flex-1"
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={addItem}
              disabled={!newItemValue.trim()}
            >
              <Plus className="h-4 w-4 mr-1" />
              {t('common:add')}
            </Button>
          </div>
        )}
      </div>
      {field.description && (
        <p className="text-xs text-muted-foreground">{field.description}</p>
      )}
    </div>
  )
}

// ============================================================================
// Object Field Component
// ============================================================================

interface ObjectFormFieldProps {
  field: FieldSchema
  value: Record<string, unknown>
  onChange: (value: Record<string, unknown>) => void
  isReadOnly: boolean
  id: string
}

function ObjectFormField({ field, value, onChange, id }: ObjectFormFieldProps) {
  const { t } = useTranslation(['plugins', 'common'])

  const updateProperty = (key: string, newValue: unknown) => {
    onChange({ ...value, [key]: newValue })
  }

  if (!field.properties) {
    return (
      <div className="space-y-2">
        <Label htmlFor={id}>{field.label}</Label>
        <p className="text-xs text-muted-foreground">{t('plugins:noPropertiesDefined')}</p>
      </div>
    )
  }

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <Braces className="h-4 w-4 text-muted-foreground" />
        <Label>
          {field.label}
          {field.required && <span className="text-destructive ml-1">*</span>}
        </Label>
      </div>
      <div className="space-y-2 pl-6 border-l-2 border-muted">
        {Object.entries(field.properties).map(([key, propField]) => (
          <FormField
            key={key}
            field={{ ...propField, name: key }}
            value={value[key]}
            onChange={(newValue) => updateProperty(key, newValue)}
          />
        ))}
      </div>
      {field.description && (
        <p className="text-xs text-muted-foreground">{field.description}</p>
      )}
    </div>
  )
}

// ============================================================================
// Key-Value Field Component
// ============================================================================

interface KeyValueFormFieldProps {
  field: FieldSchema
  value: Record<string, unknown>
  onChange: (value: Record<string, unknown>) => void
  isReadOnly: boolean
  id: string
}

function KeyValueFormField({ field, value, onChange, isReadOnly, id }: KeyValueFormFieldProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const [newKey, setNewKey] = useState('')
  const [newValue, setNewValue] = useState('')

  const addPair = () => {
    if (newKey.trim() && newKey.trim() !== '{' && newKey.trim() !== '}') {
      onChange({ ...value, [newKey.trim()]: newValue })
      setNewKey('')
      setNewValue('')
    }
  }

  const removePair = (key: string) => {
    const updated = { ...value }
    delete updated[key]
    onChange(updated)
  }

  const updateValue = (key: string, newValue: string) => {
    onChange({ ...value, [key]: newValue })
  }

  return (
    <div className="space-y-2">
      <Label htmlFor={id}>
        {field.label}
        {field.required && Object.keys(value).length === 0 && <span className="text-destructive ml-1">*</span>}
      </Label>
      <div className="space-y-2">
        {Object.entries(value).map(([key, val]) => (
          <div key={key} className="flex items-center gap-2">
            <Input
              value={key}
              disabled
              className="flex-1 max-w-[150px] font-mono text-xs bg-muted"
            />
            <span className="text-muted-foreground">:</span>
            <Input
              value={String(val)}
              onChange={(e) => updateValue(key, e.target.value)}
              disabled={isReadOnly}
              className="flex-1 font-mono text-sm"
            />
            {!isReadOnly && (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => removePair(key)}
                className="flex-shrink-0"
              >
                <X className="h-4 w-4" />
              </Button>
            )}
          </div>
        ))}
        {!isReadOnly && (
          <div className="flex items-center gap-2">
            <Input
              value={newKey}
              onChange={(e) => setNewKey(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && addPair()}
              placeholder={t('plugins:keyPlaceholder')}
              className="flex-1 max-w-[150px] font-mono text-sm"
            />
            <span className="text-muted-foreground">:</span>
            <Input
              value={newValue}
              onChange={(e) => setNewValue(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && addPair()}
              placeholder={t('plugins:valuePlaceholder')}
              className="flex-1 font-mono text-sm"
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={addPair}
              disabled={!newKey.trim()}
            >
              <Plus className="h-4 w-4 mr-1" />
              {t('common:add')}
            </Button>
          </div>
        )}
      </div>
      {field.description && (
        <p className="text-xs text-muted-foreground">{field.description}</p>
      )}
    </div>
  )
}

function isFieldVisible(field: FieldSchema, config: Record<string, unknown>): boolean {
  if (!field.showWhen) return true

  const { field: targetField, equals, notEquals, contains } = field.showWhen
  const targetValue = config[targetField]

  if (equals !== undefined) {
    return targetValue === equals
  }
  if (notEquals !== undefined) {
    return targetValue !== notEquals
  }
  if (contains !== undefined) {
    return typeof targetValue === 'string' && targetValue.includes(contains)
  }

  return true
}
