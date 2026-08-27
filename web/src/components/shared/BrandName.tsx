/**
 * BrandName Components
 *
 * Provides consistent brand name display across the application.
 * Uses centralized i18n configuration for easy rebranding.
 *
 * Logo images:
 * - Square logo: for favicons, app icons
 * - Dark theme logo: for dark backgrounds (logo-dark.png)
 * - Light theme logo: for light backgrounds (logo-light.png)
 */

import { useBrand, useBrandMessages } from '@/hooks/useBrand'
import { cn } from '@/lib/utils'

/**
 * Display the full brand name
 *
 * @example
 * ```tsx
 * <BrandName />
 * <BrandName className="text-xl font-bold" />
 * ```
 */
export function BrandName({ className }: { className?: string }) {
  const { name } = useBrand()
  return <span className={className}>{name}</span>
}

/**
 * Display the short brand name/acronym
 *
 * @example
 * ```tsx
 * <ShortBrandName />
 * <ShortBrandName className="text-xs" />
 * ```
 */
export function ShortBrandName({ className }: { className?: string }) {
  const { shortName } = useBrand()
  return <span className={cn('font-mono', className)}>{shortName}</span>
}

/**
 * Brand logo with short name (square logo image)
 *
 * @example
 * ```tsx
 * <BrandLogo />
 * ```
 */
export function BrandLogo({ className }: { className?: string }) {
  return (
    <img
      src="/logo-square.png"
      alt="NeoMind Logo"
      width={36}
      height={36}
      className={cn('w-9 h-9 rounded-xl shadow-sm', className)}
    />
  )
}

/**
 * Horizontal brand logo (auto-switches based on theme)
 * Uses logo-dark.png for dark theme, logo-light.png for light theme
 *
 * @example
 * ```tsx
 * <BrandLogoHorizontal />
 * <BrandLogoHorizontal className="h-8" />
 * ```
 */
export function BrandLogoHorizontal({ className }: { className?: string }) {
  return (
    <>
      {/* Light theme logo */}
      <img
        src="/logo-light.png"
        alt="NeoMind"
        className={cn('h-6 dark:hidden', className)}
      />
      {/* Dark theme logo */}
      <img
        src="/logo-dark.png"
        alt="NeoMind"
        className={cn('h-6 hidden dark:block', className)}
      />
    </>
  )
}

/**
 * Styled brand name with logo-inspired design
 * - Light mode: white text with orange shadow/stroke
 * - Dark mode: black text with orange shadow/stroke
 *
 * @example
 * ```tsx
 * <StyledBrandName />
 * <StyledBrandName size="sm" />
 * ```
 */
export function StyledBrandName({
  size = 'base',
  className
}: {
  size?: 'sm' | 'base' | 'lg'
  className?: string
}) {
  const { name } = useBrand()

  const sizeClasses = {
    sm: 'text-sm',
    base: 'text-base',
    lg: 'text-lg'
  }

  return (
    <span
      className={cn(
        'inline-block font-black italic tracking-tight',
        // Use foreground color that adapts to light/dark mode
        'text-foreground',
        // Orange shadow effect (3D) - top-right direction with #EB4C27
        'drop-shadow-[1.5px_-1.5px_0_var(--brand-shadow)]',
        // Transition
        'transition-all duration-normal',
        sizeClasses[size],
        className
      )}
    >
      {name}
    </span>
  )
}

/**
 * Combined brand logo and name
 * - Mobile: horizontal logo (logo-light/logo-dark) with theme switching, scaled down
 * - Desktop: horizontal logo (logo-light/logo-dark) with theme switching
 *
 * @example
 * ```tsx
 * <BrandLogoWithName />
 * ```
 */
export function BrandLogoWithName({
  logoClassName,
  nameClassName,
  showLogo = false,
  styled = true
}: {
  logoClassName?: string
  nameClassName?: string
  showLogo?: boolean
  styled?: boolean
}) {
  return (
    <>
      {/* Mobile: horizontal logo with theme switching, smaller size */}
      <span className="md:hidden">
        <BrandLogoHorizontal className={cn('h-5', logoClassName)} />
      </span>
      {/* Desktop: horizontal logo with theme switching, normal size */}
      <span className="hidden md:inline-block">
        <BrandLogoHorizontal className={cn('h-7', logoClassName)} />
      </span>
    </>
  )
}
