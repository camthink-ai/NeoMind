/**
 * Shared background for setup pages — matches the login page so the whole
 * auth flow (login → setup) reads as one surface: base gradient + the
 * device-network field (nodes, links, telemetry pulses).
 */
import { NetworkBackground } from "@/components/shared/NetworkBackground"

export function SetupBackground() {
  return (
    <div className="fixed inset-0">
      {/* Base gradient */}
      <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted" />
      {/* Aurora field — shared with login */}
      <NetworkBackground />
    </div>
  )
}
