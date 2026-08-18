/** @type {import('tailwindcss').Config} */
import containerQueries from '@tailwindcss/container-queries'

export default {
  darkMode: ["class"],
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      fontFamily: {
        sans: ['"Plus Jakarta Sans"', '"Noto Sans SC"', 'system-ui', '-apple-system', 'sans-serif'],
        mono: ['"JetBrains Mono"', '"Fira Code"', 'ui-monospace', 'SFMono-Regular', '"SF Mono"', 'Menlo', 'Consolas', 'monospace'],
      },
      // Semantic type scale below text-xs (12px) — the app's dense UI sizes.
      // Single source for both the text-* utilities and the JS constants in
      // design-system/tokens/typography.ts. Adjust sizes HERE, never by
      // reintroducing text-[Npx] literals (DESIGN_SPEC §2). Each size pairs
      // a tuned line-height (~1.3–1.45) so plain usage gets a healthy rhythm;
      // explicit leading-* still overrides.
      fontSize: {
        micro: ["9px", { lineHeight: "12px" }],     // extreme micro labels, data type labels
        nano: ["10px", { lineHeight: "14px" }],     // timestamps, tiny metadata, compact badges
        mini: ["11px", { lineHeight: "16px" }],     // badge text, secondary labels, tab labels
        code: ["12px", { lineHeight: "17px" }],     // inline code, code snippets
        body: ["13px", { lineHeight: "19px" }],     // chat messages, tool call text, markdown body
        heading: ["15px", { lineHeight: "22px" }],  // markdown headings
      },
      colors: {
        border: "var(--border)",
        input: "var(--input)",
        ring: "var(--ring)",
        background: "var(--background)",
        foreground: "var(--foreground)",
        primary: {
          DEFAULT: "var(--primary)",
          foreground: "var(--primary-foreground)",
          hover: "var(--primary-hover)",
          light: "var(--primary-bg)",
          lightHover: "var(--primary-bg-hover)",
        },
        secondary: {
          DEFAULT: "var(--secondary)",
          foreground: "var(--secondary-foreground)",
          hover: "var(--secondary-hover)",
        },
        destructive: {
          DEFAULT: "var(--destructive)",
          foreground: "var(--destructive-foreground)",
          hover: "var(--destructive-hover)",
          light: "var(--destructive-bg)",
        },
        muted: {
          DEFAULT: "var(--muted)",
          foreground: "var(--muted-foreground)",
          20: "var(--muted-20)",
          30: "var(--muted-30)",
          50: "var(--muted-50)",
        },
        accent: {
          DEFAULT: "var(--accent)",
          foreground: "var(--accent-foreground)",
        },
        popover: {
          DEFAULT: "var(--popover)",
          foreground: "var(--popover-foreground)",
        },
        card: {
          DEFAULT: "var(--card)",
          foreground: "var(--card-foreground)",
        },
        success: {
          DEFAULT: "var(--color-success)",
          light: "var(--color-success-bg)",
        },
        warning: {
          DEFAULT: "var(--color-warning)",
          light: "var(--color-warning-bg)",
        },
        error: {
          DEFAULT: "var(--color-error)",
          light: "var(--color-error-bg)",
        },
        info: {
          DEFAULT: "var(--color-info)",
          light: "var(--color-info-bg)",
        },
        // Accent category colors (OKLCH-harmonized)
        "accent-purple": {
          DEFAULT: "var(--accent-purple)",
          light: "var(--accent-purple-bg)",
        },
        "accent-orange": {
          DEFAULT: "var(--accent-orange)",
          light: "var(--accent-orange-bg)",
        },
        "accent-cyan": {
          DEFAULT: "var(--accent-cyan)",
          light: "var(--accent-cyan-bg)",
        },
        "accent-emerald": {
          DEFAULT: "var(--accent-emerald)",
          light: "var(--accent-emerald-bg)",
        },
        "accent-indigo": {
          DEFAULT: "var(--accent-indigo)",
          light: "var(--accent-indigo-bg)",
        },
        // Glass tokens
        brand: {
          DEFAULT: "var(--brand)",
          hover: "var(--brand-hover)",
          active: "var(--brand-active)",
          bg: "var(--brand-bg)",
          foreground: "var(--brand-foreground)",
        },
        glass: "var(--glass)",
        "glass-heavy": "var(--glass-heavy)",
        "surface-glass": "var(--surface-glass)",
        "glass-border": "var(--glass-border)",
        // Overlay (semi-transparent black masks for modals, loading screens, etc.)
        overlay: {
          light: "var(--overlay-light)",
          medium: "var(--overlay-medium)",
          heavy: "var(--overlay-heavy)",
        },
        // Semi-transparent background
        "bg-50": "var(--bg-50)",
        "bg-70": "var(--bg-70)",
        "bg-80": "var(--bg-80)",
        "bg-90": "var(--bg-90)",
        "bg-95": "var(--bg-95)",
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
        xl: "calc(var(--radius) * 1.5)",
        "2xl": "var(--radius-2xl)",
      },
      boxShadow: {
        sm: "var(--shadow-sm)",
        md: "var(--shadow-md)",
        lg: "var(--shadow-lg)",
        xl: "var(--shadow-xl)",
        glass: "var(--shadow-glass)",
        "glass-lg": "var(--shadow-glass-lg)",
        brand: "var(--shadow-brand)",
      },
      keyframes: {
        "slide-in": {
          "from": { transform: "translateY(-10px)", opacity: "0" },
          "to": { transform: "translateY(0)", opacity: "1" },
        },
        "slide-in-from-top": {
          "from": { transform: "translateY(-100%)", opacity: "0" },
          "to": { transform: "translateY(0)", opacity: "1" },
        },
        "slide-in-from-bottom": {
          "from": { transform: "translateY(100%)", opacity: "0" },
          "to": { transform: "translateY(0)", opacity: "1" },
        },
        "slide-in-from-left": {
          "from": { transform: "translateX(-100%)", opacity: "0" },
          "to": { transform: "translateX(0)", opacity: "1" },
        },
        "slide-in-from-right": {
          "from": { transform: "translateX(100%)", opacity: "0" },
          "to": { transform: "translateX(0)", opacity: "1" },
        },
        "fade-in": {
          "from": { opacity: "0" },
          "to": { opacity: "1" },
        },
        "fade-in-up": {
          "from": { opacity: "0", transform: "translateY(10px)" },
          "to": { opacity: "1", transform: "translateY(0)" },
        },
        "fade-out": {
          "from": { opacity: "1" },
          "to": { opacity: "0" },
        },
        "scale-in": {
          "from": { transform: "scale(0.95)", opacity: "0" },
          "to": { transform: "scale(1)", opacity: "1" },
        },
        "scale-out": {
          "from": { transform: "scale(1)", opacity: "1" },
          "to": { transform: "scale(0.95)", opacity: "0" },
        },
        "pulse-slow": {
          "0%, 100%": { opacity: "1" },
          "50%": { opacity: "0.5" },
        },
        "spin-slow": {
          "from": { transform: "rotate(0deg)" },
          "to": { transform: "rotate(360deg)" },
        },
        "bounce-subtle": {
          "0%, 100%": { transform: "translateY(0)" },
          "50%": { transform: "translateY(-5px)" },
        },
        "shimmer": {
          "from": { backgroundPosition: "-1000px 0" },
          "to": { backgroundPosition: "1000px 0" },
        },
        "typewriter": {
          "from": { width: "0" },
          "to": { width: "100%" },
        },
        "blink": {
          "0%, 50%": { opacity: "1" },
          "51%, 100%": { opacity: "0" },
        },
      },
      animation: {
        // Entrance/exit animations reference the motion tokens
        // (var(--duration-*) / var(--ease-*)) so tuning a token propagates
        // everywhere. Loops (pulse/spin/shimmer/…) keep literal periods —
        // those are cycle lengths, not transition durations.
        "slide-in": "slide-in var(--duration-normal) var(--ease-out)",
        "slide-in-from-top": "slide-in-from-top var(--duration-slow) var(--ease-out)",
        "slide-in-from-bottom": "slide-in-from-bottom var(--duration-slow) var(--ease-out)",
        "slide-in-from-left": "slide-in-from-left var(--duration-slow) var(--ease-out)",
        "slide-in-from-right": "slide-in-from-right var(--duration-slow) var(--ease-out)",
        "fade-in": "fade-in var(--duration-normal) var(--ease-out)",
        "fade-in-up": "fade-in-up var(--duration-slow) var(--ease-out)",
        "fade-out": "fade-out var(--duration-normal) var(--ease-out)",
        "scale-in": "scale-in var(--duration-normal) var(--ease-spring-soft)",
        "scale-out": "scale-out var(--duration-normal) var(--ease-standard)",
        "pulse-slow": "pulse-slow 3s cubic-bezier(0.4, 0, 0.6, 1) infinite",
        "spin-slow": "spin-slow 3s linear infinite",
        "bounce-subtle": "bounce-subtle 2s ease-in-out infinite",
        "shimmer": "shimmer 2s linear infinite",
        "typewriter": "typewriter 2s steps(40) infinite",
        "blink": "blink 1s step-end infinite",
      },
      // Performance: Animation delay variants for staggered animations
      animationDelay: {
        0: "0ms",
        100: "100ms",
        150: "150ms",
        200: "200ms",
        300: "300ms",
        400: "400ms",
        500: "500ms",
      },
      // Motion tokens as first-class transition utilities.
      // Overriding `out`/`in-out` re-points Tailwind's default ease-out /
      // ease-in-out at the design-system curves, so every existing
      // `ease-out`/`ease-in-out` across the app is tokenized for free.
      transitionDuration: {
        fast: "var(--duration-fast)",
        normal: "var(--duration-normal)",
        slow: "var(--duration-slow)",
      },
      transitionTimingFunction: {
        out: "var(--ease-out)",
        "in-out": "var(--ease-in-out)",
        standard: "var(--ease-standard)",
        spring: "var(--ease-spring-soft)",
        "spring-snappy": "var(--ease-spring-snappy)",
        "spring-soft": "var(--ease-spring-soft)",
      },
      // Container-query scale: the plugin's own defaults are tiny widget sizes
      // (md=28rem, lg=32rem, xl=36rem). Align the named sizes with the viewport
      // breakpoints (Tailwind v4 semantics) so @md:/@lg:/@xl: read naturally
      // against the page container.
      containers: {
        md: "768px",
        lg: "1024px",
        xl: "1280px",
      },
    },
  },
  plugins: [
    containerQueries,
    require("@tailwindcss/typography")({
      theme: {
        extend: {
          colors: {},
        },
      },
    }),
  ],
}
