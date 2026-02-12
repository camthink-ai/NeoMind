/// Vitest environment declarations
declare namespace Vi {
  interface JestAssertion<T = any> extends jest.Matchers<void, T> {}
  interface AsymmetricMatchersContaining {
    stringContaining(str: string): unknown
    objectContaining(obj: Record<string, unknown>): unknown
  }
}
