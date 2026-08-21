/**
 * Shared background for setup pages — matches the login page so the whole
 * auth flow (login → setup) reads as one surface: base gradient + brand-orange
 * honeycomb breathe + a single soft brand glow.
 */
import { HoneycombBackground } from "@/components/shared/HoneycombBackground"

export function SetupBackground() {
  return (
    <div className="fixed inset-0">
      {/* Base gradient */}
      <div className="absolute inset-0 bg-gradient-to-br from-background via-background to-muted" />
      {/* Honeycomb mesh — shared with login */}
      <HoneycombBackground />
      {/* One soft brand glow — restrained, just enough warmth to avoid
          feeling flat. Brand orange ties to the logo. */}
      <div
        className="absolute top-[45%] left-1/2 -translate-x-1/2 w-[42rem] h-[42rem] rounded-full blur-3xl"
        style={{ background: 'color-mix(in oklch, var(--accent-orange) 8%, transparent)' }}
      />
    </div>
  )
}
