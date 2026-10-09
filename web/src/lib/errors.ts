/**
 * Error Handling Utilities
 *
 * Centralized error handling utilities for consistent error management across the application.
 */

// ============================================================================
// Types
// ============================================================================

/**
 * Standard error types that can be handled uniformly
 */
export type StandardError =
  | Error
  | { error: { code?: string; message?: string; hint?: string } }
  | { message: string }
  | { status?: number }
  | unknown

/**
 * Error context for better error reporting
 */
export interface ErrorContext {
  /** The operation that failed */
  operation?: string
  /** Additional context about the error */
  context?: Record<string, unknown>
  /** Whether to show user-facing notification */
  silent?: boolean
  /** Custom error message for user */
  userMessage?: string
}

// ============================================================================
// Error Extraction Utilities
// ============================================================================

/**
 * Extract error message from various error types
 */
export function getErrorMessage(error: StandardError): string {
  if (!error) return 'Unknown error'

  // Error object
  if (error instanceof Error) {
    return error.message
  }

  // API error response format
  if (typeof error === 'object' && error !== null) {
    const err = error as Record<string, unknown>

    // Check for nested error object. The API's `hint` (agent/user-facing
    // recovery guidance) rides along — dropping it here left users with the
    // bare message, same class of loss as the CLI client's extractor.
    if (err.error && typeof err.error === 'object') {
      const nestedErr = err.error as { message?: string; code?: string; hint?: string }
      const hintSuffix = nestedErr.hint ? `\n${nestedErr.hint}` : ''
      if (nestedErr.message) return `${nestedErr.message}${hintSuffix}`
      if (nestedErr.code) return `Error: ${nestedErr.code}${hintSuffix}`
    }

    // Check for direct message property
    if (err.message && typeof err.message === 'string') {
      return err.message
    }

    // Check for status code
    if (err.status && typeof err.status === 'number') {
      return getStatusMessage(err.status)
    }
  }

  // String error
  if (typeof error === 'string') {
    return error
  }

  return 'An unexpected error occurred'
}

/**
 * Get HTTP status message
 */
function getStatusMessage(status: number): string {
  const statusMessages: Record<number, string> = {
    400: 'Bad request - Please check your input',
    401: 'Unauthorized - Please log in again',
    403: 'Forbidden - You don\'t have permission',
    404: 'Not found - The requested resource doesn\'t exist',
    409: 'Conflict - The request conflicts with current state',
    422: 'Validation failed - Please check your input',
    429: 'Too many requests - Please try again later',
    500: 'Server error - Please try again later',
    502: 'Service unavailable - Backend is not responding',
    503: 'Service unavailable - Please try again later',
  }
  return statusMessages[status] || `HTTP ${status} error`
}

/**
 * Extract error code if available
 */
export function getErrorCode(error: StandardError): string | undefined {
  if (!error || typeof error !== 'object') return undefined

  const err = error as Record<string, unknown>

  // Check for nested error code
  if (err.error && typeof err.error === 'object') {
    const nestedErr = err.error as { code?: string }
    return nestedErr.code
  }

  // Check for direct code property
  if (err.code && typeof err.code === 'string') {
    return err.code
  }

  // Check for HTTP status
  if (err.status && typeof err.status === 'number') {
    return err.status.toString()
  }

  return undefined
}

// ============================================================================
// Error Logging Utilities
// ============================================================================

/**
 * Log error with consistent format
 * @param error - The error to log
 * @param context - Additional context about the error
 */
export function logError(error: StandardError, context?: ErrorContext): void {
  const operation = context?.operation || 'Operation'
  const message = getErrorMessage(error)
  const code = getErrorCode(error)

  console.error(`[${operation}]`, {
    message,
    code,
    context: context?.context,
    originalError: error,
  })
}

/**
 * Log warning with consistent format
 */
export function logWarning(message: string, context?: Record<string, unknown>): void {
  console.warn(`[Warning]`, {
    message,
    context,
  })
}

// ============================================================================
// Error Classification Utilities
// ============================================================================

/**
 * Check if error is a network error
 */
export function isNetworkError(error: StandardError): boolean {
  const message = getErrorMessage(error).toLowerCase()
  return (
    message.includes('network') ||
    message.includes('fetch') ||
    message.includes('connection') ||
    message.includes('timeout') ||
    message.includes('load failed') ||  // Safari/WebKit network error
    message.includes('failed to fetch')  // Chrome/Firefox network error
  )
}

/**
 * Check if error is an authentication error
 */
export function isAuthError(error: StandardError): boolean {
  const code = getErrorCode(error)
  const message = getErrorMessage(error).toLowerCase()

  return (
    code === '401' ||
    code === 'UNAUTHORIZED' ||
    message.includes('unauthorized') ||
    message.includes('authentication') ||
    message.includes('token')
  )
}

/**
 * Check if error is a validation error
 */
export function isValidationError(error: StandardError): boolean {
  const code = getErrorCode(error)
  const message = getErrorMessage(error).toLowerCase()

  return (
    code === '400' ||
    code === '422' ||
    code === 'VALIDATION_ERROR' ||
    message.includes('validation') ||
    message.includes('invalid')
  )
}

/**
 * Check if error is a not found error
 */
export function isNotFoundError(error: StandardError): boolean {
  const code = getErrorCode(error)
  return code === '404' || code === 'NOT_FOUND'
}

/**
 * Check if error is a conflict error
 */
export function isConflictError(error: StandardError): boolean {
  const code = getErrorCode(error)
  return code === '409' || code === 'CONFLICT'
}

// ============================================================================
// Error Handling React Hook Support
// ============================================================================

/**
 * Format error for user display
 * @param error - The error to format
 * @param context - Additional context
 * @returns User-friendly error message
 */
export function formatErrorForUser(error: StandardError, context?: ErrorContext): string {
  // Use custom user message if provided
  if (context?.userMessage) {
    return context.userMessage
  }

  const message = getErrorMessage(error)

  // Provide user-friendly messages for common errors
  if (isNetworkError(error)) {
    // Check for specific timeout case
    if (message.toLowerCase().includes('timeout')) {
      return 'Request timed out. The operation is taking too long - please try again later.'
    }
    return 'Network error. Please check that the server is running and try again.'
  }

  if (isAuthError(error)) {
    return 'Your session has expired. Please log in again.'
  }

  if (isValidationError(error)) {
    return 'Please check your input and try again.'
  }

  if (isNotFoundError(error)) {
    return 'The requested resource was not found.'
  }

  return message
}

// ============================================================================
// Async Error Wrapper
// ============================================================================

/**
 * Wrap an async function with standardized error handling
 * @param operation - Operation name for logging
 * @param fn - The async function to wrap
 * @returns Wrapped function with error handling
 */
export function withErrorHandling<T extends (...args: unknown[]) => Promise<unknown>>(
  operation: string,
  fn: T
): T {
  return (async (...args: Parameters<T>) => {
    try {
      return await fn(...args)
    } catch (error) {
      logError(error as StandardError, { operation })
      throw error
    }
  }) as T
}
