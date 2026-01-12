import { useState, useCallback } from 'react'

export interface UseFormOptions<T> {
  initialValues: T
  onSubmit: (values: T) => Promise<void> | void
  validate?: (values: T) => Record<string, string> | void
}

export interface UseFormReturn<T> {
  values: T
  errors: Record<string, string>
  isSubmitting: boolean
  submitError: string | null
  setValue: <K extends keyof T>(name: K, value: T[K]) => void
  setError: (name: string, error: string) => void
  clearError: (name: string) => void
  handleSubmit: (e?: React.FormEvent) => Promise<void>
  reset: () => void
  setValues: (values: T | ((prev: T) => T)) => void
}

/**
 * Form state management hook
 *
 * Handles form values, validation, errors, and submission.
 *
 * @example
 * const { values, errors, isSubmitting, setValue, handleSubmit } = useForm({
 *   initialValues: { email: '', password: '' },
 *   onSubmit: async (values) => {
 *     await api.login(values)
 *   },
 *   validate: (values) => {
 *     if (!values.email) return { email: 'Email is required' }
 *   },
 * })
 */
export function useForm<T extends Record<string, any>>(
  options: UseFormOptions<T>
): UseFormReturn<T> {
  const [values, setValues] = useState<T>(options.initialValues)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [submitError, setSubmitError] = useState<string | null>(null)

  const setValue = useCallback(<K extends keyof T>(name: K, value: T[K]) => {
    setValues(prev => ({ ...prev, [name]: value }))
    if (errors[name as string]) {
      setErrors(prev => {
        const newErrors = { ...prev }
        delete newErrors[name as string]
        return newErrors
      })
    }
    setSubmitError(null)
  }, [errors])

  const setError = useCallback((name: string, error: string) => {
    setErrors(prev => ({ ...prev, [name]: error }))
  }, [])

  const clearError = useCallback((name: string) => {
    setErrors(prev => {
      const newErrors = { ...prev }
      delete newErrors[name]
      return newErrors
    })
  }, [])

  const handleSubmit = useCallback(async (e?: React.FormEvent) => {
    e?.preventDefault()

    // Validation
    if (options.validate) {
      const validationErrors = options.validate(values)
      if (validationErrors && Object.keys(validationErrors).length > 0) {
        setErrors(validationErrors)
        return
      }
    }

    setIsSubmitting(true)
    setSubmitError(null)

    try {
      await options.onSubmit(values)
    } catch (err) {
      const error = err instanceof Error ? err.message : '提交失败'
      setSubmitError(error)
    } finally {
      setIsSubmitting(false)
    }
  }, [values, options])

  const reset = useCallback(() => {
    setValues(options.initialValues)
    setErrors({})
    setSubmitError(null)
  }, [options.initialValues])

  return {
    values,
    errors,
    isSubmitting,
    submitError,
    setValue,
    setError,
    clearError,
    handleSubmit,
    reset,
    setValues,
  }
}

/**
 * Form field hook for individual inputs
 */
export function useFormField<T = any>(value: T, onChange: (value: T) => void) {
  return {
    value,
    onChange: (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => {
      onChange(e.target.value as T)
    },
  }
}
