import { useForm } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'
import { useState, useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { Button } from '@/components/ui/button'
import { Loader2, Eye, EyeOff } from 'lucide-react'
import type { PluginConfigSchema } from '@/types'

interface ConfigFormBuilderProps {
  schema: PluginConfigSchema
  initialValues?: Record<string, unknown>
  onSubmit: (values: Record<string, unknown>) => Promise<void>
  loading?: boolean
  submitLabel?: string
}

/**
 * Build a Zod schema from a PluginConfigSchema
 */
function buildZodSchema(schema: PluginConfigSchema): z.ZodType<Record<string, unknown>> {
  const shape: Record<string, z.ZodTypeAny> = {}

  for (const [fieldName, prop] of Object.entries(schema.properties)) {
    let fieldSchema: z.ZodTypeAny

    // Build base schema based on type
    switch (prop.type) {
      case 'string':
        fieldSchema = z.string()
        if (prop.enum && prop.enum.length > 0) {
          fieldSchema = z.enum(prop.enum as [string, ...string[]])
        }
        break
      case 'number':
        // Build number schema with constraints first, then add string coercion
        let numSchema = z.number()
        if (prop.minimum !== undefined) {
          numSchema = numSchema.min(prop.minimum)
        }
        if (prop.maximum !== undefined) {
          numSchema = numSchema.max(prop.maximum)
        }
        // Allow string input that will be converted to number
        fieldSchema = numSchema.or(z.string().transform((v) => Number(v)))
        break
      case 'boolean':
        fieldSchema = z.boolean().or(z.coerce.boolean())
        break
      case 'array':
        fieldSchema = z.array(z.any())
        break
      case 'object':
      default:
        fieldSchema = z.record(z.string(), z.any())
        break
    }

    // Apply default value
    if (prop.default !== undefined) {
      fieldSchema = fieldSchema.optional().default(prop.default) as z.ZodTypeAny
    } else if (!schema.required?.includes(fieldName)) {
      fieldSchema = fieldSchema.optional() as z.ZodTypeAny
    }

    shape[fieldName] = fieldSchema
  }

  return z.object(shape) as any
}

/**
 * Get display name for a field
 */
function getFieldDisplayName(
  fieldName: string,
  uiHints: PluginConfigSchema['ui_hints']
): string {
  return uiHints?.display_names?.[fieldName] || fieldName
}

/**
 * Get help text for a field
 */
function getFieldHelpText(
  fieldName: string,
  uiHints: PluginConfigSchema['ui_hints'],
  property: { description?: string }
): string | undefined {
  return (
    uiHints?.help_texts?.[fieldName] ||
    property.description
  )
}

/**
 * Check if a field should be shown based on visibility rules
 */
function isFieldVisible(
  fieldName: string,
  values: Record<string, unknown>,
  uiHints: PluginConfigSchema['ui_hints']
): boolean {
  if (!uiHints?.visibility_rules) return true

  for (const rule of uiHints.visibility_rules) {
    if (rule.then_show.includes(fieldName)) {
      const fieldValue = values[rule.field]
      let show = false

      switch (rule.condition) {
        case 'equals':
          show = fieldValue === rule.value
          break
        case 'not_equals':
          show = fieldValue !== rule.value
          break
        case 'contains':
          show = Array.isArray(fieldValue) && fieldValue.includes(rule.value)
          break
        case 'empty':
          show = !fieldValue || (Array.isArray(fieldValue) && fieldValue.length === 0)
          break
        case 'not_empty':
          show = !!fieldValue && (!Array.isArray(fieldValue) || fieldValue.length > 0)
          break
      }

      if (show) return true
    }
  }

  // If there are visibility rules but this field isn't in any then_show,
  // check if it's in the required list or properties
  if (uiHints.visibility_rules.length > 0) {
    const isInThenShow = uiHints.visibility_rules.some(r =>
      r.then_show.includes(fieldName)
    )
    if (isInThenShow) return true

    // Fields not in visibility rules are always shown
    return !uiHints.visibility_rules.some(r => r.then_show.includes(fieldName))
  }

  return true
}

export function ConfigFormBuilder({
  schema,
  initialValues,
  onSubmit,
  loading = false,
  submitLabel,
}: ConfigFormBuilderProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const zodSchema = buildZodSchema(schema)

  // Memoize field order to prevent infinite re-renders
  const fieldOrder = useMemo(
    () => schema.ui_hints?.field_order || Object.keys(schema.properties),
    [schema.ui_hints?.field_order, schema.properties]
  )

  const { register, handleSubmit, watch, formState: { errors }, setValue } = useForm({
    resolver: zodResolver(zodSchema as any),
    defaultValues: initialValues || {},
  })

  const watchedValues = watch()
  const [visibleFields, setVisibleFields] = useState<Set<string>>(new Set(fieldOrder))
  const [secretVisible, setSecretVisible] = useState<Record<string, boolean>>({})

  // Update visible fields when watched values change
  useEffect(() => {
    const newVisible = new Set<string>()
    for (const field of fieldOrder) {
      if (isFieldVisible(field, watchedValues, schema.ui_hints)) {
        newVisible.add(field)
      }
    }
    setVisibleFields(newVisible)
  }, [watchedValues, schema.ui_hints, fieldOrder])

  const handleFormSubmit = async (values: Record<string, unknown>) => {
    await onSubmit(values)
  }

  const toggleSecretVisibility = (fieldName: string) => {
    setSecretVisible(prev => ({ ...prev, [fieldName]: !prev[fieldName] }))
  }

  return (
    <form onSubmit={handleSubmit(handleFormSubmit)} className="space-y-4">
      {fieldOrder
        .filter(fieldName => schema.properties[fieldName] && visibleFields.has(fieldName))
        .map((fieldName) => {
          const prop = schema.properties[fieldName]
          if (!prop) return null

          const displayName = getFieldDisplayName(fieldName, schema.ui_hints)
          const helpText = getFieldHelpText(fieldName, schema.ui_hints, prop)
          const isSecret = prop.secret
          const showSecret = secretVisible[fieldName] || false
          const error = errors[fieldName]

          return (
            <div key={fieldName} className="space-y-2">
              <Label htmlFor={fieldName}>{displayName}</Label>

              {prop.type === 'boolean' ? (
                <div className="flex items-center space-x-2">
                  <Switch
                    id={fieldName}
                    checked={!!watchedValues[fieldName]}
                    onCheckedChange={(checked) => setValue(fieldName, checked)}
                  />
                  <span className="text-sm text-muted-foreground">
                    {watchedValues[fieldName] ? t('plugins:yes') : t('plugins:no')}
                  </span>
                </div>
              ) : prop.enum && prop.enum.length > 0 ? (
                <Select
                  value={String(watchedValues[fieldName] || '')}
                  onValueChange={(value) => setValue(fieldName, value)}
                >
                  <SelectTrigger id={fieldName}>
                    <SelectValue placeholder={t('plugins:selectPlaceholder')} />
                  </SelectTrigger>
                  <SelectContent>
                    {prop.enum.map((value) => (
                      <SelectItem key={String(value)} value={String(value)}>
                        {String(value)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : prop.type === 'array' ? (
                <Textarea
                  id={fieldName}
                  value={Array.isArray(watchedValues[fieldName]) ? (watchedValues[fieldName] as unknown[]).join('\n') : ''}
                  onChange={(e) => setValue(fieldName, e.target.value.split('\n'))}
                  placeholder={t('plugins:onePerLine')}
                  rows={3}
                />
              ) : (
                <div className="relative">
                  <Input
                    id={fieldName}
                    type={isSecret && !showSecret ? 'password' : 'text'}
                    placeholder={schema.ui_hints?.placeholders?.[fieldName]}
                    defaultValue={watchedValues[fieldName] as string | number | undefined}
                    {...register(fieldName as any)}
                  />
                  {isSecret && (
                    <button
                      type="button"
                      onClick={() => toggleSecretVisibility(fieldName)}
                      className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                    >
                      {showSecret ? (
                        <EyeOff className="h-4 w-4" />
                      ) : (
                        <Eye className="h-4 w-4" />
                      )}
                    </button>
                  )}
                </div>
              )}

              {helpText && (
                <p className="text-xs text-muted-foreground">{helpText}</p>
              )}

              {error && (
                <p className="text-xs text-destructive">
                  {error.message as string || t('plugins:fieldError')}
                </p>
              )}
            </div>
          )
        })}

      <Button type="submit" disabled={loading} className="w-full">
        {loading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
        {submitLabel}
      </Button>
    </form>
  )
}
