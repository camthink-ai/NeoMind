/**
 * Local storage and session storage utilities with type safety
 */

/**
 * Local storage helpers
 */
export const storage = {
  /**
   * Get item from localStorage
   * @example
   * const user = storage.get<User>('user')
   * const theme = storage.get('theme', 'light')
   */
  get: <T>(key: string, defaultValue?: T): T | null => {
    try {
      const item = localStorage.getItem(key)
      return item ? JSON.parse(item) : defaultValue ?? null
    } catch {
      return defaultValue ?? null
    }
  },

  /**
   * Set item in localStorage
   * @example
   * storage.set('user', { name: 'John' })
   */
  set: <T>(key: string, value: T): void => {
    try {
      localStorage.setItem(key, JSON.stringify(value))
    } catch (error) {
      console.error('Failed to save to localStorage:', error)
    }
  },

  /**
   * Remove item from localStorage
   * @example
   * storage.remove('user')
   */
  remove: (key: string): void => {
    localStorage.removeItem(key)
  },

  /**
   * Clear all localStorage
   */
  clear: (): void => {
    localStorage.clear()
  },

  /**
   * Check if key exists
   */
  has: (key: string): boolean => {
    return localStorage.getItem(key) !== null
  },
}

/**
 * Session storage helpers
 */
export const session = {
  /**
   * Get item from sessionStorage
   */
  get: <T>(key: string, defaultValue?: T): T | null => {
    try {
      const item = sessionStorage.getItem(key)
      return item ? JSON.parse(item) : defaultValue ?? null
    } catch {
      return defaultValue ?? null
    }
  },

  /**
   * Set item in sessionStorage
   */
  set: <T>(key: string, value: T): void => {
    try {
      sessionStorage.setItem(key, JSON.stringify(value))
    } catch (error) {
      console.error('Failed to save to sessionStorage:', error)
    }
  },

  /**
   * Remove item from sessionStorage
   */
  remove: (key: string): void => {
    sessionStorage.removeItem(key)
  },

  /**
   * Clear all sessionStorage
   */
  clear: (): void => {
    sessionStorage.clear()
  },

  /**
   * Check if key exists
   */
  has: (key: string): boolean => {
    return sessionStorage.getItem(key) !== null
  },
}

/**
 * Cookie helpers
 */
export const cookie = {
  /**
   * Get cookie value
   */
  get: (name: string): string | null => {
    const value = `; ${document.cookie}`
    const parts = value.split(`; ${name}=`)
    if (parts.length === 2) {
      return parts.pop()?.split(';').shift() ?? null
    }
    return null
  },

  /**
   * Set cookie
   */
  set: (name: string, value: string, days?: number): void => {
    let expires = ''
    if (days) {
      const date = new Date()
      date.setTime(date.getTime() + days * 24 * 60 * 60 * 1000)
      expires = `; expires=${date.toUTCString()}`
    }
    document.cookie = `${name}=${value}${expires}; path=/`
  },

  /**
   * Remove cookie
   */
  remove: (name: string): void => {
    document.cookie = `${name}=; expires=Thu, 01 Jan 1970 00:00:00 UTC; path=/`
  },
}
