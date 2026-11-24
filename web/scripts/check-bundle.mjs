#!/usr/bin/env node
// Fails when the JavaScript a user has to download on first paint exceeds the
// budget. Two budgets, because the two pages users land on are not alike:
//
//   entry + preload        what index.html names: the entry <script
//                          type="module"> plus every <link rel="modulepreload">.
//                          Downloaded (and parsed) before any page code runs,
//                          on every route. This is what the login page costs.
//
//   chat route cold start  the above plus the static import closure of the
//                          `/` route (pages/chat.tsx). A signed-in user lands
//                          there, so this is the first-paint cost for them.
//
// Both cover JS only, as delivered (raw and gzip). They do not measure time;
// scripts/measure-cold-start.mjs does.
//
// Usage: node scripts/check-bundle.mjs [distDir]
import { readFileSync, statSync, existsSync } from 'node:fs'
import { gzipSync } from 'node:zlib'
import { join, resolve } from 'node:path'

const KB = 1024
// Single BUNDLE_* prefix. BUDGET_* aliases kept so existing CI/local overrides
// still apply — prefer BUNDLE_* going forward. Always run via `npm run
// build:check` (never bare `node check-bundle.mjs` after a custom outDir).
const envNum = (...names) => {
  for (const name of names) {
    if (process.env[name] != null && process.env[name] !== '') {
      return Number(process.env[name])
    }
  }
  return undefined
}
const LIMITS = {
  entry: {
    raw: envNum('BUNDLE_ENTRY_RAW_KB', 'BUDGET_ENTRY_RAW_KB') ?? 1400,
    gzip: envNum('BUNDLE_ENTRY_GZIP_KB', 'BUDGET_ENTRY_GZIP_KB') ?? 430,
  },
  chat: {
    raw: envNum('BUNDLE_CHAT_RAW_KB', 'BUDGET_CHAT_RAW_KB') ?? 2300,
    gzip: envNum('BUNDLE_CHAT_GZIP_KB', 'BUDGET_CHAT_GZIP_KB') ?? 720,
  },
}
const dist = resolve(process.argv[2] || 'dist')
const kb = (bytes) => `${(bytes / KB).toFixed(0).padStart(5)} KB`
const local = (ref) => join(dist, ref.replace(/^\//, ''))

const measure = (refs) =>
  [...refs].map((ref) => {
    const file = local(ref)
    return { ref, raw: statSync(file).size, gz: gzipSync(readFileSync(file), { level: 9 }).length }
  })
const report = (label, sizes, limit) => {
  const raw = sizes.reduce((s, x) => s + x.raw, 0)
  const gz = sizes.reduce((s, x) => s + x.gz, 0)
  const ok = raw <= limit.raw * KB && gz <= limit.gzip * KB
  console.log(`${label}: ${sizes.length} files, ${kb(raw)} raw, ${kb(gz)} gzip   budget ${limit.raw} / ${limit.gzip} KB   ${ok ? '✓' : '✗'}`)
  return ok
}

// 1. entry + preload
const html = readFileSync(join(dist, 'index.html'), 'utf8')
const preloaded = new Set()
for (const tag of html.match(/<(?:script|link)\b[^>]*>/g) ?? []) {
  const isEntry = tag.startsWith('<script') && /\btype="module"/.test(tag)
  const isPreload = tag.startsWith('<link') && /\brel="modulepreload"/.test(tag)
  if (!isEntry && !isPreload) continue
  const ref = (tag.match(/\b(?:src|href)="([^"]+\.js)"/) ?? [])[1]
  if (ref) preloaded.add(ref)
}
if (preloaded.size === 0) {
  console.error(`✗ no entry script or modulepreload found in ${join(dist, 'index.html')}`)
  process.exit(1)
}
const entrySizes = measure(preloaded)
for (const x of entrySizes) console.log(`${kb(x.raw)} raw  ${kb(x.gz)} gzip  ${x.ref}`)
console.log('-'.repeat(72))

// 2. chat route cold start
const manifestFile = join(dist, '.vite', 'manifest.json')
if (!existsSync(manifestFile)) {
  console.error('✗ chat route cold start could not be evaluated: dist/.vite/manifest.json not found (set build.manifest: true)')
  process.exit(1)
}
const manifest = JSON.parse(readFileSync(manifestFile, 'utf8'))
const closure = (start) => {
  const seen = new Set()
  const stack = [...start]
  while (stack.length) {
    const key = stack.pop()
    if (seen.has(key) || !manifest[key]) continue
    seen.add(key)
    for (const imp of manifest[key].imports ?? []) stack.push(imp)
  }
  return seen
}
const chatKey = Object.hasOwn(manifest, 'src/pages/chat.tsx') ? 'src/pages/chat.tsx' : null
if (!chatKey) {
  console.error('✗ chat route cold start could not be evaluated: no manifest entry for src/pages/chat.tsx')
  process.exit(1)
}
const chatRefs = new Set(
  [...closure([...closure(['index.html']), chatKey])].map((key) => `/${manifest[key].file}`),
)

const okEntry = report('entry + preload JS      ', entrySizes, LIMITS.entry)
const okChat = report('chat route cold-start JS', measure(chatRefs), LIMITS.chat)
if (!okEntry || !okChat) {
  console.error(
    '\nOver budget. Code that only a lazy route needs has probably landed in an eager chunk; ' +
      'see manualChunks in vite.config.ts. Lower a budget only together with the change that earns it.',
  )
}
process.exit(okEntry && okChat ? 0 : 1)
