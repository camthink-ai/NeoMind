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


/**
 * Dynamic key families — keys the source builds at runtime
 * (`t(\`creator.notify.on${k}\`)`, `t(\`componentLibrary.${labelKey}\`)`),
 * which the literal scanner above cannot see. Each family enumerates its
 * CONCRETE keys here; the value domains come from the enumerating constants
 * in the components (notify's `['failure','always','judgment']`,
 * onboarding's feature lists, the componentLibrary key tables, ...). A key
 * that renders raw on screen because nobody added it to a bundle is exactly
 * the failure the scanner above catches for literals — this extends that
 * cover to the dynamic half.
 *
 * Deliberately absent: `agents:detail.fieldType.enum` — the enum branch
 * renders `enum(a/b)` inline and never reaches t().
 */
const DYNAMIC_KEY_FAMILIES: Array<{ ns: string; keys: string[] }> = [
  {
    ns: 'agents',
    keys: [
      'creator.notify.onFailure', 'creator.notify.onFailureDesc',
      'creator.notify.onAlways', 'creator.notify.onAlwaysDesc',
      'creator.notify.onJudgment', 'creator.notify.onJudgmentDesc',
      'card.role.recordData', 'card.role.actOrInvestigate', 'card.role.answer',
      'card.memory.tool', 'card.memory.assistant',
      'detail.memoryModeNote.tool', 'detail.memoryModeNote.assistant',
      'detail.fieldType.text', 'detail.fieldType.number', 'detail.fieldType.boolean',
    ],
  },
  {
    ns: 'common',
    keys: [
      'onboarding.setup.llm.features.builtin.title', 'onboarding.setup.llm.features.builtin.desc',
      'onboarding.setup.llm.features.local.title', 'onboarding.setup.llm.features.local.desc',
      'onboarding.setup.llm.features.cloud.title', 'onboarding.setup.llm.features.cloud.desc',
      'onboarding.setup.device.features.mqtt.title', 'onboarding.setup.device.features.mqtt.desc',
      'onboarding.setup.device.features.other.title', 'onboarding.setup.device.features.other.desc',
      'onboarding.setup.device.features.camera.title', 'onboarding.setup.device.features.camera.desc',
      'onboarding.ready.prompts.monitoring.title', 'onboarding.ready.prompts.monitoring.desc', 'onboarding.ready.prompts.monitoring.prompt',
      'onboarding.ready.prompts.automation.title', 'onboarding.ready.prompts.automation.desc', 'onboarding.ready.prompts.automation.prompt',
      'onboarding.ready.prompts.extensions.title', 'onboarding.ready.prompts.extensions.desc', 'onboarding.ready.prompts.extensions.prompt',
      'messages.severity.info', 'messages.severity.warning', 'messages.severity.critical', 'messages.severity.emergency',
      'messages.status.active', 'messages.status.acknowledged', 'messages.status.resolved', 'messages.status.archived', 'messages.status.false_positive',
    ],
  },
  {
    ns: 'dashboardComponents',
    keys: [
      'componentLibrary.indicators', 'componentLibrary.charts', 'componentLibrary.display',
      'componentLibrary.spatial', 'componentLibrary.controls', 'componentLibrary.business',
      'componentLibrary.custom', 'componentLibrary.localComponents', 'componentLibrary.marketplace',
      ...[
        'valueCard', 'ledIndicator', 'sparkline', 'progressBar', 'lineChart', 'areaChart',
        'barChart', 'pieChart', 'imageDisplay', 'imageHistory', 'webDisplay', 'markdownDisplay',
        'mapDisplay', 'videoDisplay', 'customLayer', 'toggleSwitch', 'agentMonitor', 'aiAnalyst',
      ].flatMap((k) => [`componentLibrary.${k}`, `componentLibrary.${k}Desc`]),
      'configRenderer.backgroundColor', 'configRenderer.textColor', 'configRenderer.borderColor', 'configRenderer.color',
    ],
  },
]

/**
 * tBuilder wrapper families: `const tBuilder = (key) =>
 * t(\`automation:ruleBuilder.${key}\`)` hides the literal from the scanner.
 * The call sites inside are literal — extract them from source.
 */
function wrapperKeys(): { ns: string; keys: string[] }[] {
  const families: Record<string, string> = {
    'SimpleRuleBuilderSplit.tsx': 'automation:ruleBuilder.',
    'TransformBuilderSplit.tsx': 'automation:transformBuilder.',
  }
  return Object.entries(families).map(([file, prefix]) => {
    const text = Object.entries(SOURCES).find(([f]) => f.endsWith(file))?.[1] ?? ''
    const i = prefix.indexOf(':')
    const ns = prefix.slice(0, i)
    const rest = prefix.slice(i + 1)
    const keys = [...text.matchAll(/tBuilder\(\s*'([^']+)'/g)].map((m) => rest + m[1])
    return { ns, keys }
  })
}

describe('translation keys referenced from source', () => {
  it('dynamic key families exist in every locale', () => {
    // Some registered ns names differ from their file names
    // (dashboardComponents ↔ dashboard-components.json).
    const NS_FILE: Record<string, string> = { dashboardComponents: 'dashboard-components' }
    const missing: string[] = []
    for (const { ns, keys } of [...DYNAMIC_KEY_FAMILIES, ...wrapperKeys()]) {
      const file = NS_FILE[ns] ?? ns
      for (const key of keys) {
        for (const locale of ['zh', 'en']) {
          if (resolve(bundleOf(locale, file) ?? {}, key) === undefined) {
            missing.push(`[${locale}] ${ns}:${key}`)
          }
        }
      }
    }
    expect(missing, `dynamic families missing from bundles:\n${missing.join('\n')}`).toEqual([])
  })

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
