/** @type {import('tailwindcss').Config} */
export default {
  darkMode: ["class"],
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        border: "hsl(var(--border))",
        input: "hsl(var(--input))",
        ring: "hsl(var(--ring))",
        background: "hsl(var(--background))",
        foreground: "hsl(var(--foreground))",
        primary: {
          DEFAULT: "hsl(var(--primary))",
          foreground: "hsl(var(--primary-foreground))",
        },
        secondary: {
          DEFAULT: "hsl(var(--secondary))",
          foreground: "hsl(var(--secondary-foreground))",
        },
        destructive: {
          DEFAULT: "hsl(var(--destructive))",
          foreground: "hsl(var(--destructive-foreground))",
        },
        muted: {
          DEFAULT: "hsl(var(--muted))",
          foreground: "hsl(var(--muted-foreground))",
        },
        accent: {
          DEFAULT: "hsl(var(--accent))",
          foreground: "hsl(var(--accent-foreground))",
        },
        popover: {
          DEFAULT: "hsl(var(--popover))",
          foreground: "hsl(var(--popover-foreground))",
        },
        card: {
          DEFAULT: "hsl(var(--card))",
          foreground: "hsl(var(--card-foreground))",
        },
        // Semantic colors for status indicators
        success: {
          DEFAULT: "hsl(var(--color-success))",
          light: "hsl(var(--color-success-bg))",
        },
        warning: {
          DEFAULT: "hsl(var(--color-warning))",
          light: "hsl(var(--color-warning-bg))",
        },
        error: {
          DEFAULT: "hsl(var(--color-error))",
          light: "hsl(var(--color-error-bg))",
        },
        info: {
          DEFAULT: "hsl(var(--color-info))",
          light: "hsl(var(--color-info-bg))",
        },
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
        xl: "calc(var(--radius) * 1.5)",
      },
      boxShadow: {
        sm: "var(--shadow-sm)",
        md: "var(--shadow-md)",
        lg: "var(--shadow-lg)",
        xl: "var(--shadow-xl)",
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
        "slide-in": "slide-in 0.2s ease-out",
        "slide-in-from-top": "slide-in-from-top 0.3s ease-out",
        "slide-in-from-bottom": "slide-in-from-bottom 0.3s ease-out",
        "slide-in-from-left": "slide-in-from-left 0.3s ease-out",
        "slide-in-from-right": "slide-in-from-right 0.3s ease-out",
        "fade-in": "fade-in 0.2s ease-out",
        "fade-out": "fade-out 0.2s ease-out",
        "scale-in": "scale-in 0.2s ease-out",
        "scale-out": "scale-out 0.2s ease-out",
        "pulse-slow": "pulse-slow 3s cubic-bezier(0.4, 0, 0.6, 1) infinite",
        "spin-slow": "spin-slow 3s linear infinite",
        "bounce-subtle": "bounce-subtle 2s ease-in-out infinite",
        "shimmer": "shimmer 2s linear infinite",
        "typewriter": "typewriter 2s steps(40) infinite",
        "blink": "blink 1s step-end infinite",
      },
    },
  },
  plugins: [require("@tailwindcss/typography")],
}
