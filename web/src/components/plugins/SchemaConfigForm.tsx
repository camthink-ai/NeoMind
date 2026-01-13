import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { cn } from '@/lib/utils'
import { useTranslation } from 'react-i18next'
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
