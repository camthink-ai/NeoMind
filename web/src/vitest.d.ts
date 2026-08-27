/// <reference types="vitest/globals" />

declare global {
  namespace Vi {
    interface FetchMock {
      mockClear(): void
    }
  }

  // eslint-disable-next-line no-var -- ambient global declaration; let/const don't merge into globalThis types
  var fetch: any
}
