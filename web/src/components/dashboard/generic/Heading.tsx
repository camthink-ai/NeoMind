/**
 * Heading Component
 *
 * Displays customizable heading text.
 * Supports different levels, styles, and decorations.
 */

import { ChevronRight, Minus } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useDataSource } from '@/hooks/useDataSource'
import { DashboardComponentWrapper } from '../DashboardComponentWrapper'
import type { DataSource } from '@/types/dashboard'

export type HeadingLevel = 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6'
export type HeadingAlign = 'left' | 'center' | 'right'
export type HeadingVariant = 'default' | 'underlined' | 'dashed' | 'boxed'

export interface HeadingProps {
  dataSource?: DataSource
  text?: string
  level?: HeadingLevel
  subtitle?: string
  align?: HeadingAlign
  variant?: HeadingVariant
  color?: string
  icon?: string
  size?: 'sm' | 'md' | 'lg'
  showCard?: boolean
  className?: string
}

const levelClasses: Record<HeadingLevel, string> = {
  h1: 'text-3xl md:text-4xl lg:text-5xl',
  h2: 'text-2xl md:text-3xl lg:text-4xl',
  h3: 'text-xl md:text-2xl lg:text-3xl',
  h4: 'text-lg md:text-xl lg:text-2xl',
  h5: 'text-base md:text-lg lg:text-xl',
  h6: 'text-sm md:text-base lg:text-lg',
}

const sizeModifiers = {
  sm: { base: '0.75', subtitle: 'text-xs' },
  md: { base: '1', subtitle: 'text-sm' },
  lg: { base: '1.25', subtitle: 'text-base' },
}

export function Heading({
  dataSource,
  text: propText,
  level = 'h2',
  subtitle,
  align = 'left',
  variant = 'default',
  color,
  icon,
  size = 'md',
  showCard = true,
  className,
}: HeadingProps) {
  const { data } = useDataSource<string>(dataSource, { fallback: propText })
  const text = data ?? propText ?? ''

  const levelClass = levelClasses[level]
  const sizeMod = sizeModifiers[size]

  const getAlignClass = () => {
    switch (align) {
      case 'center': return 'text-center'
      case 'right': return 'text-right'
      default: return 'text-left'
    }
  }

  const renderHeading = () => {
    const Tag = level

    const baseClasses = cn(
      'font-bold',
      levelClass,
      getAlignClass(),
      variant === 'dashed' && 'border-b-2 border-dashed border-border pb-2',
      variant === 'underlined' && 'border-b-2 border-primary pb-2'
    )

    const content = (
      <div className={cn('inline-flex items-center gap-2', className)}>
        {icon && <span className="text-xl">{icon}</span>}
        <Tag className={baseClasses} style={{ color, fontSize: `calc(${sizeMod.base}em)` }}>
          {text}
        </Tag>
      </div>
    )

    return (
      <div className="space-y-1">
        {content}
        {subtitle && (
          <p className={cn('text-muted-foreground', sizeMod.subtitle, getAlignClass())}>
            {subtitle}
          </p>
        )}
      </div>
    )
  }

  const content = (
    <div className={cn('p-4', className)}>
      {text ? renderHeading() : (
        <div className="text-center py-8 text-muted-foreground text-sm">
          No heading text
        </div>
      )}
    </div>
  )

  if (showCard) {
    return (
      <DashboardComponentWrapper
        title={text}
        showCard={variant === 'boxed'}
        padding="none"
      >
        {content}
      </DashboardComponentWrapper>
    )
  }

  return content
}
