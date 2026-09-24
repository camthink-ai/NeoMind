// Every translation key the source names literally must exist in every locale.
//
// i18next renders a missing key as the key itself — visible, but only to
// whoever is using that language, and only on the screen that renders it. That
// is how a cleanup pass that removes one key too many ships: nothing fails, the
// bundle is smaller, and a Chinese user sees `extensions:noExtensionsDesc` in a
// heading. This is the guard that turns that into a red test.
//
// Scope: the `ns:key` and `ns.key` forms, where the namespace is written out.
// Bare keys (`t('someKey')` under `useTranslation('extensions')`) need the
// enclosing namespace, which a scanner cannot know — those are covered from the
// other direction by the dead-key scan that produced the cleanup.

import { describe, expect, it } from 'vitest'

/** Every source file, as text. */
const SOURCES = import.meta.glob('../../**/*.{ts,tsx}', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>

/**
 * Every locale bundle, parsed. `import: 'default'` is required: an eager glob
 * hands back the module namespace, so without it each value is `{ default: … }`
 * and every key reads as missing — a guard that fails loudly on all 5000 keys
 * looks broken rather than useful.
 */
const BUNDLES = import.meta.glob('../locales/*/*.json', {
  eager: true,
  import: 'default',
}) as Record<string, Record<string, unknown>>

function bundleOf(locale: string, ns: string): Record<string, unknown> | undefined {
  return BUNDLES[`../locales/${locale}/${ns}.json`]
}

/**
 * A `t('ns:key')` / `t("ns:key")` / `t('ns.key')` call, plus whether it was
 * given a second argument. `t('k', 'fallback')` and `t('k', { defaultValue })`
 * still render *something* English when the key is absent; a bare `t('k')`
 * renders the key itself, which is the failure this test is for.
 */
const CALL = /\bt\(\s*(['"])([a-z][a-zA-Z0-9-]*)[:.]([A-Za-z0-9_.]+)\1\s*(,)?/g

interface Ref {
  ns: string
  key: string
  /** Files referencing it with no fallback at all. */
  bare: string[]
}

function referencedKeys(): Map<string, Ref> {
  const found = new Map<string, Ref>()
  for (const [file, text] of Object.entries(SOURCES)) {
    for (const m of text.matchAll(CALL)) {
      const [, , ns, key, comma] = m
      const id = `${ns}:${key}`
      const ref = found.get(id) ?? { ns, key, bare: [] }
      if (!comma) ref.bare.push(file)
      found.set(id, ref)
    }
  }
  return found
}

function resolve(obj: Record<string, unknown>, dotted: string): unknown {
  return dotted.split('.').reduce<unknown>(
    (acc, part) =>
      acc && typeof acc === 'object' ? (acc as Record<string, unknown>)[part] : undefined,
    obj,
  )
}

const LOCALES = ['en', 'zh']

describe('translation keys referenced from source', () => {
  const referenced = referencedKeys()

  it('found keys to check — a scanner that reads nothing passes vacuously', () => {
    expect(referenced.size).toBeGreaterThan(500)
  })

  it('every literal ns:key in the source exists in en and zh', () => {
    const missing: string[] = []
    for (const ref of referenced.values()) {
      for (const locale of LOCALES) {
        const json = bundleOf(locale, ref.ns)
        if (!json) missing.push(`${locale}: namespace "${ref.ns}" has no bundle`)
        else if (resolve(json, ref.key) === undefined) missing.push(`${locale}: ${ref.ns}:${ref.key}`)
      }
    }
    // A defaulted call site is a localisation gap, not a visible break — the
    // fallback is English, so `zh` shows English. Those are counted by the next
    // test rather than failing this one, which is about keys rendered raw.
    const hard = missing.filter((m) => referenced.get(m.split(': ')[1])?.bare.length)
    expect(hard, `referenced with no fallback and absent:\n${hard.slice(0, 40).join('\n')}`).toEqual([])
  })

  it('holds the defaulted-but-missing count at its frozen baseline', () => {
    // `t('k', 'Fallback')` never renders a raw key, so these are a localisation
    // gap rather than a break — the fallback is English and a Chinese user sees
    // English. There are too many to fix in one pass, so the count is frozen
    // here the way `.eslint-baseline` freezes the lint warnings: shrink it when
    // you touch these call sites, never raise it.
    const FROZEN = 267
    const soft = [...referenced.values()].filter(
      (r) => r.bare.length === 0 && resolve(bundleOf('en', r.ns) ?? {}, r.key) === undefined,
    )
    expect(
      soft.length,
      `${soft.length} keys are referenced with a fallback but absent from en.json`,
    ).toBeLessThanOrEqual(FROZEN)
    if (soft.length < FROZEN) {
      throw new Error(
        `The count dropped to ${soft.length} — lower FROZEN to match, so it cannot creep back up.`,
      )
    }
  })
})
