/// Vitest test setup
/// Import testing library utilities
import { expect, afterEach, vi } from 'vitest'
import { cleanup } from '@testing-library/react'
import '@testing-library/jest-dom/vitest'

// Cleanup after each test
afterEach(() => {
  cleanup()
})

// Extend Vitest's expect with jest-dom matchers
expect.extend({})

// jsdom lacks CSS.supports (used by useMobile's safe-area detection)
if (typeof (globalThis as { CSS?: { supports?: unknown } }).CSS?.supports !== 'function') {
  ;(window as unknown as { CSS: Record<string, unknown> }).CSS = {
    ...(typeof (globalThis as { CSS?: object }).CSS === 'object' ? (globalThis as { CSS?: object }).CSS : {}),
    supports: () => false,
  }
}

// jsdom lacks matchMedia (used by ThemeProvider / breakpoints)
Object.defineProperty(window, 'matchMedia', {
  writable: true,
  value: (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }),
})

// react-i18next mocked to return keys (assertions match on i18n keys; a
// string defaultValue wins so human-readable labels stay readable)
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      if (typeof opts?.defaultValue === 'string') return opts.defaultValue as string
      return key
    },
    i18n: { language: 'en', changeLanguage: vi.fn() },
  }),
  initReactI18next: { type: '3rdParty', init: vi.fn() },
}))

// No network in tests: fetchAPI is mocked, the rest of @/lib/api passes
// through so isTauriEnv() etc. keep their real behavior
vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>()
  return {
    ...actual,
    fetchAPI: vi.fn().mockResolvedValue(undefined),
  }
})
