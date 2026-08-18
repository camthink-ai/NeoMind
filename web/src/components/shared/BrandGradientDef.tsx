/**
 * Defines the brand icon gradient paint server — the logo's
 * `#FF8C00 → #FF4500` vertical orange gradient — once at the app root.
 *
 * Any SVG element can then reference it via `stroke="url(#brand-icon-gradient)"`
 * or the `.brand-icon-stroke` CSS class (see index.css; dark-mode scoped).
 * Rendered as a 0×0 hidden SVG: it only contributes the `<defs>`, no layout
 * or visual.
 *
 * Fixed hex stops (not theme-aware) on purpose: the logo itself uses these
 * exact colors on both light and dark, and an icon has no text sitting on it,
 * so a single gradient reads correctly in both themes.
 */
export function BrandGradientDef() {
  return (
    <svg
      width="0"
      height="0"
      className="pointer-events-none absolute"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        {/* userSpaceOnUse + y2=24 (lucide's viewBox) so every path in a
            multi-path icon shares ONE consistent gradient. The default
            objectBoundingBox would gradient each path by its own bbox,
            making icons like Cpu/Bot/Database look patchy/incomplete. */}
        <linearGradient
          id="brand-icon-gradient"
          gradientUnits="userSpaceOnUse"
          x1="0"
          y1="0"
          x2="0"
          y2="24"
        >
          {/* Hard doubled stops → discrete color bands (the logo's segmented
              feel), not a smooth linear blend. Three bands: bright → mid → deep. */}
          <stop offset="0%" stopColor="#FF8C00" />
          <stop offset="33%" stopColor="#FF8C00" />
          <stop offset="33%" stopColor="#FF6900" />
          <stop offset="67%" stopColor="#FF6900" />
          <stop offset="67%" stopColor="#FF4500" />
          <stop offset="100%" stopColor="#FF4500" />
        </linearGradient>
      </defs>
    </svg>
  )
}
