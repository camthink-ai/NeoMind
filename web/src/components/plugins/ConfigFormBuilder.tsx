import { useForm, Controller, useWatch } from 'react-hook-form'
import { zodResolver } from '@hookform/resolvers/zod'
import { z } from 'zod'
import { useState, useMemo, useCallback, useRef } from 'react'
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
import { Loader2, Eye, EyeOff, Upload } from 'lucide-react'
import type { PluginConfigSchema } from '@/types'
import { validateMqttTopics } from './ConfigFormBuilder.validation'

interface ConfigFormBuilderProps {
  schema: PluginConfigSchema
  initialValues?: Record<string, unknown>
  onSubmit: (values: Record<string, unknown>) => Promise<void>
  loading?: boolean
  submitLabel?: string
  /** ID to assign to the <form> element. Use when a footer button outside
   * the form needs to trigger submission via the HTML `form` attribute. */
  formId?: string
  /** When true, the inline submit button is not rendered. The parent is
   * responsible for providing a submit trigger (typically a footer button
   * bound to `formId`). */
  hideSubmitButton?: boolean
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
      case 'integer':
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
        // For MQTT subscribe_topics, add custom validation
        if (fieldName === 'subscribe_topics' || fieldName === 'subscribeTopics') {
          fieldSchema = z.array(z.string())
            .transform((topics) => topics.filter(t => t.trim())) // Remove empty lines
            .refine(
              (topics) => {
                if (topics.length === 0) return true // Allow empty array
                const result = validateMqttTopics(topics)
                return result.valid
              },
              { message: 'Invalid MQTT topic format' }
            )
        } else {
          fieldSchema = z.array(z.any())
        }
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
 * Extract default values from schema properties
 */
function getDefaultValues(
  schema: PluginConfigSchema
): Record<string, unknown> {
  const defaults: Record<string, unknown> = {}

  for (const [fieldName, prop] of Object.entries(schema.properties)) {
    if (prop.default !== undefined) {
      defaults[fieldName] = prop.default
    }
  }

  return defaults
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
    const isInThenShow = uiHints.visibility_rules.some((r: { then_show: string[] }) =>
      r.then_show.includes(fieldName)
    )
    if (isInThenShow) return true

    // Fields not in visibility rules are always shown
    return !uiHints.visibility_rules.some((r: { then_show: string[] }) => r.then_show.includes(fieldName))
  }

  return true
}

/**
 * Form field component - renders individual fields without causing parent re-renders
 */
interface FormFieldProps {
  fieldName: string
  prop: PluginConfigSchema['properties'][string]
  schema: PluginConfigSchema
  control: any
  register: any
  errors: Record<string, any>
  setValue: (name: string, value: any) => void
  t: (key: string, options?: any) => string
}

function FormField({
  fieldName,
  prop,
  schema,
  control,
  register,
  errors,
  setValue,
  t
}: FormFieldProps) {
  const [secretVisible, setSecretVisible] = useState(false)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const displayName = getFieldDisplayName(fieldName, schema.ui_hints)
  const helpText = getFieldHelpText(fieldName, schema.ui_hints, prop)
  const isSecret = prop.secret
  const isMultiline = (prop as any).format === 'multiline'
  const error = errors[fieldName]

  // Handle file upload for certificate fields
  const handleFileUpload = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (file) {
      const reader = new FileReader()
      reader.onload = (event) => {
        const content = event.target?.result as string
        setValue(fieldName, content.trim())
      }
      reader.readAsText(file)
    }
    // Reset input so same file can be uploaded again
    e.target.value = ''
  }, [fieldName, setValue])

  const triggerFileUpload = useCallback(() => {
    fileInputRef.current?.click()
  }, [])

  return (
    <div className="space-y-2">
      <Label htmlFor={fieldName}>{displayName}</Label>

      {prop.type === 'boolean' ? (
        <Controller
          name={fieldName}
          control={control}
          render={({ field }) => (
            <div className="flex items-center space-x-2">
              <Switch
                id={fieldName}
                checked={!!field.value}
                onCheckedChange={field.onChange}
              />
              <span className="text-sm text-muted-foreground">
                {field.value ? t('plugins:yes') : t('plugins:no')}
              </span>
            </div>
          )}
        />
      ) : prop.enum && prop.enum.length > 0 ? (
        <Controller
          name={fieldName}
          control={control}
          render={({ field }) => (
            <Select value={String(field.value || '')} onValueChange={field.onChange}>
              <SelectTrigger id={fieldName}>
                <SelectValue placeholder={t('plugins:selectPlaceholder')} />
              </SelectTrigger>
              <SelectContent>
                {prop.enum!.map((value) => (
                  <SelectItem key={String(value)} value={String(value)}>
                    {String(value)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        />
      ) : prop.type === 'array' ? (
        <Controller
          name={fieldName}
          control={control}
          render={({ field }) => (
            <Textarea
              id={fieldName}
              value={Array.isArray(field.value) ? (field.value as unknown[]).join('\n') : ''}
              onChange={(e) => field.onChange(e.target.value.split('\n'))}
              placeholder={
                (fieldName === 'subscribe_topics' || fieldName === 'subscribeTopics')
                  ? `ne301/+\nne301/+/upload/report\nsensor/+/data`
                  : t('plugins:onePerLine')
              }
              rows={4}
            />
          )}
        />
      ) : prop.type === 'object' || (prop.type as any) === 'record' ? (
        <Controller
          name={fieldName}
          control={control}
          render={({ field }) => (
            <Textarea
              id={fieldName}
              value={typeof field.value === 'object' ? JSON.stringify(field.value, null, 2) : String(field.value || '')}
              onChange={(e) => {
                try {
                  const parsed = JSON.parse(e.target.value)
                  field.onChange(parsed)
                } catch {
                  // Invalid JSON, keep as string for now
                  field.onChange(e.target.value)
                }
              }}
              placeholder={t('plugins:config.jsonPlaceholder')}
              rows={4}
              className="font-mono text-xs"
            />
          )}
        />
      ) : prop.type === 'number' || prop.type === 'integer' ? (
        <Controller
          name={fieldName}
          control={control}
          render={({ field }) => (
            <Input
              id={fieldName}
              type="number"
              inputMode="numeric"
              value={field.value ?? ''}
              onChange={(e) => {
                const value = e.target.value
                // Allow empty string or convert to number
                if (value === '') {
                  field.onChange('')
                } else {
                  const num = Number(value)
                  field.onChange(isNaN(num) ? value : num)
                }
              }}
              placeholder={schema.ui_hints?.placeholders?.[fieldName]}
            />
          )}
        />
      ) : isMultiline ? (
        <Controller
          name={fieldName}
          control={control}
          render={({ field }) => (
            <div className="space-y-2">
              <Textarea
                id={fieldName}
                value={String(field.value || '')}
                onChange={(e) => field.onChange(e.target.value)}
                placeholder={schema.ui_hints?.placeholders?.[fieldName]}
                rows={6}
                className="font-mono text-xs"
              />
              <div className="flex items-center gap-2">
                <input
                  ref={fileInputRef}
                  type="file"
                  accept=".pem,.crt,.cer,.key"
                  onChange={handleFileUpload}
                  className="hidden"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={triggerFileUpload}
                  className="text-xs"
                >
                  <Upload className="h-4 w-4 mr-1" />
                  {t('plugins:uploadFile', { defaultValue: 'Upload File' })}
                </Button>
                <span className="text-xs text-muted-foreground">
                  {t('plugins:orPaste', { defaultValue: 'or paste content above' })}
                </span>
              </div>
            </div>
          )}
        />
      ) : (
        <div className="relative">
          <Input
            id={fieldName}
            type={isSecret && !secretVisible ? 'password' : 'text'}
            placeholder={schema.ui_hints?.placeholders?.[fieldName]}
            {...register(fieldName)}
          />
          {isSecret && (
            <button
              type="button"
              onClick={() => setSecretVisible(!secretVisible)}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
            >
              {secretVisible ? (
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
        <p className="text-xs text-error">
          {error.message as string || t('plugins:fieldError')}
        </p>
      )}
    </div>
  )
}

export function ConfigFormBuilder({
  schema,
  initialValues,
  onSubmit,
  loading = false,
  submitLabel,
  formId,
  hideSubmitButton = false,
}: ConfigFormBuilderProps) {
  const { t } = useTranslation(['plugins', 'common'])
  const zodSchema = buildZodSchema(schema)

  // Memoize field order to prevent infinite re-renders
  const fieldOrder = useMemo(
    () => schema.ui_hints?.field_order || Object.keys(schema.properties),
    [schema.ui_hints?.field_order, schema.properties]
  )

  // Extract default values from schema
  const schemaDefaults = useMemo(() => getDefaultValues(schema), [schema])

  // Merge schema defaults with provided initialValues (initialValues take precedence)
  const defaultValues = useMemo(() => ({
    ...schemaDefaults,
    ...initialValues,
  }), [schemaDefaults, initialValues])

  const { register, handleSubmit, control, formState: { errors }, setValue } = useForm({
    resolver: zodResolver(zodSchema as any),
    defaultValues,
  })

  // Get visibility-related fields only for re-render optimization
  const visibilityFieldNames = useMemo(() => {
    if (!schema.ui_hints?.visibility_rules?.length) return []
    return schema.ui_hints.visibility_rules.map((r: { field: string }) => r.field)
  }, [schema.ui_hints?.visibility_rules])

  // Use useWatch to only watch fields that affect visibility
  // This prevents re-renders when other fields change
  const watchedVisibilityValues = useWatch({
    control,
    name: visibilityFieldNames,
  })

  // Build visibility values object from watched array
  const visibilityValues = useMemo(() => {
    if (!visibilityFieldNames.length) return {}
    const values: Record<string, unknown> = {}
    visibilityFieldNames.forEach((field: string, index: number) => {
      values[field] = watchedVisibilityValues?.[index]
    })
    return values
  }, [watchedVisibilityValues, visibilityFieldNames])

  // Compute visible fields based on visibility values
  const visibleFields = useMemo(() => {
    const newVisible = new Set<string>()
    // If no visibility rules, all fields are visible
    if (!schema.ui_hints?.visibility_rules?.length) {
      for (const field of fieldOrder) {
        newVisible.add(field)
      }
      return newVisible
    }
    // Otherwise, calculate visibility based on watched fields
    for (const field of fieldOrder) {
      if (isFieldVisible(field, visibilityValues, schema.ui_hints)) {
        newVisible.add(field)
      }
    }
    return newVisible
  }, [visibilityValues, schema.ui_hints, fieldOrder])

  const handleFormSubmit = async (values: Record<string, unknown>) => {
    // Convert string values to proper types based on schema
    const convertedValues: Record<string, unknown> = {}
    for (const [key, value] of Object.entries(values)) {
      const prop = schema.properties[key]
      if (prop) {
        if ((prop.type === 'number' || prop.type === 'integer') && typeof value === 'string') {
          // Convert string to number for numeric fields
          const num = Number(value)
          convertedValues[key] = isNaN(num) ? value : num
        } else if (prop.type === 'boolean' && typeof value === 'string') {
          // Convert string to boolean for boolean fields
          convertedValues[key] = value === 'true'
        } else {
          convertedValues[key] = value
        }
      } else {
        convertedValues[key] = value
      }
    }
    await onSubmit(convertedValues)
  }

  return (
    <form id={formId} onSubmit={handleSubmit(handleFormSubmit)} className="space-y-4">
      {fieldOrder
        .filter((fieldName: string) => schema.properties[fieldName] && visibleFields.has(fieldName))
        .map((fieldName: string) => {
          const prop = schema.properties[fieldName]
          if (!prop) return null

          return (
            <FormField
              key={fieldName}
              fieldName={fieldName}
              prop={prop}
              schema={schema}
              control={control}
              register={register}
              errors={errors}
              setValue={setValue}
              t={t}
            />
          )
        })}

      {!hideSubmitButton && (
        <Button type="submit" disabled={loading} className="w-full">
          {loading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
          {submitLabel}
        </Button>
      )}
    </form>
  )
}
