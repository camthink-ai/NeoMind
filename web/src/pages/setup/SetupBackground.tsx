/**
 * Shared background for setup pages — matches the login page:
 * flat `bg-background` plus the faint fading canvas dot grid.
 */
export function SetupBackground() {
  return (
    <div
      aria-hidden="true"
      className="pointer-events-none absolute inset-0"
      style={{
        backgroundImage: 'radial-gradient(circle, var(--border) 1px, transparent 1.2px)',
        backgroundSize: '22px 22px',
        maskImage: 'radial-gradient(ellipse 60% 55% at 50% 45%, black 20%, transparent 80%)',
        WebkitMaskImage: 'radial-gradient(ellipse 60% 55% at 50% 45%, black 20%, transparent 80%)',
      }}
    />
  )
}
