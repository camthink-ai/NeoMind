/**
 * Regression tests for component library category grouping.
 *
 * Bug history: extension components whose manifest declared a category
 * outside the built-in list (e.g. "other") were silently dropped from
 * the Add Component dialog because groupComponentsByCategory() only
 * returned groups matching categoryOrder. Extension-declared categories
 * must now render as their own groups after the built-ins.
 */
import { describe, it, expect, beforeEach } from 'vitest'
import { dynamicRegistry } from '../registry/DynamicRegistry'
import {
  groupComponentsByCategory,
  getCategoryInfo,
  getComponentMeta,
} from '../registry/registry'
import type { DashboardComponentDto } from '@/types'

function extDto(type: string, category: string): DashboardComponentDto {
  return {
    extension_id: 'test-extension',
    type,
    name: type,
    description: `${type} description`,
    category,
    bundle_url: '/api/test/bundle.js',
    size_constraints: { minW: 2, minH: 2, defaultW: 4, defaultH: 3, maxW: 12, maxH: 12 },
    has_data_source: false,
    has_display_config: false,
    has_actions: false,
  } as unknown as DashboardComponentDto
}

describe('groupComponentsByCategory with extension-declared categories', () => {
  beforeEach(() => {
    dynamicRegistry.clear()
  })

  it('renders extension components with an unknown category as their own group', () => {
    dynamicRegistry.register('test-extension', 'Test Extension', extDto('gym-widget-a', 'other'))
    dynamicRegistry.register('test-extension', 'Test Extension', extDto('gym-widget-b', 'other'))

    const groups = groupComponentsByCategory()
    const other = groups.find(g => g.category === 'other' as never)

    expect(other).toBeDefined()
    expect(other!.components.map(c => c.type).sort()).toEqual(['gym-widget-a', 'gym-widget-b'])
  })

  it('places unknown groups after the built-in categories, sorted alphabetically', () => {
    dynamicRegistry.register('test-extension', 'Test Extension', extDto('z-widget', 'zzz-custom'))
    dynamicRegistry.register('test-extension', 'Test Extension', extDto('a-widget', 'aaa-custom'))

    const groups = groupComponentsByCategory()
    const categories = groups.map(g => g.category as string)
    const builtInCount = categories.filter(c =>
      ['indicators', 'charts', 'controls', 'display', 'spatial', 'business'].includes(c)
    ).length

    // built-ins first, then the extension groups in stable (alphabetical) order
    expect(categories.indexOf('indicators')).toBeLessThan(categories.indexOf('aaa-custom'))
    expect(categories.indexOf('zzz-custom') - categories.indexOf('aaa-custom')).toBe(1)
    expect(builtInCount).toBeGreaterThanOrEqual(6)
  })

  it('still groups conventionally-categorized extension components under custom', () => {
    dynamicRegistry.register('test-extension', 'Test Extension', extDto('custom-widget', 'custom'))

    const groups = groupComponentsByCategory()
    expect(groups.find(g => g.category === 'custom')).toBeDefined()
  })

  it('getCategoryInfo falls back to a prettified label and generic icon for unknown categories', () => {
    const info = getCategoryInfo('my-widgets' as never)
    expect(info.name).toBe('My Widgets')
    expect(info.icon).toBeDefined()

    const plain = getCategoryInfo('other' as never)
    expect(plain.name).toBe('Other')
  })

  it('getComponentMeta resolves extension component types', () => {
    dynamicRegistry.register('test-extension', 'Test Extension', extDto('gym-widget-a', 'other'))
    const meta = getComponentMeta('gym-widget-a' as never)
    expect(meta).toBeDefined()
    expect(meta!.category).toBe('other' as never)
  })

  it('defaults a missing/empty extension category into the custom group', () => {
    const noCategory = { ...extDto('null-widget', 'custom'), category: undefined } as unknown as DashboardComponentDto
    const emptyCategory = { ...extDto('empty-widget', 'x'), category: '' } as unknown as DashboardComponentDto
    dynamicRegistry.register('test-extension', 'Test Extension', noCategory)
    dynamicRegistry.register('test-extension', 'Test Extension', emptyCategory)

    const groups = groupComponentsByCategory()
    const custom = groups.find(g => g.category === 'custom')!
    const types = custom.components.map(c => c.type)
    expect(types).toContain('null-widget')
    expect(types).toContain('empty-widget')
    // and no "null"/"" bucket is ever created
    expect(groups.find(g => !g.category)).toBeUndefined()
  })
})
