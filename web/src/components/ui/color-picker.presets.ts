// Color presets — split from color-picker.tsx (react-refresh: component
// files export only components).

// mirror of color-picker's local token constant
const THEME_FOREGROUND = 'hsl(var(--foreground))'

export const COLOR_PRESETS = {
  primary: [
    THEME_FOREGROUND, // Theme foreground (black in light, white in dark)
    '#6360ef', // Indigo-Blue
    '#36b37e', // Emerald
    '#e8a735', // Amber
    '#e07838', // Orange
    '#d86098', // Rose
    '#4ca8c8', // Sky Blue
    '#4aba6a', // Green
  ],
  neutral: [
    THEME_FOREGROUND, // Theme foreground (black in light, white in dark)
    '#171717', // Zinc 950
    '#404040', // Zinc 700
    '#737373', // Zinc 500
    '#a3a3a3', // Zinc 400
    '#d4d4d4', // Zinc 300
    '#e5e5e5', // Zinc 200
    '#f5f5f5', // Zinc 100
  ],
  semantic: [
    '#36b37e', // Success (Emerald)
    '#e8a735', // Warning (Amber)
    '#e07838', // Error (Orange)
    '#6360ef', // Info (Indigo-Blue)
    '#4ca8c8', // Sky Blue
  ],
}
