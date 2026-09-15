/**
 * Shared background for setup pages — matches the login page: flat
 * background color, nothing else. The auth flow reads as one quiet
 * surface; the card carries all the depth.
 */
export function SetupBackground() {
  return <div className="fixed inset-0 bg-background" aria-hidden />
}
