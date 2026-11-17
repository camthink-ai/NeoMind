/**
 * Process-wide holder for the active CT-Prov BLE GATT session.
 *
 * BLE links occupy the device (BUSY) until released. Dialog unmount alone is
 * not enough when the operator navigates SPA routes while the Add Device
 * dialog / zustand flag stays around — callers register a release hook here
 * and route / dialog guards call {@link releaseActiveBleSession}.
 */

type ReleaseFn = () => void

let releaseFn: ReleaseFn | null = null
let generation = 0

/**
 * Register the release hook for the currently held BLE session.
 * Replaces any previous session (releasing it first).
 * Returns an unregister function that does **not** release — only clears
 * the registry when this registration is still current.
 */
export function retainActiveBleSession(release: ReleaseFn): () => void {
  if (releaseFn && releaseFn !== release) {
    const prev = releaseFn
    releaseFn = null
    try {
      prev()
    } catch {
      /* ignore prior session release errors */
    }
  }
  const gen = ++generation
  releaseFn = release
  return () => {
    if (generation === gen && releaseFn === release) {
      releaseFn = null
    }
  }
}

/** Drop the active BLE session if any (safe to call when none). */
export function releaseActiveBleSession(): void {
  const fn = releaseFn
  releaseFn = null
  if (!fn) return
  try {
    fn()
  } catch {
    /* ignore */
  }
}

export function hasActiveBleSession(): boolean {
  return releaseFn != null
}
