import { type ClassValue, clsx } from "clsx"
import { extendTailwindMerge } from "tailwind-merge"

// tailwind-merge doesn't know the custom fontSize utilities registered in
// tailwind.config.js (text-micro/nano/mini/code/body/heading). Without this
// it treats them as textColor classes, so cn(textMicro, 'text-muted-foreground')
// silently dropped the SIZE — labels rendered at the inherited default.
const twMerge = extendTailwindMerge({
  extend: {
    classGroups: {
      'font-size': [{ text: ['micro', 'nano', 'mini', 'code', 'body', 'heading'] }],
    },
  },
})

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

// Re-export design system utilities used by consumers via @/lib/utils
export {
  formatValue,
  toNumberArray,
  clamp,
  normalize,
} from '@/design-system/utils/format'

export { getIconForEntity } from '@/design-system/icons'
export { EntityIcon } from '@/design-system/icons'
