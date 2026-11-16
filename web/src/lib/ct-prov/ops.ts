/** CT-Prov/1 v2.0 ops and error codes (6 ops / 9 codes). */

export const ProvOp = {
  deviceInfoGet: 'device.info.get',
  communicationCapabilitiesGet: 'communication.capabilities.get',
  communicationScan: 'communication.scan',
  configSet: 'config.set',
  configGet: 'config.get',
  sessionClose: 'session.close',
} as const

export type ProvOp = (typeof ProvOp)[keyof typeof ProvOp]

export function provOpFromWire(opName: string): ProvOp | null {
  for (const value of Object.values(ProvOp)) {
    if (value === opName) return value
  }
  return null
}

export const ProvErrorCode = {
  invalidMessage: 'INVALID_MESSAGE',
  unsupportedVersion: 'UNSUPPORTED_VERSION',
  unsupportedOperation: 'UNSUPPORTED_OPERATION',
  invalidArgument: 'INVALID_ARGUMENT',
  busy: 'BUSY',
  sectionNotSupported: 'SECTION_NOT_SUPPORTED',
  fieldNotSupported: 'FIELD_NOT_SUPPORTED',
  storageFailed: 'STORAGE_FAILED',
  internalError: 'INTERNAL_ERROR',
} as const

export type ProvErrorCode = (typeof ProvErrorCode)[keyof typeof ProvErrorCode]

export function provErrorCodeFromWire(codeName: string): ProvErrorCode | null {
  for (const value of Object.values(ProvErrorCode)) {
    if (value === codeName) return value
  }
  return null
}

/** Device apply hint — display only; do not poll connectivity from it. */
export type ApplyHint = 'immediate' | 'reconnect' | 'reboot'

export function resultOpName(op: ProvOp | string): string {
  return `${op}.result`
}
