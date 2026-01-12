/**
 * Async operation utilities
 */

/**
 * Debounce function execution
 *
 * @example
 * const debouncedSearch = debounce((query: string) => {
 *   console.log('Searching for:', query)
 * }, 300)
 */
export function debounce<T extends (...args: any[]) => any>(
  func: T,
  wait: number
): (...args: Parameters<T>) => void {
  let timeout: ReturnType<typeof setTimeout> | null = null

  return (...args: Parameters<T>) => {
    if (timeout) clearTimeout(timeout)
    timeout = setTimeout(() => func(...args), wait)
  }
}

/**
 * Throttle function execution
 *
 * @example
 * const throttledScroll = throttle(() => {
 *   console.log('Scrolling')
 * }, 100)
 */
export function throttle<T extends (...args: any[]) => any>(
  func: T,
  limit: number
): (...args: Parameters<T>) => void {
  let inThrottle: boolean = false

  return (...args: Parameters<T>) => {
    if (!inThrottle) {
      func(...args)
      inThrottle = true
      setTimeout(() => inThrottle = false, limit)
    }
  }
}

/**
 * Retry async function with exponential backoff
 *
 * @example
 * const result = await retry(
 *   () => fetch('/api/data').then(r => r.json()),
 *   3,
 *   1000
 * )
 */
export async function retry<T>(
  fn: () => Promise<T>,
  maxRetries: number = 3,
  initialDelay: number = 1000
): Promise<T> {
  let lastError: Error | null = null

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      return await fn()
    } catch (error) {
      lastError = error as Error
      if (attempt < maxRetries) {
        const delay = initialDelay * Math.pow(2, attempt)
        await new Promise(resolve => setTimeout(resolve, delay))
      }
    }
  }

  throw lastError || new Error('Max retries exceeded')
}

/**
 * Add timeout to a promise
 *
 * @example
 * const result = await withTimeout(
 *   fetch('/api/data'),
 *   5000
 * )
 */
export async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number
): Promise<T> {
  const timeoutPromise = new Promise<never>((_, reject) => {
    setTimeout(() => reject(new Error('Operation timeout')), timeoutMs)
  })

  return Promise.race([promise, timeoutPromise])
}

/**
 * Delay execution
 *
 * @example
 * await delay(1000) // Wait 1 second
 */
export function delay(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms))
}

/**
 * Poll a function until it returns true or timeout
 *
 * @example
 * const success = await poll(
 *   () => document.querySelector('.loaded') !== null,
 *   5000,
 *   500
 * )
 */
export async function poll(
  condition: () => boolean | Promise<boolean>,
  timeoutMs: number = 5000,
  intervalMs: number = 500
): Promise<boolean> {
  const startTime = Date.now()

  while (Date.now() - startTime < timeoutMs) {
    if (await condition()) {
      return true
    }
    await delay(intervalMs)
  }

  return false
}
